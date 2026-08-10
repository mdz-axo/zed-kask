//! Manifest executor — deterministic multi-step orchestration
//!
//! Executes a `BundleManifest` cascade: select → populate → execute.
//! Each `BundleManifestStep` is dispatched according to its `action` field:
//!
//! - **select**: Render a selector template, call inference, parse the
//!   JSON result to choose the next step or resolve a variable.
//! - **populate**: Render a template with the accumulated context map, producing
//!   a filled prompt or data payload.
//! - **execute**: Invoke an MCP tool with parameters bound from the context map.
//! - **compute**: Invoke a canonical `hkask_forecast` primitive deterministically
//!   (no LLM round-trip). The step's `compute_ref` names the function;
//!   `input_mapping` binds its arguments from prior step results. This connects
//!   the skill pipeline to the deterministic math layer (Fermi, outside view,
//!   Bayesian, Brier, calibration adjustment).
//! - **choice**: Evaluate a condition against context, branch by setting `_next_ordinal`.
//! - **loop**: Re-enter the cascade from `loop_target` ordinal (defaults to 0),
//!   incrementing the iteration counter. Iterative re-entry is bounded by
//!   `convergence.max_iterations`; the matryoshka depth limit bounds recursive
//!   nesting (flowdef sub-cascades), not this.
//!   `loop_target` is a Jinja expression rendered against the current context,
//!   enabling targeted re-entry: the convergence check can emit a numeric
//!   `re_entry_target` field that routes the loop to the failing step.
//! - **abort**: Exit the cascade with a convergence status. Emits `reg.skill.converged`.
//! - **escalate**: Exit the cascade with an escalation error. Emits `reg.skill.escalated`.
//!
//! The executor respects iterative convergence (`manifest.convergence`),
//! gas budgets (`manifest.gas.cap` — hard parent allocation with
//! per-token deduction after inference calls), timeout constraints
//! (`step.timeout_seconds` — hard, enforced via tokio::time::timeout),
//! and conditional step execution (`step.condition`).
//! The PDCA loop executes steps in ordinal order, handling `loop` actions by
//! re-entering from the target ordinal until convergence threshold is met,
//! max iterations are exhausted, or `abort`/`escalate` is triggered.
//!
//! Template rendering supports two modes:
//!
//! - **minijinja** (`step.renderer == "minijinja"`): Load template from
//!   `step.template_ref` (a file path like `curator/system_state_gather.j2`)
//!   relative to `template_base_path`, then render with full Jinja2 syntax.
//! - **inline** (no `renderer` or any other value): Render `template_ref` or
//!   `renderer` as an inline template string with simple `{{key}}` substitution.
//!
//! Architecture: hkask-templates owns the executor because it needs
//! `InferencePort` (for select/populate) and `ToolPort` (for execute),
//! both of which are already dependencies of this crate.

use crate::budget::{BudgetSnapshot, BudgetTracker};
use crate::bundle::BundleManifest;
use crate::bundle::BundleManifestStep;
use crate::compute::dispatch_compute;
use crate::condition::{evaluate_step_condition, parse_choice_condition};
use crate::convergence::{ConvergenceStatus, ConvergenceTracker};
use crate::input_mapping::resolve_mapping_value;
use crate::load_manifest_from_yaml;
use crate::output_schema::{build_structured_output_tool, resolve_output_schema};
use crate::ports::{Result, TemplateError};
use crate::template_renderer::TemplateRenderer;
use hkask_capability::tool_taint::ToolTaint;
use hkask_capability::{DelegationAction, DelegationResource};
use hkask_capability::{ToolPort, ToolPortError};
use hkask_guard::{SpotlightMode, Spotlighter};
use hkask_regulation::SkillFeedbackSpan;
use hkask_types::NotFound;
use hkask_types::WebID;
use hkask_types::json_extract as llm_json;
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, InferencePort, InferenceResult};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

/// Extract the PDCA feedback phase from a template_ref string.
///
/// Template refs look like "sankey-flow/sankey-classify" or
/// "diataxis-diagram/diataxis-diagram-generate". The phase is extracted
/// from the last segment after the final '-' or '/'. Returns None if the
/// segment doesn't match one of the canonical phases (see `SkillFeedbackSpan`).
///
/// This is the bridge between the step's template_ref and the
/// SkillFeedbackSpan enum — it lets the executor emit the correct
/// reg.skill.<id>.<phase> span without hardcoding ordinal-to-phase mappings.
fn extract_feedback_phase(template_ref: &str) -> Option<&'static str> {
    // Extract the last path segment (after the final '/').
    let last_segment = template_ref.rsplit('/').next().unwrap_or(template_ref);
    // Match against the canonical phases by checking if the last segment
    // contains the phase name. This handles both "sankey-classify" and
    // "adversarial-convergence-check" — the phase name appears as a substring.
    // Order matters between substrings mapping to DIFFERENT phases (e.g.
    // "evaluate" is checked before "convergence" so a template named
    // "evaluate-convergence" classifies as Evaluate, not Convergence).
    // The paired substrings ("convergence"/"converge",
    // "operator_feedback"/"feedback") map to the same phase, so their
    // relative order is harmless.
    if last_segment.contains("classify") {
        Some(SkillFeedbackSpan::Classify.phase())
    } else if last_segment.contains("gather") {
        Some(SkillFeedbackSpan::Gather.phase())
    } else if last_segment.contains("draft")
        || last_segment.contains("generate")
        || last_segment.contains("extract")
    {
        Some(SkillFeedbackSpan::Draft.phase())
    } else if last_segment.contains("evaluate") {
        Some(SkillFeedbackSpan::Evaluate.phase())
    } else if last_segment.contains("convergence") || last_segment.contains("converge") {
        Some(SkillFeedbackSpan::Convergence.phase())
    } else if last_segment.contains("operator_feedback") || last_segment.contains("feedback") {
        Some(SkillFeedbackSpan::OperatorFeedback.phase())
    } else if last_segment.contains("write") {
        Some(SkillFeedbackSpan::Write.phase())
    } else if last_segment.contains("outcome") {
        Some(SkillFeedbackSpan::Outcome.phase())
    } else {
        None
    }
}

/// Manifest executor — drives the select → populate → execute cascade.
///
/// Created once per session (or per manifest invocation) and wired into the
/// REPL turn loop. The executor holds references to the infrastructure
/// ports it needs:
///
/// - `InferencePort` — for rendering selector templates and populating prompts
/// - `ToolPort` — for invoking MCP tools in execute steps
/// - `template_base_path` — filesystem path for resolving `template_ref` values
///   when `renderer == "minijinja"`
#[derive(Clone)]
pub struct ManifestExecutor {
    /// Inference port for select/populate actions.
    inference: Arc<dyn InferencePort>,
    /// Tool port for execute actions.
    tools: Arc<dyn ToolPort>,
    /// Default LLM parameters for inference calls
    default_params: LLMParameters,
    /// Base filesystem path for resolving template_ref values.
    /// When `step.renderer == "minijinja"`, `step.template_ref` is resolved
    /// relative to this path. Defaults to `registry/templates/`.
    template_renderer: TemplateRenderer,

    /// Spotlighter for transforming untrusted tool outputs (Layer 2 defense).
    /// Applied to every MCP tool result before it enters the LLM context.
    /// Source: Microsoft Research arXiv:2403.14720
    spotlighter: Spotlighter,
    /// Optional runtime policy for pre-execution checks (Layer 6 defense).
    /// When present, checked before every MCP tool invocation.
    /// Source: VeriGuard pattern + AgentGuard arXiv:2509.23864
    runtime_policy: Option<Arc<hkask_regulation::DefaultPolicy>>,
    /// FIDES taint labels for context entries (Layer 5 defense).
    /// Maps `step_N_result` keys to their ToolTaint label.
    /// Source: Microsoft Research FIDES (arXiv:2505.23643)
    taint_labels: Arc<std::sync::Mutex<HashMap<String, ToolTaint>>>,
    /// Optional callback to check if the `terminal` built-in tool is enabled
    /// for the current agent profile. Wired by the bridge with
    /// `AgentProfileSettings::is_tool_enabled("terminal")`. When present,
    /// profile enforcement uses this (the correct check — `terminal` is a
    /// built-in agent tool, not an MCP tool, so `discover_tools()` won't find
    /// it in production). When absent (unit tests), falls back to
    /// `ToolPort::discover_tools()` (which works with test stubs that
    /// advertise `terminal`).
    terminal_check: Option<Arc<dyn Fn() -> bool + Send + Sync>>,

    /// Optional progress callback for real-time cascade feedback.
    ///
    /// When set, `run_cascade` calls it at the start of each step with a
    /// human-readable description (e.g. "Step 2/5: scope (populate) — scoping
    /// the diff"). The callback is `Send + Sync` because the cascade runs on
    /// a background tokio executor. The bridge creates this from the tool's
    /// `ToolCallEventStream` so progress appears as thinking traces in the
    /// agent UI — the user can see what the cascade is doing and cancel if
    /// it goes off track. When `None` (unit tests), no progress is emitted.
    progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
}

impl ManifestExecutor {
    /// Create a new executor with the given infrastructure ports.
    ///
    /// expect: "The system resolves and executes template manifest cascades"
    /// \[P3\] Motivating: Generative Space — executor for template manifest cascades
    /// pre:  inference and mcp are initialized
    /// post: returns ManifestExecutor with default template_base_path
    pub fn new(
        inference: Arc<dyn InferencePort>,
        tools: Arc<dyn ToolPort>,
        default_params: LLMParameters,
    ) -> Self {
        Self {
            inference,
            tools,
            default_params,
            template_renderer: TemplateRenderer::new(std::path::PathBuf::from(
                crate::template_renderer::DEFAULT_TEMPLATE_BASE_PATH,
            )),

            spotlighter: Spotlighter::new(SpotlightMode::Delimit),
            runtime_policy: None,
            taint_labels: Arc::new(std::sync::Mutex::new(HashMap::new())),
            terminal_check: None,
            progress: None,
        }
    }

    /// Wire a callback that checks whether the `terminal` built-in tool is
    /// enabled for the current agent profile. Used by the bridge to enforce
    /// proposer/evaluator separation (F6) — `terminal` is a built-in agent
    /// tool, not an MCP tool, so `discover_tools()` cannot detect it in
    /// production. The bridge wires this with
    /// `AgentProfileSettings::is_tool_enabled("terminal")`. When absent,
    /// profile enforcement falls back to `discover_tools()` (MCP tools only).
    #[must_use]
    pub fn with_terminal_check(mut self, check: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        self.terminal_check = Some(check);
        self
    }

    /// Wire a progress callback for real-time cascade feedback. The callback
    /// is invoked at the start of each cascade step with a human-readable
    /// description (e.g. "Step 2/5: scope (populate)"). The bridge creates
    /// this from the tool's `ToolCallEventStream` so the user sees thinking
    /// traces during skill execution and can steer or cancel. When absent
    /// (unit tests, pre-wiring), no progress is emitted.
    #[must_use]
    pub fn with_progress(mut self, progress: Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Set the template base path for resolving template_ref values.
    /// Useful for integration tests that need to point to a test fixture directory.
    #[must_use]
    pub fn with_template_base_path(mut self, path: std::path::PathBuf) -> Self {
        self.template_renderer = TemplateRenderer::new(path);
        self
    }

    /// Attach a runtime policy for pre-execution checks (Layer 6 defense).
    /// When set, every MCP tool invocation is checked before execution.
    ///
    /// expect: "The system checks every proposed tool invocation before execution"
    /// post: runtime_policy is set to Some(policy)
    #[must_use]
    pub fn with_runtime_policy(mut self, policy: Arc<hkask_regulation::DefaultPolicy>) -> Self {
        self.runtime_policy = Some(policy);
        self
    }

    /// Accessor: returns whether a runtime policy is wired.
    /// Used by the RR-0053 wiring test (kask_bridge) to verify `build_executor`
    /// attaches a `DefaultPolicy` — without it, the FIDES Source→Sink block
    /// (Layer 4) is dead code in production. Not `#[cfg(test)]`-gated because
    /// the test lives in a downstream crate (kask_bridge), which compiles this
    /// crate without `--cfg test`.
    pub fn runtime_policy_is_wired(&self) -> bool {
        self.runtime_policy.is_some()
    }

    /// Check whether a JSON value references any tainted (Source) context entries.
    ///
    /// This is the FIDES taint propagation check: recursively scans the value
    /// for `{"$ref": "step_N_result..."}` patterns and inline Jinja
    /// `{{ step_N_result }}` expressions, and checks whether any referenced
    /// context entry is labeled `Source` (untrusted).
    ///
    /// Source: Microsoft Research FIDES (arXiv:2505.23643)
    ///
    /// expect: "The system detects untrusted data flowing into tool inputs"
    /// pre:  value is the bound input JSON for a tool invocation
    /// post: returns true iff any $ref or {{ }} reference in the value resolves
    ///       to a Source-labeled entry
    fn check_untrusted_input(&self, value: &Value) -> bool {
        // `extract_referenced_keys` walks the entire value tree (Object $ref,
        // Array recursion, String inline-Jinja) and returns the set of
        // referenced context keys. This replaces the prior separate recursive
        // walk that duplicated `collect_referenced_keys`'s logic — one walk
        // instead of two.
        let keys = self.extract_referenced_keys(value);
        if keys.is_empty() {
            return false;
        }
        let labels = self.taint_labels.lock().unwrap_or_else(|e| e.into_inner());
        keys.iter()
            .any(|k| labels.get(k).copied().unwrap_or(ToolTaint::Pure) == ToolTaint::Source)
    }

    /// Propagate taint labels from referenced context entries to a newly bound key.
    ///
    /// When `input_mapping` resolves a value via `resolve_mapping_value`, the
    /// resolved value may originate from a Source-tainted context entry. This
    /// method inspects the *original* (pre-resolution) mapping value for
    /// references to tainted keys — both `$ref` patterns and inline Jinja
    /// `{{ step_N_result }}` expressions — and if any referenced key is tainted,
    /// labels the new key with the same taint.
    ///
    /// This closes the FIDES closure break (ART-3/IR-1) where inline-Jinja
    /// bindings used `context.insert` (not `insert_tainted`), losing the
    /// Source taint label and bypassing the Source→Sink block rule.
    ///
    /// expect: "The system propagates taint labels through input_mapping bindings"
    /// pre:  original_value is the pre-resolution mapping value; new_key is the
    ///       context key the resolved value will be inserted under
    /// post: if any referenced context key is tainted, new_key is labeled with
    ///       the strongest taint found (Source > Endorser > Pure)
    fn propagate_taint_for_binding(&self, original_value: &Value, new_key: &str) {
        let referenced_keys = self.extract_referenced_keys(original_value);
        if referenced_keys.is_empty() {
            return;
        }
        let mut labels = self.taint_labels.lock().unwrap_or_else(|e| e.into_inner());
        // Find the strongest taint among referenced keys.
        // Source > Endorser > Pure (Source is the only one that triggers the
        // Sink block rule, but propagating Endorser preserves the audit trail).
        let mut strongest = ToolTaint::Pure;
        for key in &referenced_keys {
            let taint = labels.get(key).copied().unwrap_or(ToolTaint::Pure);
            if taint == ToolTaint::Source {
                strongest = ToolTaint::Source;
                break; // Source is the strongest — no need to check further.
            }
            if taint == ToolTaint::Endorser && strongest == ToolTaint::Pure {
                strongest = ToolTaint::Endorser;
            }
        }
        if strongest != ToolTaint::Pure {
            labels.insert(new_key.to_string(), strongest);
        }
    }

    /// Extract context keys referenced in a mapping value, before resolution.
    ///
    /// Recognizes two reference patterns:
    /// - `$ref`: `{"$ref": "step_1_result.field"}` → extracts `step_1_result`
    /// - Inline Jinja: `"{{ step_1_result.field }}"` or `"{{ step_1_result }}"`
    ///   → extracts `step_1_result`
    ///
    /// Returns the set of referenced context keys (first segment before any dot).
    fn extract_referenced_keys(&self, value: &Value) -> Vec<String> {
        let mut keys = Vec::new();
        self.collect_referenced_keys(value, &mut keys);
        keys.sort();
        keys.dedup();
        keys
    }

    fn collect_referenced_keys(&self, value: &Value, keys: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(ref_path)) = map.get("$ref") {
                    if let Some(key) = ref_path.split('.').next()
                        && !key.is_empty()
                    {
                        keys.push(key.to_string());
                    }
                    return;
                }
                for v in map.values() {
                    self.collect_referenced_keys(v, keys);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    self.collect_referenced_keys(v, keys);
                }
            }
            Value::String(s) => {
                // Inline Jinja: extract identifiers that look like context keys.
                // Pattern: {{ identifier }} or {{ identifier.field }}
                // We look for `{{` ... `}}` spans and extract the first identifier.
                let mut remaining = s.as_str();
                while let Some(open) = remaining.find("{{") {
                    let after_open = &remaining[open + 2..];
                    let Some(close) = after_open.find("}}") else {
                        break;
                    };
                    let expr = after_open[..close].trim();
                    // Extract the first identifier-like token (starts with
                    // letter/underscore, followed by word chars). This avoids
                    // matching Jinja keywords like `if`, `for`, `endif`.
                    let token = expr
                        .split(|c: char| !c.is_alphanumeric() && c != '_')
                        .find(|t| {
                            !t.is_empty()
                                && (t.starts_with(|c: char| c.is_alphabetic())
                                    || t.starts_with('_'))
                                && !matches!(
                                    *t,
                                    "if" | "for" | "endif" | "endfor" | "else" | "elif"
                                )
                        });
                    if let Some(tok) = token {
                        // Treat as a context key if it is a known step-result
                        // prefix OR is present in the taint-labels map. The
                        // taint-labels lookup closes the blind spot where a
                        // Source-tainted value bound under a non-`step_`-prefixed
                        // name (e.g. `user_query`, `crafted_url`) lost its
                        // taint label and bypassed the Source→Sink block
                        // (FIDES L4, RR-0053 companion).
                        if tok.starts_with("step_") || tok == "task" || tok == "prev_step" {
                            keys.push(tok.to_string());
                        } else {
                            let labels =
                                self.taint_labels.lock().unwrap_or_else(|e| e.into_inner());
                            if labels.contains_key(tok) {
                                keys.push(tok.to_string());
                            }
                        }
                    }
                    remaining = &after_open[close + 2..];
                }
            }
            _ => {}
        }
    }

    async fn invoke_tool(
        &self,
        tool_name: &str,
        input: Value,
        action_number: u64,
        has_untrusted_input: bool,
    ) -> Result<(Value, ToolTaint)> {
        let tool_info = self.tools.get_tool_info(tool_name).await.ok_or_else(|| {
            TemplateError::NotFound(NotFound {
                entity_type: "tool".to_string(),
                id: tool_name.to_string(),
            })
        })?;

        if let Some(policy) = &self.runtime_policy {
            use hkask_regulation::PolicyVerdict;

            match policy.check(
                tool_name,
                tool_info.taint,
                has_untrusted_input,
                action_number,
            ) {
                PolicyVerdict::Block(reason) => {
                    tracing::warn!(
                        target: "reg.guard.runtime_policy",
                        tool = tool_name,
                        verdict = "block",
                        %reason,
                        "REG"
                    );
                    return Err(TemplateError::Manifest(format!(
                        "Runtime policy blocked tool '{tool_name}': {reason}"
                    )));
                }
                PolicyVerdict::RequireHuman(reason) => {
                    tracing::warn!(
                        target: "reg.guard.runtime_policy",
                        tool = tool_name,
                        verdict = "require_human",
                        %reason,
                        "REG"
                    );
                    return Err(TemplateError::Manifest(format!(
                        "Runtime policy requires human confirmation for '{tool_name}': {reason}"
                    )));
                }
                PolicyVerdict::Log(message) => {
                    info!(target: "reg.guard.runtime_policy", tool = tool_name, verdict = "log", %message, "REG");
                }
                PolicyVerdict::Allow => {}
            }
        }

        let executor_webid = WebID::from_persona(b"manifest-executor");
        let token = hkask_capability::DelegationToken::new(
            DelegationResource::Tool,
            tool_name.to_string(),
            DelegationAction::Execute,
            executor_webid,
            executor_webid,
        );

        let result = self
            .tools
            .invoke(&tool_info.server_id, tool_name, input, &token)
            .await
            .map_err(|error| match error {
                ToolPortError::CapabilityDenied(message) => {
                    TemplateError::CapabilityDenied(message)
                }
                other => TemplateError::Mcp(Box::new(other)),
            })?;
        Ok((
            spotlight_tool_output(&self.spotlighter, &result),
            tool_info.taint,
        ))
    }

    /// Execute the full manifest cascade with iterative PDCA convergence.
    ///
    /// Steps are sorted by ordinal and executed in sequence. The cascade loops
    /// when a `loop` action is encountered, re-entering from the target ordinal
    /// until the convergence threshold is met (via `abort`) or `max_iterations`
    /// is exhausted. If `convergence.max_iterations == 0`, executes once
    /// (single-pass for one-shot manifests).
    ///
    /// Returns the final context map with convergence metadata under `_convergence`.
    ///
    /// # Cancel Safety
    ///
    /// This function is *not* cancel-safe mid-cascade. Dropping the future
    /// between steps abandons the cascade state (gas used, iteration count,
    /// context map) — the registry is not mutated (skills/manifests are read
    /// before execution), but the caller's accumulated context is lost. The
    /// `taint_labels` mutex is released cleanly (no poisoning) on drop.
    /// Callers that need resume semantics should persist `initial_context`
    /// and re-invoke with the prior context map.
    pub async fn execute_manifest(
        &self,
        manifest: &BundleManifest,
        initial_context: HashMap<String, Value>,
    ) -> Result<HashMap<String, Value>> {
        let (context, _last_ordinal, _snapshot) =
            self.run_cascade(manifest, initial_context, 0).await?;
        Ok(context)
    }

    /// Drive the cascade with an explicit recursion `depth`.
    ///
    /// The public `execute_manifest` enters at depth 0; `execute_flowdef`
    /// re-enters with `depth + 1`. The matryoshka guard fires when `depth`
    /// exceeds `SYSTEM_MAX_RECURSION`, bounding *recursive nesting* (flowdef
    /// sub-cascades) — NOT iterative loop re-entry, which is bounded by
    /// `convergence.max_iterations`. Conflating the two (the prior bug)
    /// silently capped `max_iterations` at `SYSTEM_MAX_RECURSION`: a manifest
    /// declaring `max_iterations: 10` that failed to converge errored at
    /// iteration 8 with "Matryoshka depth limit exceeded" instead of exiting
    /// `MaxedOut` at iteration 10.
    async fn run_cascade(
        &self,
        manifest: &BundleManifest,
        initial_context: HashMap<String, Value>,
        depth: u8,
    ) -> Result<(HashMap<String, Value>, Option<u32>, BudgetSnapshot)> {
        if depth > hkask_capability::SYSTEM_MAX_RECURSION {
            return Err(TemplateError::Manifest(format!(
                "Matryoshka depth limit ({}) exceeded",
                hkask_capability::SYSTEM_MAX_RECURSION
            )));
        }
        let mut context = initial_context;
        // Steps are sorted by ordinal at load time (see `load_manifest_from_yaml`).
        // Borrow directly — no per-cascade clone+sort.
        let steps = &manifest.steps;

        // Unified convergence tracking (extracted to `convergence.rs`).
        // Replaces 5 `let` locals (max_iterations, threshold, field,
        // improvement_enabled, min_iterations, baseline_quality) with one tracker.
        let mut convergence = ConvergenceTracker::new(&manifest.convergence);
        let max_iterations = convergence.max_iterations();
        let threshold = convergence.threshold();
        let field = convergence.field().to_string();
        let improvement_enabled = convergence.improvement_enabled();
        let mut iteration: u32 = 0;
        // Unified gas + rJoule budget tracking (extracted to `budget.rs`).
        // Replaces 6 `let mut` locals (gas_used, gas_alerted, rjoule_used,
        // rjoule_alerted, plus the cap/threshold reads) with one tracker.
        let mut budget = BudgetTracker::new(&manifest.gas, &manifest.rjoule);

        // Initial convergence context (status: running, iteration 0).
        let snap = budget.snapshot();
        convergence.inject_running(
            &mut context,
            0,
            snap.gas_used,
            snap.gas_cap,
            snap.rjoule_used,
            snap.rjoule_cap,
        );

        let mut step_idx: usize = 0;
        // Track the highest ordinal of any step that stored a `step_N_result`
        // key during this cascade. Used by `execute_flowdef` to extract the
        // sub-cascade's final result in O(1) instead of scanning the entire
        // context HashMap (the `extract_final_step_result` fallback).
        let mut last_result_ordinal: Option<u32> = None;

        'cascade: loop {
            iteration += 1;
            // Update live convergence context for template awareness
            let snap = budget.snapshot();
            convergence.inject_running(
                &mut context,
                iteration,
                snap.gas_used,
                snap.gas_cap,
                snap.rjoule_used,
                snap.rjoule_cap,
            );

            while step_idx < steps.len() {
                let step = &steps[step_idx];

                info!(
                    target: "reg.skill.cascade.step_executed",
                    iteration = iteration,
                    step = step.ordinal,
                    action = %step.action,
                    description = %step.description,
                    "REG"
                );

                // Emit progress to the tool's event stream so the user can see
                // what the cascade is doing in real time and steer or cancel.
                // The callback is set by the bridge from `ToolCallEventStream`;
                // when absent (unit tests), this is a no-op.
                if let Some(ref progress) = self.progress {
                    let total = steps.len();
                    let desc = if step.description.is_empty() {
                        String::new()
                    } else {
                        format!(" — {}", step.description)
                    };
                    let action = &step.action;
                    if iteration > 1 {
                        progress(&format!(
                            "Iteration {iteration}, step {}/{total}: {action}{desc}",
                            step_idx + 1,
                        ));
                    } else {
                        progress(&format!("Step {}/{total}: {action}{desc}", step_idx + 1,));
                    }
                }

                // Evaluate step condition — skip if false.
                // Conditions may be Jinja expressions ({{ ... }}), which are rendered
                // against the context first, or truthy/comparison expressions evaluated
                // by evaluate_step_condition (supports ==, !=, <, <=, >, >=, AND/OR/NOT).
                if let Some(ref cond) = step.condition {
                    let resolved_cond = if cond.contains("{{") {
                        match self.template_renderer.render(cond, &context) {
                            Ok(rendered) => rendered.trim().to_string(),
                            Err(e) => {
                                info!(
                                    target: "reg.skill.cascade.step_executed",
                                    step = step.ordinal,
                                    error = %e,
                                    "condition render failed; treating as false"
                                );
                                String::from("false")
                            }
                        }
                    } else {
                        cond.clone()
                    };
                    if !evaluate_step_condition(&resolved_cond, &context) {
                        info!(
                            target: "reg.skill.cascade.step_executed",
                            iteration = iteration,
                            step = step.ordinal,
                            condition = %resolved_cond,
                            skipped = true,
                            "REG"
                        );
                        step_idx += 1;
                        continue;
                    }
                }

                // Profile enforcement (proposer/evaluator separation): if the
                // step declares a `profile`, verify `terminal` is NOT available.
                // This is the mechanical gate — a SKILL.md instruction is not a
                // gate. The check is effect-based (queries discover_tools), not
                // name-based, so it catches a user who customizes a built-in
                // profile to re-enable `terminal`. See .rules "Advertised
                // invariants need enforcement points".
                if let Some(ref profile_name) = step.profile {
                    let terminal_available = match &self.terminal_check {
                        Some(check) => check(),
                        None => {
                            // Fallback: discover_tools() returns MCP tools only.
                            // In production, `terminal` is a built-in agent tool
                            // and won't be found here — the bridge must wire
                            // `with_terminal_check` for production enforcement.
                            let available = self.tools.discover_tools().await;
                            available.iter().any(|t| t == "terminal")
                        }
                    };
                    if terminal_available {
                        return Err(TemplateError::Manifest(format!(
                            "Step {} declares profile '{}' but the `terminal` tool is available. \
                             This violates proposer/evaluator separation — a proposer with terminal \
                             can evaluate its own tests (self-confirming loop anti-pattern). \
                             Remediation: remove `terminal` from the '{}' profile in settings, \
                             or bind this step to a profile without `terminal` (e.g. `ask`).",
                            step.ordinal, profile_name, profile_name
                        )));
                    }
                }

                match step.action.as_str() {
                    // ── Abort: converged — exit with success ──
                    "abort" => {
                        info!(
                            target: "reg.skill.convergence.converged",
                            iteration = iteration,
                            reason = "abort action",
                            "REG"
                        );
                        let snap = budget.snapshot();
                        convergence.finalize_report(
                            &mut context,
                            ConvergenceStatus::Converged,
                            "quality_met",
                            iteration,
                            snap.gas_used,
                            snap.gas_cap,
                            snap.rjoule_used,
                            snap.rjoule_cap,
                        );
                        break 'cascade;
                    }

                    // ── Escalate: blocked — exit with error ──
                    "escalate" => {
                        let reason = step.description.clone();
                        info!(
                            target: "reg.skill.convergence.escalated",
                            iteration = iteration,
                            reason = %reason,
                            "REG"
                        );
                        let snap = budget.snapshot();
                        convergence.finalize_report(
                            &mut context,
                            ConvergenceStatus::Escalated,
                            "obstacle_blocked",
                            iteration,
                            snap.gas_used,
                            snap.gas_cap,
                            snap.rjoule_used,
                            snap.rjoule_cap,
                        );
                        return Err(TemplateError::Manifest(format!(
                            "Cascade escalated at step {}: {}",
                            step.ordinal, reason
                        )));
                    }

                    // ── Choice: evaluate condition, branch ──
                    "choice" => {
                        let target_ordinal = self.evaluate_choice(step, &context)?;
                        if let Some(target) = target_ordinal {
                            // Jump to target step
                            if let Some(pos) = steps.iter().position(|s| s.ordinal == target) {
                                step_idx = pos;
                                info!(
                                    target: "reg.skill.cascade.step_executed",
                                    iteration = iteration,
                                    choice_jump = target,
                                    "REG"
                                );
                                continue; // Re-enter loop at target step
                            }
                        }
                        // No jump — fall through to next step
                    }

                    // ── Loop: re-enter cascade from target ordinal ──
                    // Iterative loop re-entry is bounded by `convergence.max_iterations`
                    // (checked below); the matryoshka guard in `run_cascade` bounds
                    // recursive nesting (flowdef), not this.
                    "loop" => {
                        let loop_target = step
                            .input_mapping
                            .as_ref()
                            .and_then(|m| m.get("loop_target"))
                            .and_then(|v| v.as_str())
                            .and_then(|s| {
                                self.template_renderer
                                    .render(s, &context)
                                    .ok()
                                    .and_then(|rendered| rendered.trim().parse::<u32>().ok())
                            })
                            .unwrap_or(0);

                        info!(
                            target: "reg.skill.cascade.step_executed",
                            iteration = iteration,
                            loop_target = loop_target,
                            recursion_depth = depth,
                            "REG"
                        );

                        // Check convergence before looping
                        if iteration >= max_iterations {
                            let snap = budget.snapshot();
                            convergence.finalize_report(
                                &mut context,
                                ConvergenceStatus::MaxedOut,
                                "energy_spent",
                                iteration,
                                snap.gas_used,
                                snap.gas_cap,
                                snap.rjoule_used,
                                snap.rjoule_cap,
                            );
                            // Honor on_not_reached: if "escalate", emit span and
                            // return error instead of silently exiting.
                            if manifest.convergence.on_not_reached == "escalate" {
                                info!(
                                    target: "reg.skill.convergence.escalated",
                                    iteration = iteration,
                                    reason = "convergence not reached (max_iterations exhausted)",
                                    "REG"
                                );
                                return Err(TemplateError::Manifest(format!(
                                    "Cascade escalated: convergence not reached after {iteration} iterations (threshold {threshold}, field {field})"
                                )));
                            }
                            break 'cascade;
                        }

                        // Bind loop input_mapping (except loop_target) into context
                        // BEFORE recording the convergence cycle, so the Kata-model
                        // `convergence_signal` binding (e.g.
                        // `convergence_signal: "{{ step_14_result }}"`) is present
                        // in the context when `push_cycle_from_context` reads it.
                        // The prior ordering (push → bind) read a one-iteration-stale
                        // signal — the Cauchy check saw `[NaN, stale_1, stale_2, ...]`
                        // instead of `[fresh_1, fresh_2, ...]`. Carried state (e.g.
                        // prior_probability) is also available next iteration.
                        if let Some(ref mapping) = step.input_mapping
                            && let Value::Object(map) = mapping
                        {
                            for (k, v) in map {
                                if k == "loop_target" {
                                    continue;
                                }
                                let bound =
                                    resolve_mapping_value(v, &context, &self.template_renderer);
                                // Propagate taint from referenced Source entries
                                // to the new binding key (ART-3/IR-1 fix).
                                self.propagate_taint_for_binding(v, k);
                                context.insert(k.clone(), bound);
                            }
                        }

                        // Record this iteration's convergence data in the
                        // trajectory history BEFORE the convergence check, AFTER
                        // binding the loop's input_mapping so the Kata-model
                        // `convergence_signal` is the current iteration's reading.
                        // For the Kata model, the convergence signal and Brier
                        // score are read from the context (the signal is
                        // produced by a `compute` step — `kata.hypotenuse` for
                        // Kata-gap skills, or `lisp.eval`/any compute for
                        // custom-signal skills — and bound into context via the
                        // loop step's `convergence_signal:` mapping). For the
                        // legacy model, the self-grade metric is read from the
                        // convergence field. If the Kata fields aren't present,
                        // falls back to pushing NaN (a missing reading is not a
                        // converged reading).
                        convergence.push_cycle_from_context(&context);

                        // Check threshold convergence
                        if convergence.check_met(&context, iteration) {
                            let snap = budget.snapshot();
                            convergence.finalize_report(
                                &mut context,
                                ConvergenceStatus::Converged,
                                "quality_met",
                                iteration,
                                snap.gas_used,
                                snap.gas_cap,
                                snap.rjoule_used,
                                snap.rjoule_cap,
                            );
                            break 'cascade;
                        }

                        // Snapshot the prior iteration's step results under a
                        // `prev_step_N_result` namespace so refinement loops
                        // can reference them without manifest-level input_mapping
                        // gymnastics. This makes the Self-Refine pattern
                        // ("here is the previous artifact, identify its worst
                        // defect, refine it") expressible in templates:
                        // `{{ prev_step_1_result | tojson }}`.
                        //
                        // Without this, the loop re-enters step 1, which
                        // overwrites `step_1_result` — the prior artifact is
                        // lost, and the loop regenerates from scratch instead
                        // of refining. That made trajectory convergence
                        // impossible (each iteration produces a different
                        // artifact against a different goal, so the metric
                        // bounces instead of stabilizing).
                        // Acquire the taint-labels lock once for the entire
                        // snapshot loop — the prior code acquired it twice per
                        // step (read + write), which is 2N acquisitions for an
                        // N-step manifest where 1 suffices.
                        let mut labels =
                            self.taint_labels.lock().unwrap_or_else(|e| e.into_inner());
                        for step in steps.iter() {
                            let key = format!("step_{}_result", step.ordinal);
                            if let Some(val) = context.get(&key) {
                                let prev_key = format!("prev_{}", key);
                                // The snapshot copies the value, so it must
                                // also copy the taint label — otherwise a
                                // Source-tainted artifact silently loses its
                                // label when referenced as prev_step_N_result.
                                if let Some(label) = labels.get(&key).copied() {
                                    labels.insert(prev_key.clone(), label);
                                }
                                context.insert(prev_key, val.clone());
                            }
                        }
                        drop(labels);

                        // Re-enter: reset step index to loop target
                        if let Some(pos) = steps.iter().position(|s| s.ordinal == loop_target) {
                            step_idx = pos;
                            continue 'cascade; // Re-enter cascade from target — increments iteration
                        } else {
                            step_idx = 0; // Default: restart from beginning
                            continue 'cascade;
                        }
                    }

                    // ── Standard actions: select, populate, execute ──
                    "select" => {
                        context = self.execute_select(step, context, &mut budget).await?;
                        // Check budget exhaustion after select (unified gas + rJoule).
                        if let Some(_exhausted) = budget.check_exhausted(iteration) {
                            let snap = budget.snapshot();
                            convergence.finalize_report(
                                &mut context,
                                ConvergenceStatus::MaxedOut,
                                "energy_spent",
                                iteration,
                                snap.gas_used,
                                snap.gas_cap,
                                snap.rjoule_used,
                                snap.rjoule_cap,
                            );
                            break 'cascade;
                        }
                    }
                    "populate" => {
                        context = self.execute_populate(step, context).await?;
                    }
                    "compute" => {
                        context = self.execute_compute(step, context).await?;
                    }
                    "execute" | "feedback" | "validate" | "retrieve" => {
                        match self.execute_tool_invoke(step, &mut context).await {
                            Ok(()) => {}
                            Err(TemplateError::CapabilityDenied(msg)) => {
                                // Consult the manifest's declared capability-denied
                                // policy instead of blindly propagating the error.
                                // 10 manifests declare `on_capability_denied: escalate`.
                                let policy = manifest.error_handling.on_capability_denied.as_str();
                                match policy {
                                    "escalate" => {
                                        info!(
                                            target: "reg.skill.cascade.escalated",
                                            iteration = iteration,
                                            step = step.ordinal,
                                            reason = "capability denied — escalating per manifest policy",
                                            "REG"
                                        );
                                        let snap = budget.snapshot();
                                        convergence.finalize_report(
                                            &mut context,
                                            ConvergenceStatus::Escalated,
                                            "capability_denied",
                                            iteration,
                                            snap.gas_used,
                                            snap.gas_cap,
                                            snap.rjoule_used,
                                            snap.rjoule_cap,
                                        );
                                        return Err(TemplateError::Manifest(format!(
                                            "Cascade escalated at step {}: capability denied: {}",
                                            step.ordinal, msg
                                        )));
                                    }
                                    "abort" => {
                                        info!(
                                            target: "reg.skill.convergence.converged",
                                            iteration = iteration,
                                            step = step.ordinal,
                                            reason = "capability denied — aborting per manifest policy",
                                            "REG"
                                        );
                                        let snap = budget.snapshot();
                                        convergence.finalize_report(
                                            &mut context,
                                            ConvergenceStatus::Converged,
                                            "capability_denied_abort",
                                            iteration,
                                            snap.gas_used,
                                            snap.gas_cap,
                                            snap.rjoule_used,
                                            snap.rjoule_cap,
                                        );
                                        break 'cascade;
                                    }
                                    _ => {
                                        // Default (including empty): propagate the error.
                                        return Err(TemplateError::CapabilityDenied(msg));
                                    }
                                }
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    // RenderAct: render a template (`.j2` or `.yaml`) without
                    // inference. The action is the rendering — the output is
                    // produced by Jinja2, not by an LLM. Used for reference
                    // content, macro libraries, and structured reference docs.
                    "render" => {
                        context = self.execute_render(step, context).await?;
                    }
                    // FlowDef: recursively execute a `.yaml` sub-manifest as a
                    // nested cascade. The sub-manifest has its own convergence
                    // threshold, gas budget, and steps. The sub-cascade's gas
                    // budget is capped to the parent's remaining budget, and its
                    // consumption is deducted from the parent's counters — this
                    // closes the gas feedback loop so a sub-cascade can't bypass
                    // the parent's gas exhaustion check. Results are stored
                    // under `step_{ordinal}_result` without merging the full
                    // sub-context (which would risk overwriting parent keys).
                    // This is the composability/recursion primitive — skills
                    // compose into larger skills.
                    "flowdef" => {
                        let (new_context, gas_consumed, rjoule_consumed) = self
                            .execute_flowdef(
                                step,
                                context,
                                budget.remaining_gas(),
                                budget.remaining_rjoule(),
                                depth,
                            )
                            .await?;
                        context = new_context;
                        budget.consume_child(gas_consumed, rjoule_consumed);
                    }

                    other => {
                        return Err(TemplateError::Manifest(format!(
                            "Unknown manifest step action: '{}'",
                            other
                        )));
                    }
                }

                // Track the highest ordinal that stored a `step_N_result` key,
                // so `execute_flowdef` can extract the sub-cascade's final
                // result in O(1) instead of scanning the full context.
                //
                // Only update for actions that store under `step_{ordinal}_result`:
                // select, compute, execute/feedback/validate/retrieve, render,
                // flowdef. `populate` stores under `step_{ordinal}_populated` (not
                // `_result`), and `choice` may fall through without emitting any
                // key — both would corrupt the tracker if set unconditionally.
                // Control-flow actions (abort/escalate/loop) break or continue
                // before reaching here; `choice` falls through only when no
                // branch jumps, but it emits no result key, so it must be excluded.
                if matches!(
                    step.action.as_str(),
                    "select"
                        | "compute"
                        | "execute"
                        | "feedback"
                        | "validate"
                        | "retrieve"
                        | "render"
                        | "flowdef"
                ) {
                    last_result_ordinal = Some(step.ordinal);
                }

                // ── Unified skill feedback span emission (P9 §9.2) ──────────
                // After each select step, emit the corresponding SkillFeedbackSpan
                // under reg.skill.<manifest.id>.<phase>. The phase is derived from
                // the step's template_ref (e.g. "sankey-flow/sankey-classify" →
                // "classify"). Only select steps emit feedback spans — loop,
                // choice, abort, and escalate are control flow, not PDCA phases.
                if step.action == "select"
                    && let Some(ref template_ref) = step.template_ref
                    && let Some(phase) = extract_feedback_phase(template_ref)
                {
                    let span_target = format!("{}.{}", manifest.ledger.span_namespace, phase);
                    // tracing's target: needs &'static str, but we have a
                    // dynamic namespace. Use tracing::event! with the
                    // target as a field instead, and emit under the
                    // generic "reg.skill" target (which is registered).
                    // The full namespace is carried in the `ns` field.
                    info!(
                        target: "reg.skill",
                        ns = %span_target,
                        skill_id = %manifest.id,
                        phase = phase,
                        step = step.ordinal,
                        iteration = iteration,
                        template_ref = %template_ref,
                        "REG"
                    );
                }

                // ── Branching: route based on step result ───────────────────
                // If the step has a `branching` map, read the routing key from
                // the step result (field name from `branching_field`, default
                // "routing") and jump to the target ordinal. This closes the
                // feedback loop for select/execute steps — e.g., a proptest
                // fail routes back to the tracer, a bug-hunt gap routes back
                // to the plan. If the routing field is absent or does not
                // match any key, execution continues to the next ordinal.
                if let Some(ref branching) = step.branching {
                    let field_name = step.branching_field.as_deref().unwrap_or("routing");
                    let result_key = format!("step_{}_result", step.ordinal);
                    if let Some(routing) = context
                        .get(&result_key)
                        .and_then(|v| v.get(field_name))
                        .and_then(|v| v.as_str())
                        && let Some(&target_ordinal) = branching.get(routing)
                        && let Some(pos) = steps.iter().position(|s| s.ordinal == target_ordinal)
                    {
                        info!(
                            target: "reg.skill.cascade.step_executed",
                            iteration = iteration,
                            step = step.ordinal,
                            branch_key = %routing,
                            branch_target = target_ordinal,
                            "REG"
                        );
                        step_idx = pos;
                        continue;
                    } else if !context.contains_key(&result_key) {
                        // The step declared a `branching` map but its action did
                        // not emit a `step_{ordinal}_result` key (e.g. `populate`
                        // stores `step_{ordinal}_populated`). The branching map can
                        // never route — warn so the misconfiguration is not silent
                        // (the `.rules` "fails open with no diagnostic" trap).
                        // Actions that re-enter early (`loop`/`abort`) never reach
                        // this block; result-emitting actions (select/execute/
                        // compute/render/flowdef) write `_result` and are unaffected.
                        warn!(
                            target: "reg.skill.cascade.branching_misconfigured",
                            step = step.ordinal,
                            action = %step.action,
                            "Step {} (action '{}') declares a `branching` map but the action did \
                             not emit a `step_{{ordinal}}_result` key — the branching map will never \
                             route. Remediation: remove `branching` from this step, or use an action \
                             that emits a result (select/execute/compute/render/flowdef).",
                            step.ordinal,
                            step.action
                        );
                    }
                }

                step_idx += 1;
            }

            // while loop exited normally — reset step_idx for implicit loop re-entry
            step_idx = 0;

            // Check gas exhaustion at end of pass
            if let Some(_exhausted) = budget.check_exhausted(iteration) {
                let snap = budget.snapshot();
                convergence.finalize_report(
                    &mut context,
                    ConvergenceStatus::MaxedOut,
                    "energy_spent",
                    iteration,
                    snap.gas_used,
                    snap.gas_cap,
                    snap.rjoule_used,
                    snap.rjoule_cap,
                );
                break 'cascade;
            }

            // Compute compound quality from nested skill reports
            if manifest.convergence.aggregation != "none"
                && !manifest.convergence.aggregation_sources.is_empty()
            {
                let compound = convergence.compute_compound_quality(
                    &context,
                    &manifest.convergence.aggregation,
                    &manifest.convergence.aggregation_sources,
                );
                context.insert(field.clone(), serde_json::json!(compound));
            }

            // Capture baseline quality on first full pass. Done AFTER compound
            // quality computation so the baseline is in the same value space as
            // subsequent readings (compound if aggregation is enabled, raw
            // field value otherwise). Capturing before compound computation
            // would mix pre-compound and compound values in the improvement
            // ratio, producing nonsense.
            if improvement_enabled {
                convergence.capture_baseline(&context);
            }

            // Record this iteration's convergence data in the trajectory history
            // AFTER compound quality computation, so the history records the
            // same value check_met will read. For the Kata model, reads the
            // convergence signal and Brier score from the context (produced by
            // `compute` steps). For the legacy model, reads the self-grade
            // metric from the convergence field.
            convergence.push_cycle_from_context(&context);

            // ── End of pass: check convergence if no explicit loop/abort ──
            if iteration >= max_iterations {
                let snap = budget.snapshot();
                convergence.finalize_report(
                    &mut context,
                    ConvergenceStatus::MaxedOut,
                    "energy_spent",
                    iteration,
                    snap.gas_used,
                    snap.gas_cap,
                    snap.rjoule_used,
                    snap.rjoule_cap,
                );
                // Honor on_not_reached: if "escalate", emit span and return error
                // instead of silently exiting. This makes the convergence contract
                // real — skills that declare on_not_reached: escalate will actually
                // escalate when they fail to converge.
                if manifest.convergence.on_not_reached == "escalate" {
                    info!(
                        target: "reg.skill.convergence.escalated",
                        iteration = iteration,
                        reason = "convergence not reached (max_iterations exhausted)",
                        "REG"
                    );
                    return Err(TemplateError::Manifest(format!(
                        "Cascade escalated: convergence not reached after {iteration} iterations (threshold {threshold}, field {field})"
                    )));
                }
                break 'cascade;
            }

            if convergence.check_met(&context, iteration) {
                let snap = budget.snapshot();
                convergence.finalize_report(
                    &mut context,
                    ConvergenceStatus::Converged,
                    "quality_met",
                    iteration,
                    snap.gas_used,
                    snap.gas_cap,
                    snap.rjoule_used,
                    snap.rjoule_cap,
                );
                break 'cascade;
            }

            // Implicit loop: re-enter from step 0. Iterative re-entry is
            // bounded by `convergence.max_iterations` (checked above); the
            // matryoshka guard in `run_cascade` bounds recursive nesting
            // (flowdef), not this.
        }

        context.insert("_recursion_depth".to_string(), Value::Number(depth.into()));
        let final_snapshot = budget.snapshot();
        Ok((context, last_result_ordinal, final_snapshot))
    }

    /// Evaluate a `choice` step's condition against the context.
    /// Returns `Some(ordinal)` to jump to, or `None` to continue to next step.
    fn evaluate_choice(
        &self,
        step: &BundleManifestStep,
        context: &HashMap<String, Value>,
    ) -> Result<Option<u32>> {
        let mapping = match &step.input_mapping {
            Some(m) => m,
            None => {
                // A `choice` step with no `input_mapping` can never branch —
                // the `branches` array lives under `input_mapping.branches`.
                // Warn so the misconfiguration is not silent (the `.rules`
                // "fails open with no diagnostic" trap — the `branching`
                // misconfiguration at the call site has a warn; this is the
                // `choice` counterpart).
                warn!(
                    target: "reg.skill.cascade.choice_misconfigured",
                    step = step.ordinal,
                    "Step {} (action 'choice') has no `input_mapping` — the `branches` array lives under \
                     `input_mapping.branches`. The choice will never branch. Remediation: add an \
                     `input_mapping` with a `branches` array, or use `select` + `branching` (the \
                     production routing mechanism).",
                    step.ordinal
                );
                return Ok(None);
            }
        };

        // Branch on a JSON path comparison
        if let Some(branches) = mapping.get("branches").and_then(|b| b.as_array()) {
            for branch in branches {
                let condition = branch
                    .get("condition")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                let action = branch.get("action").and_then(|a| a.as_str()).unwrap_or("");

                let matched = match condition {
                    "default" | "else" => true,
                    _ => {
                        // Simple threshold check: "composite < 0.15"
                        if let Some((field, op, val_str)) = parse_choice_condition(condition) {
                            let current =
                                context.get(field).and_then(|v| v.as_f64()).unwrap_or(1.0);
                            let target: f64 = val_str.parse().unwrap_or(0.0);
                            match op {
                                "<" => current < target,
                                "<=" => current <= target,
                                ">" => current > target,
                                ">=" => current >= target,
                                "==" => (current - target).abs() < 0.001,
                                _ => false,
                            }
                        } else {
                            false
                        }
                    }
                };

                if matched {
                    return match action {
                        "continue" => Ok(None),
                        "abort" | "escalate" => {
                            // Handled by subsequent abort/escalate step; return None to continue.
                            // NOTE: this is an advertised contract with no enforcement —
                            // the manifest must follow with an explicit abort/escalate step,
                            // otherwise the cascade silently continues. There is no runtime
                            // check that such a step exists.
                            Ok(None)
                        }
                        _ => {
                            // Try to parse as ordinal number
                            action.parse::<u32>().ok().map(Some).ok_or_else(|| {
                                TemplateError::Manifest(format!(
                                    "Choice action '{}' is not a valid ordinal",
                                    action
                                ))
                            })
                        }
                    };
                }
            }
        } else {
            // `input_mapping` is present but has no `branches` key (or `branches`
            // is not an array). The choice will never branch — warn so the
            // misconfiguration is not silent (the `.rules` "fails open with no
            // diagnostic" trap).
            warn!(
                target: "reg.skill.cascade.choice_misconfigured",
                step = step.ordinal,
                "Step {} (action 'choice') has `input_mapping` but no `branches` array — the choice \
                 will never branch. Remediation: add a `branches` array under `input_mapping` \
                 (each branch has `condition` and `action`), or use `select` + `branching`.",
                step.ordinal
            );
        }

        Ok(None)
    }

    /// **Select** — Render a selector template, call inference, parse JSON result.
    ///
    /// The selector template (from `step.template_ref` or `step.renderer`) is
    /// rendered with the current context. The rendered prompt is sent to the
    /// inference port. The response is parsed as JSON and merged into context.
    async fn execute_select(
        &self,
        step: &BundleManifestStep,
        mut context: HashMap<String, Value>,
        budget: &mut BudgetTracker,
    ) -> Result<HashMap<String, Value>> {
        // Apply input_mapping: resolve {{ }} string values (and $ref objects) from the
        // context and promote them to top-level template variables. Without this, mapped
        // names referenced in .j2 templates (e.g. {{ tasks }}) would render empty.
        if let Some(ref mapping) = step.input_mapping
            && let Value::Object(map) = mapping
        {
            for (k, v) in map {
                let bound = resolve_mapping_value(v, &context, &self.template_renderer);
                // Propagate taint from referenced Source entries to the new
                // binding key (ART-3/IR-1 fix — closes FIDES closure break).
                self.propagate_taint_for_binding(v, k);
                context.insert(k.clone(), bound);
            }
        }

        let (prompt, raw_template_content) = self.render_step_template_with_raw(step, &context)?;

        let params = self.default_params.clone();

        // Resolve the output schema for this step. If a schema is available
        // (from step.output_schema or the template's contract.output frontmatter),
        // declare a synthetic tool and pass it via the `tools` parameter. The
        // model is forced to call the tool (emitting JSON conforming to the
        // schema) instead of emitting free-text prose. This is the LangGraph/Swarm
        // pattern: enforce the output contract at the inference API layer.
        //
        // This fixes the systemic "No JSON found in inference response" failure:
        // 79/364 templates don't instruct JSON output, and many more have fenced
        // examples that the parser confuses for the actual result. Tool-calling
        // eliminates the parsing heuristic entirely — the API guarantees JSON.
        let output_schema = resolve_output_schema(step, &raw_template_content);
        let structured_tool = output_schema
            .as_ref()
            .map(|schema| build_structured_output_tool(schema.clone()));
        let tools: Option<&[ChatToolDefinition]> =
            structured_tool.as_ref().map(std::slice::from_ref);

        let (result_text, tool_calls, cost_usd): (
            String,
            Vec<hkask_types::StructuredToolCall>,
            Option<f64>,
        ) = {
            let timeout_dur = std::time::Duration::from_secs(step.timeout_seconds as u64);
            let result: InferenceResult = match tokio::time::timeout(
                timeout_dur,
                self.inference.generate(&prompt, &params, tools),
            )
            .await
            {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => return Err(TemplateError::Inference(e)),
                Err(_elapsed) => {
                    return Err(TemplateError::Manifest(format!(
                        "Step {} timed out after {}s",
                        step.ordinal, step.timeout_seconds
                    )));
                }
            };
            let cost_usd = result.cost_usd;
            (result.text, result.tool_calls, cost_usd)
        };

        // rJoule (USD) tracking — charge the inference call's USD cost. The
        // InferencePort populates `cost_usd` from the provider's observed response
        // (`usage.cost` / `market_cost` / `estimated_cost`), not an operator-configured
        // price table (1 rJoule = $1 USD). `None` when the provider reports no cost
        // (local Ollama, the zed IPC bridge path which doesn't surface cost, $0) —
        // free, not charged. Charged AFTER the call (cost is response-driven, only
        // known post-call); the `check_exhausted` below trips the rJoule hard limit once
        // cumulative spend exceeds `rjoule.cap` (a USD budget). Only LLM inference
        // (`select` steps) is charged here — MCP `execute` steps that hit paid
        // external APIs are NOT yet charged rJoule through the executor (TODO:
        // per-MCP-server gates like `hkask-mcp-media`'s `MediaBudget`, or a
        // `cost_usd` field on the MCP tool response envelope).
        if let Some(cost) = cost_usd {
            budget.charge_rjoule(cost);
        }

        // Gas tracking — deduct one iteration of compute
        budget.charge_iteration();

        // Extract the parsed result. If the model called the structured-output
        // tool, use the tool call arguments directly (the API guaranteed JSON
        // conforming to the schema). Otherwise, fall back to parsing the text
        // response (for models that don't support tool calling, or when no
        // schema was available).
        let parsed: Value = if let Some(tool_call) = tool_calls.first() {
            info!(
                target: "reg.skill.cascade.step_executed",
                step = step.ordinal,
                structured_output = true,
                "Model emitted structured tool call — extracting args"
            );
            tool_call.args.clone()
        } else {
            if output_schema.is_some() {
                warn!(
                    target: "reg.skill.cascade.step_executed",
                    step = step.ordinal,
                    "Model did not call structured-output tool — falling back to text parsing"
                );
            }
            parse_json_response(&result_text, step.ordinal)?
        };
        context.insert(format!("step_{}_result", step.ordinal), parsed);

        // Inject dual-budget context for template awareness (unified via BudgetTracker).
        budget.inject_into_context(&mut context);

        Ok(context)
    }

    /// **Populate** — Render a template with the accumulated context.
    ///
    /// If the step has `input_mapping` (bindings), those are resolved against
    /// the context and merged in before template rendering. This allows selector
    /// output fields like `step_1_result.memory_type` to be promoted to top-level
    /// template variables via `{"$ref": "step_1_result.memory_type"}` bindings.
    ///
    /// The template is rendered with the current context map. The rendered
    /// output is stored in context under `step_{ordinal}_populated`.
    async fn execute_populate(
        &self,
        step: &BundleManifestStep,
        mut context: HashMap<String, Value>,
    ) -> Result<HashMap<String, Value>> {
        // Resolve bindings from input_mapping and merge into context. Uses
        // `resolve_mapping_value` (not the legacy `bind_parameters`) so inline
        // `{{ expr }}` Jinja binding values render — matching every other step
        // action. `bind_parameters` passed `{{ }}` strings through as literals,
        // silently leaving populate templates with unresolved `{{ }}` source
        // (12 manifests in the registry hit this: root-cause-analysis,
        // voice-models, prompt-injection-diagnostic, rag-pipeline, ...).
        if let Some(ref mapping) = step.input_mapping
            && let Value::Object(map) = mapping
        {
            for (k, v) in map {
                let bound = resolve_mapping_value(v, &context, &self.template_renderer);
                // Propagate taint from referenced Source entries to the new
                // binding key (RR-0027 — same FIDES closure break as RR-0026).
                // Pass the *original* mapping value (with $ref / {{ }} markers),
                // not the resolved value — propagate_taint_for_binding inspects
                // pre-resolution markers to find referenced keys.
                self.propagate_taint_for_binding(v, k);
                context.insert(k.clone(), bound);
            }
        }

        let populated = self.render_step_template(step, &context)?;
        // Rendered output interpolates context values into a string — if any
        // binding fed by this step's input_mapping was Source-tainted, the
        // rendered artifact is derived from Source data and must carry the
        // label, or a downstream $ref to it bypasses check_untrusted_input
        // (which gates on labels, not content).
        if let Some(ref mapping) = step.input_mapping {
            self.propagate_taint_for_binding(mapping, &format!("step_{}_populated", step.ordinal));
        }
        context.insert(
            format!("step_{}_populated", step.ordinal),
            Value::String(populated),
        );

        Ok(context)
    }

    /// **Render** (RenderAct) — Render a template without inference.
    ///
    /// The action is the rendering. The template (`.j2` or `.yaml`) is
    /// rendered with minijinja and the output is stored in context. No
    /// LLM call is made — this is for reference content, macro libraries,
    /// and structured reference docs that are never sent to the LLM.
    ///
    /// Per the hKask Pattern A type system, RenderAct is the non-inference
    /// layer: content that is included into other templates via
    /// `{% include %}`/`{% from %}` or consumed as structured reference.
    async fn execute_render(
        &self,
        step: &BundleManifestStep,
        mut context: HashMap<String, Value>,
    ) -> Result<HashMap<String, Value>> {
        // Resolve bindings from input_mapping and merge into context.
        if let Some(ref mapping) = step.input_mapping
            && let Value::Object(map) = mapping
        {
            for (k, v) in map {
                let bound = resolve_mapping_value(v, &context, &self.template_renderer);
                // Propagate taint from referenced Source entries to the new
                // binding key (ART-3/IR-1 fix — closes FIDES closure break).
                self.propagate_taint_for_binding(v, k);
                context.insert(k.clone(), bound);
            }
        }

        let rendered = self.render_step_template(step, &context)?;
        // Rendered output interpolates context values into a string — if any
        // binding fed by this step's input_mapping was Source-tainted, the
        // rendered artifact is derived from Source data and must carry the
        // label, or a downstream $ref to it bypasses check_untrusted_input
        // (which gates on labels, not content).
        if let Some(ref mapping) = step.input_mapping {
            self.propagate_taint_for_binding(mapping, &format!("step_{}_result", step.ordinal));
        }
        context.insert(
            format!("step_{}_result", step.ordinal),
            Value::String(rendered),
        );

        Ok(context)
    }

    /// **FlowDef** — Recursively execute a `.yaml` sub-manifest as a nested
    /// cascade.
    ///
    /// The sub-manifest has its own convergence threshold, gas budget, and
    /// steps. It is loaded from the filesystem (or embedded registry fallback),
    /// parsed as a `BundleManifest`, and executed via `execute_manifest()`.
    ///
    /// **Gas budget closure:** The sub-cascade's gas/rjoule caps are capped to
    /// the parent's remaining budget (`parent_gas_cap` / `parent_rjoule_cap`).
    /// The sub-cascade's actual consumption is returned to the parent, which
    /// deducts it from its own counters. This closes the gas feedback loop —
    /// a sub-cascade cannot bypass the parent's gas exhaustion check.
    ///
    /// **Context isolation:** Only the sub-cascade's final result value is
    /// stored under `step_{ordinal}_result` in the parent context. The full
    /// sub-context is NOT merged back — this prevents the sub-cascade from
    /// overwriting parent context keys.
    ///
    /// This is the composability/recursion primitive — skills compose into
    /// larger skills. A step with `action: flowdef` and
    /// `template_ref: media/logo-discovery` loads `media/logo-discovery.yaml`,
    /// runs its PDCA cascade, and returns the result.
    ///
    /// Returns `(context, gas_consumed, rjoule_consumed)` so the caller can
    /// deduct consumption from the parent's counters.
    async fn execute_flowdef(
        &self,
        step: &BundleManifestStep,
        mut context: HashMap<String, Value>,
        parent_gas_remaining: u64,
        parent_rjoule_remaining: f64,
        depth: u8,
    ) -> Result<(HashMap<String, Value>, u64, f64)> {
        let template_ref = step.template_ref.as_deref().ok_or_else(|| {
            TemplateError::Manifest(format!(
                "Step {} has action='flowdef' but no template_ref",
                step.ordinal
            ))
        })?;

        // Resolve {{key}} references from context before loading.
        let template_ref = TemplateRenderer::render_inline(template_ref, &context);

        // Load the sub-manifest YAML. Filesystem first (so YAML edits take
        // effect without recompilation), then embedded .j2/.yaml as fallback.
        let manifest_yaml = if let Ok(content) = self
            .template_renderer
            .load_from_disk(&template_ref, step.ordinal)
        {
            content
        } else if let Some(content) = crate::template_yaml_file(&template_ref) {
            content.to_string()
        } else if let Some(content) = crate::template_file(&template_ref) {
            content.to_string()
        } else {
            return Err(TemplateError::NotFound(NotFound {
                entity_type: "flowdef sub-manifest".to_string(),
                id: format!(
                    "Step {}: sub-manifest '{}' not found on filesystem or in embedded registry",
                    step.ordinal, template_ref
                ),
            }));
        };

        // Parse the sub-manifest.
        let mut sub_manifest = load_manifest_from_yaml(&manifest_yaml).map_err(|e| {
            TemplateError::Manifest(format!(
                "Step {}: failed to parse sub-manifest '{}': {}",
                step.ordinal, template_ref, e
            ))
        })?;

        // Cap the sub-cascade's gas/rjoule to the parent's remaining budget.
        // This closes the gas feedback loop: the sub-cascade cannot consume
        // more than the parent has left. If the sub-manifest declares a
        // smaller budget, that smaller value is used (min of declared and
        // remaining).
        let sub_gas_cap = (sub_manifest.gas.cap as u64).min(parent_gas_remaining);
        let sub_rjoule_cap_f64 =
            (sub_manifest.rjoule.cap as f64).min(parent_rjoule_remaining.max(0.0));
        // Guard against NaN: if parent_rjoule_remaining is NaN, the min is NaN,
        // and `NaN as u32` silently becomes 0. Clamp to 0.0 if not finite.
        let sub_rjoule_cap = if sub_rjoule_cap_f64.is_finite() {
            sub_rjoule_cap_f64
        } else {
            tracing::warn!(
                target: "hkask.templates",
                parent_rjoule_remaining = ?parent_rjoule_remaining,
                "sub_rjoule_cap is not finite (NaN/Inf) — clamping to 0."
            );
            0.0
        };
        sub_manifest.gas.cap = sub_gas_cap as u32;
        sub_manifest.rjoule.cap = sub_rjoule_cap as u32;

        // Resolve bindings from input_mapping and merge into context for
        // the sub-cascade.
        if let Some(ref mapping) = step.input_mapping
            && let Value::Object(map) = mapping
        {
            for (k, v) in map {
                let bound = resolve_mapping_value(v, &context, &self.template_renderer);
                // Propagate taint from referenced Source entries to the new
                // binding key (ART-3/IR-1 fix — closes FIDES closure break).
                self.propagate_taint_for_binding(v, k);
                context.insert(k.clone(), bound);
            }
        }

        // Snapshot the parent's context keys before the sub-cascade so we can
        // detect what the sub-cascade added (for gas accounting, not context
        // merge — we don't merge the full sub-context back).
        let parent_keys: std::collections::HashSet<String> = context.keys().cloned().collect();

        // Execute the sub-cascade. Box::pin is required because this is a
        // recursive async fn — without it, the future would be infinitely
        // sized. Re-enter `run_cascade` with `depth + 1` so the matryoshka
        // guard in `run_cascade` bounds recursive nesting (this is the ONLY
        // path that increments depth; iterative loop re-entry does not).
        // `run_cascade` returns the last-completed step ordinal and the
        // sub-cascade's final budget snapshot so we can extract the final
        // result in O(1) and report actual gas/rjoule usage (not the capped
        // cap) to the parent.
        let (sub_result, last_ordinal, sub_budget_snapshot) =
            Box::pin(self.run_cascade(&sub_manifest, context, depth + 1)).await?;

        // Extract the sub-cascade's final result value. We do NOT merge the
        // full sub-context back into the parent — only the result is stored,
        // preventing the sub-cascade from overwriting parent context keys.
        //
        // The final result is the highest-ordinal `step_N_result` key.
        // `run_cascade` tracks the last-completed ordinal, so we can read it
        // directly in O(1). The `extract_final_step_result` fallback (full
        // context scan) is used when the ordinal is unavailable OR when the
        // tracked key is absent from the context (defense-in-depth — the
        // producer only tracks result-emitting actions, but a future action
        // type or an edge case could still produce a stale ordinal).
        let final_step_key = last_ordinal.map(|n| format!("step_{n}_result"));
        let result_value = match &final_step_key {
            Some(key) => sub_result
                .get(key)
                .cloned()
                .unwrap_or_else(|| extract_final_step_result(&sub_result)),
            None => extract_final_step_result(&sub_result),
        };

        // Reconstruct the parent context from the sub-result. The sub-cascade
        // received the parent's context, so the sub-result contains the
        // parent's keys plus the sub-cascade's additions. We keep only the
        // parent's original keys (preserving any updates the sub-cascade made
        // to those keys) plus the step result.
        let mut parent_context = HashMap::new();
        for (k, v) in &sub_result {
            if parent_keys.contains(k) {
                parent_context.insert(k.clone(), v.clone());
            }
        }
        // Taint labels live in the Arc-shared map, so labels the sub-cascade
        // set on parent keys persist — no copy needed for those. The new
        // step_{ordinal}_result key, however, is inserted below without a
        // label; copy the label of the sub-cascade's final step result (the
        // same ordinal key we extracted above) so a Source-tainted sub-result
        // doesn't enter the parent context unlabeled. Acquire the lock once
        // for the read + write (the prior code acquired it twice in
        // succession — a TOCTOU window and 2× the lock cost).
        if let Some(ref final_key) = final_step_key {
            let mut labels = self.taint_labels.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(label) = labels.get(final_key).copied() {
                labels.insert(format!("step_{}_result", step.ordinal), label);
            }
        }
        parent_context.insert(format!("step_{}_result", step.ordinal), result_value);

        // Compute gas/rjoule consumed by the sub-cascade. `run_cascade` returns
        // the sub-cascade's final `BudgetSnapshot`, so we report the ACTUAL
        // usage (`gas_used` / `rjoule_used`), not the capped cap. The prior
        // implementation reported `sub_gas_cap` / `sub_rjoule_cap` (the capped
        // caps) as consumed — conservative but distorted the parent's gas
        // feedback loop: a sub-cascade that converged in 1 iteration using
        // 100 gas of a 5000 cap still deducted 5000 from the parent, which
        // could prematurely exhaust the parent's budget. The actual usage is
        // bounded by the capped cap (the sub-cascade's `BudgetTracker` was
        // constructed with the capped cap), so this never under-counts.
        let gas_consumed = sub_budget_snapshot.gas_used;
        let rjoule_consumed = sub_budget_snapshot.rjoule_used;

        Ok((parent_context, gas_consumed, rjoule_consumed))
    }

    /// **Execute** — Invoke an MCP tool with parameters bound from context.
    ///
    /// The MCP server/tool is specified in `step.mcp` (format: "server/tool").
    /// Parameters are bound from `step.input_mapping` or the current context.
    async fn execute_tool_invoke(
        &self,
        step: &BundleManifestStep,
        context: &mut HashMap<String, Value>,
    ) -> Result<()> {
        let mcp_ref_raw = step.mcp.as_deref().ok_or_else(|| {
            TemplateError::Manifest(format!(
                "Execute step {} has no mcp reference",
                step.ordinal
            ))
        })?;

        // Resolve ${variable} references in the MCP reference against context
        let mcp_ref = TemplateRenderer::render_inline(mcp_ref_raw, context);

        // Check the pre-resolution mapping for Source-tainted $ref references
        // BEFORE resolve_mapping_value strips them. check_untrusted_input scans
        // for {"$ref": "…"} patterns; running it on the resolved input (where
        // $ref is gone) always returns false — the FIDES gate was theater on
        // this path. Pass the result into invoke_tool so the policy gate sees
        // the real taint state of the referenced context entries.
        let has_untrusted_input = step
            .input_mapping
            .as_ref()
            .is_some_and(|mapping| self.check_untrusted_input(mapping));

        // Use resolve_mapping_value (not bind_parameters) so inline `{{ expr }}`
        // Jinja strings in tool inputs are rendered — matching every other step
        // type. bind_parameters only handled $ref and literals, passing `{{ }}`
        // strings through verbatim.
        let input: Value = step
            .input_mapping
            .as_ref()
            .map(|mapping| resolve_mapping_value(mapping, context, &self.template_renderer))
            .unwrap_or_else(|| {
                Value::Object(
                    context
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                )
            });

        let (result, tool_taint) = self
            .invoke_tool(&mcp_ref, input, context.len() as u64, has_untrusted_input)
            .await?;

        let result_key = format!("step_{}_result", step.ordinal);

        // FIDES taint propagation: if this tool is a Source (returns untrusted
        // data from external sources), mark the result as tainted so downstream
        // Sink tools can detect it via check_untrusted_input.
        // Layer 5 defense (Microsoft Research FIDES arXiv:2505.23643).
        if tool_taint == ToolTaint::Source {
            self.taint_labels
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(result_key.clone(), ToolTaint::Source);
        }

        context.insert(result_key, result);

        Ok(())
    }

    /// Execute a `compute` step — invoke a canonical `hkask_forecast` primitive
    /// deterministically, without an LLM round-trip. The step's `compute_ref`
    /// names the function; `input_mapping` binds its arguments from prior step
    /// results. The return value is stored as `step_{ordinal}_result`.
    ///
    /// This is the connection between the skill pipeline and the deterministic
    /// math layer: stages 1 (Fermi), 2 (outside view), 4 (Bayesian), and
    /// calibration feedback become `compute` steps instead of LLM `select` steps.
    async fn execute_compute(
        &self,
        step: &BundleManifestStep,
        mut context: HashMap<String, Value>,
    ) -> Result<HashMap<String, Value>> {
        let compute_ref = step.compute_ref.as_deref().ok_or_else(|| {
            TemplateError::Manifest(format!("Compute step {} has no compute_ref", step.ordinal))
        })?;

        let input: Value = step
            .input_mapping
            .as_ref()
            .map(|mapping| {
                // Use resolve_mapping_value (not bind_parameters) so `{{ }}`
                // Jinja expressions with defaults render correctly — the same
                // convention every other step action (select/populate/loop/
                // render/flowdef) uses. bind_parameters passed `{{ }}` strings
                // through as literals, silently breaking compute steps that
                // bound context values via Jinja (e.g. lisp.eval's
                // histories degraded to empty defaults; swarm.converge_accumulate
                // hard-errored on get_f64). $ref objects and literal values pass
                // through unchanged, so existing compute bindings are unaffected.
                if let Value::Object(map) = mapping {
                    let mut out = serde_json::Map::new();
                    for (k, v) in map {
                        let bound = resolve_mapping_value(v, &context, &self.template_renderer);
                        // Propagate taint from referenced Source entries to the
                        // bound key (the .rules "input_mapping bindings must
                        // propagate taint" trap — RR-0026/RR-0027 fixed this at
                        // every other resolve_mapping_value call site; compute was
                        // the remaining gap, silently absent because it used
                        // bind_parameters which never rendered bindings).
                        self.propagate_taint_for_binding(v, k);
                        out.insert(k.clone(), bound);
                    }
                    Value::Object(out)
                } else {
                    mapping.clone()
                }
            })
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

        let result = dispatch_compute(compute_ref, &input)?;
        info!(
            target: "reg.skill.cascade.compute",
            ordinal = step.ordinal,
            compute_ref = compute_ref,
            "REG"
        );
        // The compute output is derived from the bound inputs — if any input
        // was bound from a Source-tainted context entry, the output carries
        // Source data and must inherit the label, or a downstream Sink tool fed
        // this `step_N_result` bypasses the FIDES Source→Sink gate (the gate
        // reads labels, not content). Same closure-break class as populate/render,
        // which already label their outputs — compute was the remaining gap.
        if let Some(ref mapping) = step.input_mapping {
            self.propagate_taint_for_binding(mapping, &format!("step_{}_result", step.ordinal));
        }
        context.insert(format!("step_{}_result", step.ordinal), result);

        Ok(context)
    }
}

/// Render a template step according to its renderer mode.
///
/// Dispatches based on `step.renderer`:
/// - `"minijinja"` — Load template from `step.template_ref` (a file path
///   like `curator/system_state_gather.j2`) relative to the renderer's base path,
///   then render with full Jinja2 syntax via minijinja.
/// - Inline/absent — Render `step.template_ref` or `step.renderer` as a
///   simple template string with `{{key}}` substitution.
impl ManifestExecutor {
    fn render_step_template(
        &self,
        step: &BundleManifestStep,
        context: &HashMap<String, Value>,
    ) -> Result<String> {
        let (prompt, _raw_content) = self.render_step_template_with_raw(step, context)?;
        Ok(prompt)
    }

    /// Render a step's template and return both the rendered prompt and the
    /// raw template content (before rendering). The raw content is used to
    /// extract the `contract.output` frontmatter for structured-output tool
    /// calling — the schema lives in the frontmatter, not the rendered prompt.
    fn render_step_template_with_raw(
        &self,
        step: &BundleManifestStep,
        context: &HashMap<String, Value>,
    ) -> Result<(String, String)> {
        let renderer = step.renderer.as_deref().unwrap_or("");

        match renderer {
            "minijinja" => {
                // template_ref is a file path relative to the renderer's base path.
                // Resolve {{key}} references from context before loading.
                let template_ref_raw = step.template_ref.as_deref().ok_or_else(|| {
                    TemplateError::Manifest(format!(
                        "Step {} has renderer='minijinja' but no template_ref",
                        step.ordinal
                    ))
                })?;
                let template_ref = TemplateRenderer::render_inline(template_ref_raw, context);

                // Delegate resolution + loading to the renderer. The renderer
                // owns the filesystem→embedded ladder and the .j2/.yaml fallbacks.
                let template_content = self.template_renderer.load(&template_ref, step.ordinal)?;

                info!(
                    target: "reg.spec.executor",
                    step = step.ordinal,
                    template = %template_ref,
                    "Rendering minijinja template"
                );

                let prompt = self.template_renderer.render(&template_content, context)?;
                Ok((prompt, template_content))
            }
            _ => {
                // Inline mode: template_ref or renderer contains the template string
                let template_content = step
                    .template_ref
                    .as_deref()
                    .or(step.renderer.as_deref())
                    .ok_or_else(|| {
                        TemplateError::Manifest(format!(
                            "Step {} has no template_ref or renderer",
                            step.ordinal
                        ))
                    })?;

                let rendered = TemplateRenderer::render_inline(template_content, context);
                Ok((rendered, template_content.to_string()))
            }
        }
    }
}

/// Deterministically extract the final step's result from a cascade context.
///
/// `execute_manifest` stores each step's output under a `step_{ordinal}_result`
/// key. HashMap iteration order is randomized, so `values().last()` would pick
/// an arbitrary step. This function parses the ordinal from each `step_N_result`
/// key and returns the value of the highest ordinal as a `Value`. Falls back to
/// `Value::Null` if no `step_N_result` keys are present.
///
/// Used by `execute_flowdef` to extract the sub-cascade's final result without
/// merging the full sub-context back into the parent.
///
/// Applies `normalize_model_output` to strip `<thinking>` reasoning wrappers
/// that reasoning models emit before the final answer — without this, the
/// tags pollute downstream step inputs and break JSON parsing (Wang 2026,
/// arXiv:2603.02615v1, Appendix A.4).
pub fn extract_final_step_result(context: &HashMap<String, Value>) -> Value {
    extract_final_step_entry(context)
        .map(|(_, value)| normalize_model_output(&value).into_owned())
        .unwrap_or(Value::Null)
}

/// The ordinal-keyed selector behind `extract_final_step_result`, returning
/// the key as well so callers that need to copy the key's taint label don't
/// re-implement the ordinal parse (the `.rules` trap this guards against
/// exists because this logic was once re-implemented at multiple sites).
fn extract_final_step_entry(context: &HashMap<String, Value>) -> Option<(String, Value)> {
    context
        .iter()
        .filter_map(|(key, value)| {
            key.strip_prefix("step_")
                .and_then(|rest| rest.strip_suffix("_result"))
                .and_then(|n| n.parse::<u32>().ok())
                .map(|ordinal| (ordinal, key, value))
        })
        .max_by_key(|(ordinal, _, _)| *ordinal)
        .map(|(_, key, value)| (key.clone(), value.clone()))
}

/// Strip model-emitted reasoning wrappers from a step result value.
///
/// Reasoning models (e.g. Kimi K2, DeepSeek-R1) emit `<thinking>...</thinking>`
/// blocks before the final answer. Without stripping, these tags pollute
/// downstream step inputs and break JSON parsing. This is the failure mode
/// documented in Wang (2026, arXiv:2603.02615v1, Appendix A.4): the RLM
/// framework's `find_code_blocks` / `find_final_answer` parsers missed
/// answers entirely until a `strip_think_tags` helper was added.
///
/// Also strips a stray closing `</thinking>` token when the opening tag was
/// truncated by a streaming chunk boundary. Non-string values pass through
/// unchanged (the wrapper only appears in model-generated text).
///
/// Returns `Cow::Borrowed` when no stripping is needed (the common path),
/// `Cow::Owned` when tags were removed — avoiding a clone on clean output.
fn normalize_model_output(value: &Value) -> Cow<'_, Value> {
    let Value::String(s) = value else {
        return Cow::Borrowed(value);
    };
    if !s.contains("<thinking") && !s.contains("</thinking>") {
        return Cow::Borrowed(value);
    }
    let mut cleaned = s.to_string();
    // Strip paired `<thinking>...</thinking>` blocks iteratively. A model may
    // emit several; each pass removes the first complete pair. An unclosed
    // opening tag (truncated stream) breaks out of the loop and falls through
    // to the stray-tag strip below.
    while let Some(start) = cleaned.find("<thinking") {
        let after_open = match cleaned[start..].find('>') {
            Some(i) => start + i + 1,
            None => break,
        };
        let end = match cleaned[after_open..].find("</thinking>") {
            Some(i) => after_open + i,
            None => break,
        };
        cleaned = format!("{}{}", &cleaned[..start], &cleaned[end + 11..]);
    }
    // Strip stray closing tags (opening was truncated by streaming).
    cleaned = cleaned.replace("</thinking>", "");
    Cow::Owned(Value::String(cleaned))
}

/// Parse a JSON response from an inference call.
///
/// Attempts to extract JSON from the response text, handling cases where
/// the model wraps the JSON in markdown code fences.
fn parse_json_response(text: &str, step_ordinal: u32) -> Result<Value> {
    if let Ok(v) = serde_json::from_str(text) {
        return Ok(v);
    }
    // Brace-balanced extraction (RR-0028): the old first-brace to last-brace
    // slice approach silently merged an injected JSON block in the model's
    // reasoning preamble with its real answer. `extract_json_from_response`
    // returns exactly one top-level object, defeating the injection.
    let extracted = llm_json::extract_json_from_response(text);
    serde_json::from_str(&extracted).map_err(|e| {
        TemplateError::Manifest(format!(
            "Step {}: Failed to parse JSON response: {}",
            step_ordinal, e
        ))
    })
}

/// Apply spotlighting to a tool output value before it enters the LLM context.
///
/// Serializes the JSON value to a string, applies the spotlighting transform,
/// and wraps the result back as a JSON string value. This ensures the LLM sees
/// the untrusted content marked as data, not instructions.
///
/// Source: Microsoft Research arXiv:2403.14720
fn spotlight_tool_output(spotlighter: &Spotlighter, result: &Value) -> Value {
    let text = match result {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    Value::String(spotlighter.spotlight(&text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_capability::{DelegationToken, ToolFuture, ToolInfo};
    use hkask_types::InferenceError;
    use proptest::prelude::*;
    use std::future::Future;
    use std::pin::Pin;
    use std::result::Result;

    /// Stub `InferencePort` — never reached by the `abort`-action tests below
    /// (the profile gate fires before the action, or the action converges
    /// without inference). Returns an error if ever called.
    struct StubInference;
    impl InferencePort for StubInference {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            Box::pin(async { Err(InferenceError::Generation("stub".into())) })
        }
    }

    /// Stub `ToolPort` whose `discover_tools` returns a configurable list.
    /// `discover_tools` is the fallback path the executor uses when no
    /// `terminal_check` callback is wired — pinning that fallback is the
    /// point of the third test.
    struct StubToolPort {
        discover: Vec<String>,
    }
    impl ToolPort for StubToolPort {
        fn invoke<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
            _args: Value,
            _token: &'a DelegationToken,
        ) -> ToolFuture<'a, Result<Value, ToolPortError>> {
            Box::pin(async { Err(ToolPortError::InvocationFailed("stub".into())) })
        }
        fn discover_tools<'a>(&'a self) -> ToolFuture<'a, Vec<String>> {
            let discover = self.discover.clone();
            Box::pin(async move { discover })
        }
        fn get_tool_info<'a>(&'a self, _tool_name: &'a str) -> ToolFuture<'a, Option<ToolInfo>> {
            Box::pin(async { None })
        }
    }

    /// Minimal skill manifest with one `profile: ask` step that converges via
    /// `abort` (no inference or tool call required). Used to exercise the
    /// profile gate in isolation.
    const GATE_MANIFEST: &str = "\
manifest:
  id: profile-gate-test
  category: skill
steps:
  - ordinal: 1
    action: abort
    description: converge
    profile: ask
";

    fn make_executor(discover: Vec<String>) -> ManifestExecutor {
        ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(StubToolPort { discover }),
            LLMParameters::default(),
        )
    }

    /// Wiring `with_terminal_check` to report `terminal` enabled must make the
    /// per-step gate refuse the cascade — the proposer/evaluator separation
    /// invariant. This pins the wired path (SF1): before the fix the callback
    /// was never installed and the gate silently never fired.
    #[tokio::test]
    async fn profile_gate_fires_when_terminal_check_says_enabled() {
        let manifest = load_manifest_from_yaml(GATE_MANIFEST).expect("parse");
        let executor = make_executor(vec![]).with_terminal_check(Arc::new(|| true));
        let err = executor
            .execute_manifest(&manifest, HashMap::new())
            .await
            .expect_err("gate must fire when terminal is enabled");
        let msg = err.to_string();
        assert!(
            msg.contains("terminal") && msg.contains("proposer/evaluator separation"),
            "unexpected gate error: {msg}"
        );
    }

    /// When the wired callback reports `terminal` disabled, the gate passes and
    /// the `abort` step converges normally — the gate does not false-positive.
    #[tokio::test]
    async fn profile_gate_passes_when_terminal_check_says_disabled() {
        let manifest = load_manifest_from_yaml(GATE_MANIFEST).expect("parse");
        let executor = make_executor(vec![]).with_terminal_check(Arc::new(|| false));
        let result = executor.execute_manifest(&manifest, HashMap::new()).await;
        assert!(
            result.is_ok(),
            "gate must pass when terminal is disabled; got: {:?}",
            result.err()
        );
    }

    /// When no callback is wired, the gate falls back to `discover_tools()`. This
    /// pins the fallback contract: it can only see MCP tools, never the built-in
    /// `terminal` — so in production (where `terminal` is built-in) the
    /// unwired gate is a no-op that never blocks. The bridge wires the callback
    /// (see `skill_executor.rs`) to avoid relying on this fallback.
    #[tokio::test]
    async fn profile_gate_fallback_uses_discover_tools_when_unwired() {
        let manifest = load_manifest_from_yaml(GATE_MANIFEST).expect("parse");

        // Fallback sees "terminal" advertised -> gate fires.
        let executor = make_executor(vec!["terminal".to_string()]);
        let err = executor
            .execute_manifest(&manifest, HashMap::new())
            .await
            .expect_err("fallback must fire when discover_tools lists terminal");
        assert!(err.to_string().contains("terminal"));

        // Fallback does not see "terminal" -> gate passes, abort converges.
        let executor = make_executor(vec![]);
        let result = executor.execute_manifest(&manifest, HashMap::new()).await;
        assert!(
            result.is_ok(),
            "fallback must pass when discover_tools omits terminal; got: {:?}",
            result.err()
        );
    }

    // ── Restored security regressions (RR-0011/0012/0026/0027/0033/0034) ──
    // These were dropped during test-harness consolidation but are keyed by name
    // in kask/security/regressions/RR-NNNN.yaml (kind: cargo-test), so they must
    // live in this `mod tests` (the gate runs `cargo test --lib`) and keep their
    // exact names. They pin the FIDES taint, spotlight, and runtime-policy
    // invariants that the property tests in tests/ do not cover.

    /// Build a minimal executor with `taint_labels` pre-populated, for testing
    /// `propagate_taint_for_binding` / `extract_referenced_keys` in isolation.
    /// The taint methods never call inference/tools, so the shared stubs suffice.
    fn test_executor_with_taint(taint: Vec<(&str, ToolTaint)>) -> ManifestExecutor {
        let executor = ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(StubToolPort { discover: vec![] }),
            LLMParameters::default(),
        );
        let mut labels = executor
            .taint_labels
            .lock()
            .expect("taint labels mutex not poisoned");
        for (key, label) in taint {
            labels.insert(key.to_string(), label);
        }
        drop(labels);
        executor
    }

    /// RR-0026: an inline-Jinja binding of a Source-tainted entry must
    /// propagate the taint label. Before the fix, `context.insert` (not
    /// `insert_tainted`) was used, so the new key was silently labeled Pure —
    /// bypassing the FIDES Source→Sink block.
    #[test]
    fn propagate_taint_from_inline_jinja_source() {
        let executor = test_executor_with_taint(vec![("step_1_result", ToolTaint::Source)]);
        let value = serde_json::json!("{{ step_1_result }}");
        executor.propagate_taint_for_binding(&value, "bound_data");
        let labels = executor
            .taint_labels
            .lock()
            .expect("taint labels mutex not poisoned");
        assert_eq!(
            labels.get("bound_data").copied(),
            Some(ToolTaint::Source),
            "Source taint must propagate through inline-Jinja bindings"
        );
    }

    /// RR-0027: `execute_populate` must call `propagate_taint_for_binding`
    /// before `context.insert`, otherwise Source-tainted values bound via
    /// `$ref` lose their label and bypass the FIDES Source→Sink block. Same
    /// closure-break class as RR-0026. (execute_populate now uses
    /// `resolve_mapping_value` like every other step action — the legacy
    /// `bind_parameters` was deleted since it passed `{{ }}` through verbatim.)
    #[tokio::test]
    async fn execute_populate_propagates_source_taint() {
        let executor = test_executor_with_taint(vec![("step_1_result", ToolTaint::Source)]);

        let step = BundleManifestStep {
            ordinal: 2,
            action: "populate".to_string(),
            description: "bind step_1_result into data".to_string(),
            renderer: None,
            template_ref: Some("{{data}}".to_string()),
            mcp: None,
            compute_ref: None,
            gas_cap: 0,
            timeout_seconds: 0,
            input_mapping: Some(serde_json::json!({
                "data": {"$ref": "step_1_result"}
            })),
            output_schema: None,
            phase: crate::bundle::cascade::CascadePhase::default(),
            condition: None,
            branching: None,
            branching_field: None,
            profile: None,
        };

        let mut context = HashMap::new();
        context.insert(
            "step_1_result".to_string(),
            Value::String("untrusted external content".to_string()),
        );

        let result = executor
            .execute_populate(&step, context)
            .await
            .expect("execute_populate should succeed with an inline template and $ref binding");

        let labels = executor
            .taint_labels
            .lock()
            .expect("taint labels mutex not poisoned");
        assert_eq!(
            labels.get("data").copied(),
            Some(ToolTaint::Source),
            "execute_populate must propagate Source taint through $ref bindings (RR-0027)"
        );
        assert_eq!(
            result.get("step_2_populated").and_then(|v| v.as_str()),
            Some("untrusted external content"),
        );
        assert_eq!(
            labels.get("step_2_populated").copied(),
            Some(ToolTaint::Source),
            "rendered output derived from Source bindings must inherit the label"
        );
    }

    /// Regression for the `bind_parameters` deletion: a `populate` step whose
    /// `input_mapping` uses an inline-Jinja `{{ }}` binding value must RENDER
    /// the expression (resolve it from context), not pass it through as a
    /// literal string. Before the fix, `execute_populate` used `bind_parameters`,
    /// which returned the literal `"{{ step_1_result }}"` string as the binding —
    /// 12 manifests in the registry hit this (root-cause-analysis, voice-models,
    /// prompt-injection-diagnostic, rag-pipeline, ...).
    #[tokio::test]
    async fn execute_populate_renders_inline_jinja_binding() {
        let executor = ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(StubToolPort { discover: vec![] }),
            LLMParameters::default(),
        );

        let step = BundleManifestStep {
            ordinal: 2,
            action: "populate".to_string(),
            description: "bind via inline jinja".to_string(),
            renderer: None,
            // Template references the bound variable by name.
            template_ref: Some("{{problem_statement}}".to_string()),
            mcp: None,
            compute_ref: None,
            gas_cap: 0,
            timeout_seconds: 0,
            input_mapping: Some(serde_json::json!({
                "problem_statement": "{{ step_1_result }}"
            })),
            output_schema: None,
            phase: crate::bundle::cascade::CascadePhase::default(),
            condition: None,
            branching: None,
            branching_field: None,
            profile: None,
        };

        let mut context = HashMap::new();
        context.insert(
            "step_1_result".to_string(),
            Value::String("the real problem".to_string()),
        );

        let result = executor
            .execute_populate(&step, context)
            .await
            .expect("populate with inline-jinja binding should render");
        assert_eq!(
            result.get("step_2_populated").and_then(|v| v.as_str()),
            Some("the real problem"),
            "inline-Jinja binding in populate input_mapping must render to the context value, \
             not pass through as the literal `{{{{ step_1_result }}}}` string"
        );
    }

    /// Tool port stub that reports a Source-tainted tool and returns a fixed
    /// payload. Used by the spotlight and sub-cascade taint tests to drive the
    /// real `invoke_tool` / `execute_flowdef` paths.
    struct SourceToolPort;

    impl hkask_capability::ToolPort for SourceToolPort {
        fn invoke<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
            _args: Value,
            _token: &'a hkask_capability::DelegationToken,
        ) -> hkask_capability::ToolFuture<
            'a,
            std::result::Result<Value, hkask_capability::ToolPortError>,
        > {
            Box::pin(async { Ok(serde_json::json!("untrusted sub-cascade output")) })
        }

        fn get_tool_info<'a>(
            &'a self,
            _tool_name: &'a str,
        ) -> hkask_capability::ToolFuture<'a, Option<hkask_capability::ToolInfo>> {
            Box::pin(async {
                Some(hkask_capability::ToolInfo {
                    name: "read".to_string(),
                    description: "Source tool stub".to_string(),
                    input_schema: serde_json::json!({}),
                    server_id: "hkask-mcp-stub".to_string(),
                    required_capability: None,
                    taint: ToolTaint::Source,
                })
            })
        }

        fn discover_tools<'a>(&'a self) -> hkask_capability::ToolFuture<'a, Vec<String>> {
            Box::pin(async { vec!["read".to_string()] })
        }
    }

    /// RR-0011: tool outputs must be spotlighted before entering the LLM
    /// context — a refactor that drops the spotlight call from `invoke_tool`
    /// must fail this test (a grep only proves the call exists somewhere).
    #[tokio::test]
    async fn tool_output_is_spotlighted_on_the_invoke_path() {
        let executor = ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(SourceToolPort),
            LLMParameters::default(),
        );
        let (result, _taint) = executor
            .invoke_tool("read", serde_json::json!({}), 1, false)
            .await
            .expect("SourceToolPort invoke should succeed");
        let text = result.as_str().expect("spotlighted output is a string");
        assert!(
            text.contains("untrusted sub-cascade output"),
            "payload must survive spotlighting: {text}"
        );
        assert_ne!(
            text, "untrusted sub-cascade output",
            "output must be transformed by the spotlighter (delimited), not passed through raw"
        );
    }

    /// RR-0012: the runtime policy must gate tool invocation — a `RequireHuman`
    /// verdict must prevent the tool from being invoked at all.
    #[tokio::test]
    async fn runtime_policy_block_prevents_tool_invocation() {
        let executor = ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(SourceToolPort),
            LLMParameters::default(),
        )
        .with_runtime_policy(Arc::new(hkask_regulation::DefaultPolicy::new(
            hkask_regulation::PolicyConfig {
                human_in_loop_tools: ["read".to_string()].into_iter().collect(),
                ..Default::default()
            },
        )));
        let result = executor
            .invoke_tool("read", serde_json::json!({}), 1, false)
            .await;
        let err = result.expect_err("RequireHuman verdict must abort the invocation");
        assert!(
            err.to_string().contains("requires human confirmation"),
            "error must surface the policy verdict: {err}"
        );
    }

    /// RR-0033: the refinement-loop snapshot copies `step_N_result` values to
    /// `prev_step_N_result` keys — it must also copy the taint label, otherwise
    /// a Source-tainted artifact referenced as `prev_step_N_result` bypasses the
    /// FIDES Source→Sink block. The render step uses inline mode (renderer
    /// unset → template_ref is the template string), so "artifact" renders to
    /// itself with no file I/O.
    #[tokio::test]
    async fn loop_snapshot_propagates_source_taint_to_prev_key() {
        let executor = test_executor_with_taint(vec![]);

        let manifest_yaml = r#"
manifest:
  id: taint-loop-snapshot-test
steps:
  - ordinal: 1
    action: render
    description: produce the artifact
    template_ref: "artifact"
  - ordinal: 2
    action: loop
    description: re-enter for one refinement pass
    input_mapping:
      loop_target: "1"
convergence:
  max_iterations: 5
  min_iterations: 0
  on_not_reached: abort
  threshold: 0.0
  convergence_field: composite
"#;
        let manifest =
            load_manifest_from_yaml(manifest_yaml).expect("test manifest YAML must parse");

        executor
            .taint_labels
            .lock()
            .expect("taint labels mutex not poisoned")
            .insert("step_1_result".to_string(), ToolTaint::Source);

        let result = executor
            .execute_manifest(&manifest, HashMap::new())
            .await
            .expect("cascade with a loop pass should succeed");

        assert!(
            result.contains_key("prev_step_1_result"),
            "the loop snapshot must produce prev_step_1_result"
        );
        let labels = executor
            .taint_labels
            .lock()
            .expect("taint labels mutex not poisoned");
        assert_eq!(
            labels.get("prev_step_1_result").copied(),
            Some(ToolTaint::Source),
            "the prev_step_N_result snapshot must carry the Source label (RR-0033)"
        );
    }

    /// RR-0034: when a sub-cascade's final step result is Source-tainted, the
    /// `step_{ordinal}_result` inserted into the parent context must carry the
    /// same label — otherwise a Source-tainted sub-result enters the parent
    /// unlabeled and bypasses the Source→Sink block.
    #[tokio::test]
    async fn sub_cascade_final_result_taint_labels_parent_step_key() {
        let executor = ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(SourceToolPort),
            LLMParameters::default(),
        );

        let tmp = std::env::temp_dir().join("hkask-flowdef-taint-test");
        std::fs::create_dir_all(&tmp).expect("create temp template dir");
        std::fs::write(
            tmp.join("taint-sub.yaml"),
            r#"
manifest:
  id: taint-sub-test
steps:
  - ordinal: 1
    action: execute
    description: read untrusted data
    mcp: "hkask-mcp-stub/read"
convergence:
  max_iterations: 1
  min_iterations: 0
  on_not_reached: abort
  threshold: 0.0
  convergence_field: composite
"#,
        )
        .expect("write sub-manifest");
        let executor = executor.with_template_base_path(tmp.clone());

        let step = BundleManifestStep {
            ordinal: 7,
            action: "flowdef".to_string(),
            description: "run the tainting sub-cascade".to_string(),
            renderer: None,
            template_ref: Some("taint-sub".to_string()),
            mcp: None,
            compute_ref: None,
            gas_cap: 0,
            timeout_seconds: 0,
            input_mapping: None,
            output_schema: None,
            phase: crate::bundle::cascade::CascadePhase::default(),
            condition: None,
            branching: None,
            branching_field: None,
            profile: None,
        };

        let (parent_context, _gas, _rjoule) = executor
            .execute_flowdef(&step, HashMap::new(), 100, 100.0, 0)
            .await
            .expect("sub-cascade with a Source tool should succeed");

        let step_result = parent_context
            .get("step_7_result")
            .and_then(|v| v.as_str())
            .expect("the parent's step key holds the sub-cascade's final result");
        assert!(
            step_result.contains("untrusted sub-cascade output"),
            "the parent's step key holds the sub-cascade's final result (spotlight-wrapped): {step_result}"
        );
        let labels = executor
            .taint_labels
            .lock()
            .expect("taint labels mutex not poisoned");
        assert_eq!(
            labels.get("step_7_result").copied(),
            Some(ToolTaint::Source),
            "the parent's step_7_result must inherit the sub-cascade's final-result Source label (RR-0034)"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// S1 regression: iterative loop re-entry must be bounded by
    /// `convergence.max_iterations`, NOT by `SYSTEM_MAX_RECURSION`. Before the
    /// fix, `recursion_depth` was incremented on every loop re-entry (explicit
    /// `loop` and implicit end-of-pass) and never reset, so a manifest with
    /// `max_iterations: 10` that failed to converge errored at iteration 8 with
    /// "Matryoshka depth limit (7) exceeded" instead of exiting `MaxedOut` at
    /// iteration 10 — silently capping `max_iterations` at the matryoshka limit.
    #[tokio::test]
    async fn iterative_loop_is_bounded_by_max_iterations_not_matryoshka() {
        let executor = ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(StubToolPort { discover: vec![] }),
            LLMParameters::default(),
        );
        let manifest_yaml = r#"
manifest:
  id: matryoshka-regression
steps:
  - ordinal: 1
    action: render
    description: produce artifact
    template_ref: "artifact"
convergence:
  max_iterations: 10
  min_iterations: 0
  on_not_reached: abort
  threshold: 0.0
  convergence_field: composite
"#;
        let manifest = load_manifest_from_yaml(manifest_yaml).expect("manifest must parse");
        let result = executor
            .execute_manifest(&manifest, HashMap::new())
            .await
            .expect("non-converging cascade must exit MaxedOut, not error with matryoshka limit");
        let status = result
            .get("_convergence")
            .and_then(|c| c.get("status"))
            .and_then(|s| s.as_str())
            .expect("_convergence.status must be present");
        assert_eq!(
            status, "maxed_out",
            "non-converging cascade bounded by max_iterations must exit MaxedOut, got {status}"
        );
        let iterations = result
            .get("_convergence")
            .and_then(|c| c.get("iterations_completed"))
            .and_then(|v| v.as_u64())
            .expect("iterations_completed must be present");
        assert_eq!(
            iterations, 10,
            "cascade must run all 10 iterations before MaxedOut (matryoshka previously capped at 7)"
        );
    }

    /// Regression for BUG-1: `last_result_ordinal` was set unconditionally
    /// after the match block, including for `populate` (which stores
    /// `step_N_populated`, not `step_N_result`) and `choice` (which may
    /// fall through without emitting any key). When such a step was the last
    /// to run in a flowdef sub-cascade, `execute_flowdef` looked up
    /// `step_N_result`, got `None`, and silently returned `Value::Null`
    /// instead of falling back to `extract_final_step_result`.
    ///
    /// This test constructs a sub-manifest where step 1 is `select` (writes
    /// `step_1_result`) and step 2 is `populate` (writes `step_2_populated`,
    /// NOT `step_2_result`). The flowdef result must be step 1's output, not
    /// `Value::Null`.
    #[tokio::test]
    async fn flowdef_result_not_null_when_last_step_is_populate() {
        // Stub InferencePort returning valid JSON so the select step succeeds.
        struct JsonInference;
        impl InferencePort for JsonInference {
            fn generate(
                &self,
                _prompt: &str,
                _parameters: &LLMParameters,
                _tools: Option<&[ChatToolDefinition]>,
            ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
            {
                Box::pin(async {
                    Ok(InferenceResult {
                        text: r#"{"answer": 42}"#.to_string(),
                        model: "test".to_string(),
                        usage: hkask_types::InferenceUsage {
                            prompt_tokens: 0,
                            completion_tokens: 0,
                            total_tokens: 0,
                        },
                        finish_reason: "stop".to_string(),
                        token_probabilities: None,
                        tool_calls: vec![],
                        reasoning: None,
                        cost_usd: None,
                    })
                })
            }
        }

        let executor = ManifestExecutor::new(
            Arc::new(JsonInference),
            Arc::new(StubToolPort { discover: vec![] }),
            LLMParameters::default(),
        );

        let tmp = std::env::temp_dir().join("hkask-flowdef-populate-last-test");
        std::fs::create_dir_all(&tmp).expect("create temp template dir");
        // Sub-manifest: step 1 select (writes step_1_result), step 2 populate
        // (writes step_2_populated, NOT step_2_result).
        std::fs::write(
            tmp.join("populate-last-sub.yaml"),
            r#"
manifest:
  id: populate-last-sub
steps:
  - ordinal: 1
    action: select
    description: produce a result
    renderer: inline
    template_ref: "{{ 1 + 1 }}"
    gas_cap: 1000
    timeout_seconds: 5
  - ordinal: 2
    action: populate
    description: produce a populated artifact (not a result)
    renderer: inline
    template_ref: "populated content"
convergence:
  max_iterations: 1
  min_iterations: 0
  on_not_reached: abort
  threshold: 0.0
  convergence_field: composite
gas:
  cap: 10000
  cost_per_iteration: 100
  alert_threshold: 0.8
  hard_limit: true
rjoule:
  cap: 0
  alert_threshold: 0.8
  hard_limit: true
error_handling:
  on_capability_denied: escalate
ledger:
  span_namespace: reg.skill.test
  telemetry_namespace: hkask.template.test
audit:
  enabled: false
"#,
        )
        .expect("write sub-manifest");
        let executor = executor.with_template_base_path(tmp.clone());

        let step = BundleManifestStep {
            ordinal: 7,
            action: "flowdef".to_string(),
            description: "run sub-cascade ending in populate".to_string(),
            renderer: None,
            template_ref: Some("populate-last-sub".to_string()),
            mcp: None,
            compute_ref: None,
            gas_cap: 0,
            timeout_seconds: 0,
            input_mapping: None,
            output_schema: None,
            phase: crate::bundle::cascade::CascadePhase::default(),
            condition: None,
            branching: None,
            branching_field: None,
            profile: None,
        };

        let (parent_context, _gas, _rjoule) = executor
            .execute_flowdef(&step, HashMap::new(), 100, 100.0, 0)
            .await
            .expect("sub-cascade should succeed");

        // The flowdef step's result (step_7_result) must be the select step's
        // output (step_1_result from the sub-cascade), NOT Value::Null.
        let flowdef_result = parent_context
            .get("step_7_result")
            .expect("step_7_result must exist");
        assert!(
            !flowdef_result.is_null(),
            "flowdef result must not be null when sub-cascade ends in populate; got: {flowdef_result}"
        );
        assert_eq!(
            flowdef_result.get("answer"),
            Some(&serde_json::json!(42)),
            "flowdef result must be the select step's output"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// S1 regression: the matryoshka guard must still bound recursive nesting
    /// (flowdef sub-cascades). A flowdef chain nested deeper than
    /// `SYSTEM_MAX_RECURSION` must error with the matryoshka message — this
    /// confirms the guard moved to the recursion edge (run_cascade entry) and
    /// wasn't deleted along with the iterative-loop increments.
    #[tokio::test]
    async fn matryoshka_guard_still_bounds_flowdef_recursion() {
        let tmp = std::env::temp_dir().join("hkask-matryoshka-recursion-test");
        std::fs::create_dir_all(&tmp).expect("create temp template dir");
        // A sub-manifest whose only step re-enters itself via flowdef. Each
        // recursive call increments depth by 1; once depth exceeds
        // SYSTEM_MAX_RECURSION the guard fires.
        std::fs::write(
            tmp.join("self-recurse.yaml"),
            r#"
manifest:
  id: self-recurse
steps:
  - ordinal: 1
    action: flowdef
    description: re-enter self
    template_ref: "self-recurse"
convergence:
  max_iterations: 1
  min_iterations: 0
  on_not_reached: abort
  threshold: 0.0
  convergence_field: composite
"#,
        )
        .expect("write sub-manifest");
        let executor = ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(StubToolPort { discover: vec![] }),
            LLMParameters::default(),
        )
        .with_template_base_path(tmp.clone());

        let step = BundleManifestStep {
            ordinal: 1,
            action: "flowdef".to_string(),
            description: "kick off the recursion".to_string(),
            renderer: None,
            template_ref: Some("self-recurse".to_string()),
            mcp: None,
            compute_ref: None,
            gas_cap: 0,
            timeout_seconds: 0,
            input_mapping: None,
            output_schema: None,
            phase: crate::bundle::cascade::CascadePhase::default(),
            condition: None,
            branching: None,
            branching_field: None,
            profile: None,
        };
        let err = executor
            .execute_flowdef(&step, HashMap::new(), u64::MAX, f64::MAX, 0)
            .await
            .expect_err("deeply nested flowdef recursion must hit the matryoshka guard");
        assert!(
            err.to_string().contains("Matryoshka depth limit"),
            "deeply nested flowdef must error with the matryoshka message, got: {err}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// S2 regression: the FIDES Source→Sink gate (`check_untrusted_input`) must
    /// recognize inline-Jinja `{{ step_N_result }}` references, not only `$ref`.
    /// Before the fix, a Sink tool fed Source data via inline Jinja bypassed the
    /// block because `has_untrusted_input` was computed by scanning only `$ref`.
    /// The propagation (RR-0026/0027) already labeled inline-Jinja bindings; the
    /// gate must scan the same grammar to make the label load-bearing.
    #[tokio::test]
    async fn fides_gate_blocks_sink_tool_fed_via_inline_jinja() {
        struct SinkToolPort;
        impl hkask_capability::ToolPort for SinkToolPort {
            fn invoke<'a>(
                &'a self,
                _server: &'a str,
                _tool: &'a str,
                _args: Value,
                _token: &'a DelegationToken,
            ) -> ToolFuture<'a, Result<Value, ToolPortError>> {
                Box::pin(async {
                    Ok(serde_json::json!(
                        "sink tool must not be invoked when the gate blocks"
                    ))
                })
            }
            fn get_tool_info<'a>(
                &'a self,
                _tool_name: &'a str,
            ) -> ToolFuture<'a, Option<hkask_capability::ToolInfo>> {
                Box::pin(async {
                    Some(hkask_capability::ToolInfo {
                        name: "write".to_string(),
                        description: "Sink tool stub".to_string(),
                        input_schema: serde_json::json!({}),
                        server_id: "hkask-mcp-stub".to_string(),
                        required_capability: None,
                        taint: ToolTaint::Sink,
                    })
                })
            }
            fn discover_tools<'a>(&'a self) -> ToolFuture<'a, Vec<String>> {
                Box::pin(async { vec!["write".to_string()] })
            }
        }

        let executor = ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(SinkToolPort),
            LLMParameters::default(),
        )
        .with_runtime_policy(Arc::new(hkask_regulation::DefaultPolicy::new(
            hkask_regulation::PolicyConfig::default(),
        )));
        // Label step_1_result as Source, as `execute_tool_invoke` would for a
        // Source tool's output. The gate must consult this label when the input
        // references step_1_result via inline Jinja.
        executor
            .taint_labels
            .lock()
            .expect("taint labels mutex not poisoned")
            .insert("step_1_result".to_string(), ToolTaint::Source);

        let step = BundleManifestStep {
            ordinal: 2,
            action: "execute".to_string(),
            description: "sink fed via inline jinja".to_string(),
            renderer: None,
            template_ref: None,
            mcp: Some("hkask-mcp-stub/write".to_string()),
            compute_ref: None,
            gas_cap: 0,
            timeout_seconds: 0,
            input_mapping: Some(serde_json::json!({ "query": "{{ step_1_result }}" })),
            output_schema: None,
            phase: crate::bundle::cascade::CascadePhase::default(),
            condition: None,
            branching: None,
            branching_field: None,
            profile: None,
        };

        let mut context = HashMap::new();
        context.insert(
            "step_1_result".to_string(),
            Value::String("untrusted external content".to_string()),
        );

        let err = executor
            .execute_tool_invoke(&step, &mut context)
            .await
            .expect_err("FIDES gate must block Sink tool fed Source data via inline Jinja");
        assert!(
            err.to_string().contains("blocked"),
            "error must surface the FIDES Source→Sink block verdict: {err}"
        );
    }

    /// #4 compute taint regression: a `compute` step whose `input_mapping` binds
    /// from a Source-tainted context entry must label its `step_N_result` output
    /// as Source. Before the fix, `execute_compute` labeled only the bound
    /// input keys, not the output — so a downstream Sink tool fed the compute
    /// output (derived from Source data) bypassed the FIDES gate, which reads
    /// labels, not content. Uses `kata.hypotenuse` (takes two f64s).
    #[tokio::test]
    async fn compute_output_inherits_source_taint_from_inputs() {
        let executor = ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(StubToolPort { discover: vec![] }),
            LLMParameters::default(),
        );
        executor
            .taint_labels
            .lock()
            .expect("taint labels mutex not poisoned")
            .insert("step_1_result".to_string(), ToolTaint::Source);

        let step = BundleManifestStep {
            ordinal: 2,
            action: "compute".to_string(),
            description: "compute over Source-tainted inputs".to_string(),
            renderer: None,
            template_ref: None,
            mcp: None,
            compute_ref: Some("kata.hypotenuse".to_string()),
            gas_cap: 0,
            timeout_seconds: 0,
            input_mapping: Some(serde_json::json!({
                "object_gap": {"$ref": "step_1_result.object_gap"},
                "process_gap": {"$ref": "step_1_result.process_gap"}
            })),
            output_schema: None,
            phase: crate::bundle::cascade::CascadePhase::default(),
            condition: None,
            branching: None,
            branching_field: None,
            profile: None,
        };

        let mut context = HashMap::new();
        context.insert(
            "step_1_result".to_string(),
            serde_json::json!({"object_gap": 0.3, "process_gap": 0.4}),
        );

        let result = executor
            .execute_compute(&step, context)
            .await
            .expect("kata.hypotenuse over numeric inputs should succeed");
        // Sanity: the compute ran.
        assert!(
            result.contains_key("step_2_result"),
            "compute output must be stored"
        );
        let labels = executor
            .taint_labels
            .lock()
            .expect("taint labels mutex not poisoned");
        assert_eq!(
            labels.get("step_2_result").copied(),
            Some(ToolTaint::Source),
            "compute output derived from Source-tainted inputs must inherit the Source label"
        );
    }

    /// #4 extract_feedback_phase contract: pins the phase derivation from a
    /// step's template_ref. The `write`/`feedback` ordering matters — a segment
    /// containing both (`write-feedback`) must resolve to OperatorFeedback, not
    /// Write. `adversarial-convergence-check` (mid-segment match) must resolve to
    /// Convergence.
    #[test]
    fn extract_feedback_phase_resolves_known_refs() {
        assert_eq!(
            extract_feedback_phase("sankey-flow/sankey-classify"),
            Some("classify")
        );
        assert_eq!(
            extract_feedback_phase("diataxis-diagram/diataxis-diagram-generate"),
            Some("draft")
        );
        assert_eq!(
            extract_feedback_phase("skill-x/evaluate-report"),
            Some("evaluate")
        );
        assert_eq!(
            extract_feedback_phase("adversarial-red-team/adversarial-convergence-check"),
            Some("convergence")
        );
        // `write-feedback` contains both `write` and `feedback`; feedback must win.
        assert_eq!(
            extract_feedback_phase("skill-x/write-operator-feedback"),
            Some("operator_feedback")
        );
        // A bare `write` (no feedback) still resolves to Write.
        assert_eq!(
            extract_feedback_phase("skill-x/write-report"),
            Some("write")
        );
        assert_eq!(extract_feedback_phase("skill-x/unknown-step"), None);
    }

    /// rJoule (USD) wiring (#3): `execute_select` must charge the inference
    /// call's `cost_usd` to the rJoule budget (1 rJoule = $1 USD). The
    /// InferencePort populates `cost_usd` from token usage × the model's
    /// per-token price; the executor passes it to `BudgetTracker::charge_rjoule`.
    /// This pins the core wiring that makes `rjoule.cap` a live USD budget.
    #[tokio::test]
    async fn execute_select_charges_rjoule_from_cost_usd() {
        // Stub InferencePort returning a result with a known USD cost.
        struct PricedInference;
        impl InferencePort for PricedInference {
            fn generate(
                &self,
                _prompt: &str,
                _parameters: &LLMParameters,
                _tools: Option<&[ChatToolDefinition]>,
            ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
            {
                Box::pin(async {
                    Ok(InferenceResult {
                        text: "{}".to_string(), // valid JSON so parse_json_response succeeds
                        model: "testpriced".to_string(),
                        usage: hkask_types::InferenceUsage {
                            prompt_tokens: 1_000_000,
                            completion_tokens: 0,
                            total_tokens: 1_000_000,
                        },
                        finish_reason: "stop".to_string(),
                        token_probabilities: None,
                        tool_calls: vec![],
                        reasoning: None,
                        cost_usd: Some(0.50), // $0.50 → 0.50 rJoule
                    })
                })
            }
        }

        let executor = ManifestExecutor::new(
            Arc::new(PricedInference),
            Arc::new(StubToolPort { discover: vec![] }),
            LLMParameters::default(),
        );

        let step = BundleManifestStep {
            ordinal: 1,
            action: "select".to_string(),
            description: "priced inference call".to_string(),
            renderer: None,
            template_ref: Some("{{data}}".to_string()),
            mcp: None,
            compute_ref: None,
            gas_cap: 0,
            timeout_seconds: 30,
            input_mapping: None,
            output_schema: None,
            phase: crate::bundle::cascade::CascadePhase::default(),
            condition: None,
            branching: None,
            branching_field: None,
            profile: None,
        };

        use crate::budget::BudgetTracker;
        use crate::bundle::config::{BundleGasConfig, RjouleConfig};
        let gas = BundleGasConfig {
            cap: u32::MAX,
            cost_per_iteration: 0,
            alert_threshold: 1.0,
            hard_limit: false,
        };
        let rjoule = RjouleConfig {
            cap: 1, // $1 budget (1 rJoule = $1 USD)
            alert_threshold: 0.8,
            hard_limit: true,
        };
        let mut budget = BudgetTracker::new(&gas, &rjoule);

        let _result = executor
            .execute_select(&step, HashMap::new(), &mut budget)
            .await
            .expect("select with priced inference should succeed");

        let snap = budget.snapshot();
        assert!(
            (snap.rjoule_used - 0.50).abs() < 1e-9,
            "execute_select must charge cost_usd ($0.50) to the rJoule budget, got {} rJoule",
            snap.rjoule_used
        );
    }

    /// Fix #1 regression: the `loop` arm must bind `input_mapping` BEFORE
    /// `push_cycle_from_context` reads `convergence_signal` from the context.
    /// The prior ordering (push → bind) read a one-iteration-stale signal —
    /// the first iteration pushed NaN (no binding yet) and subsequent
    /// iterations pushed the *previous* iteration's signal. With the fix,
    /// the first iteration pushes the current signal.
    ///
    /// This manifest uses a Kata-enabled `gap` convergence mode with
    /// `min_iterations: 0`. Step 1 (compute) produces 0.0 (below `gap_epsilon`).
    /// Step 2 (loop) binds `convergence_signal: "{{ step_1_result }}"` and
    /// loops back to step 1. With the fix, the signal history is `[0.0]` and
    /// the cascade converges at iteration 1. Without the fix, the history is
    /// `[NaN, 0.0]` and convergence is delayed to iteration 2.
    #[tokio::test]
    async fn loop_arm_binds_convergence_signal_before_push() {
        let executor = ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(StubToolPort { discover: vec![] }),
            LLMParameters::default(),
        );

        let manifest_yaml = r#"
manifest:
  id: loop-signal-ordering-test
steps:
  - ordinal: 1
    action: compute
    compute_ref: lisp.eval
    description: produce a convergence signal of 0.0
    input_mapping:
      form: "0"
  - ordinal: 2
    action: loop
    description: bind convergence_signal and loop back to step 1
    input_mapping:
      loop_target: "1"
      convergence_signal: "{{ step_1_result }}"
convergence:
  max_iterations: 5
  min_iterations: 0
  on_not_reached: abort
  threshold: 0.0
  convergence_field: composite
  convergence_mode: gap
  target_artifacts_field: current_artifacts
  current_artifacts_field: current_artifacts
  target_procedure_field: current_procedure
  current_procedure_field: current_procedure
  gap_epsilon: 0.05
  cauchy_epsilon: 0.03
  cauchy_window: 3
  brier_window: 3
  brier_threshold: 0.15
gas:
  cap: 100000
  cost_per_iteration: 1
  alert_threshold: 0.8
  hard_limit: true
rjoule:
  cap: 0
  alert_threshold: 0.8
  hard_limit: true
error_handling:
  on_capability_denied: escalate
ledger:
  span_namespace: reg.skill.test
  telemetry_namespace: hkask.template.test
audit:
  enabled: false
"#;
        let manifest =
            load_manifest_from_yaml(manifest_yaml).expect("test manifest YAML must parse");

        let result = executor
            .execute_manifest(&manifest, HashMap::new())
            .await
            .expect("Kata gap cascade with signal 0.0 should converge");

        let convergence = result
            .get("_convergence")
            .expect("_convergence key present");
        let iterations = convergence
            .get("iterations_completed")
            .and_then(|v| v.as_u64())
            .expect("iterations_completed present");
        assert_eq!(
            iterations, 1,
            "with the fix, the signal is bound before push, so the gap check \
             sees [0.0] and converges at iteration 1. Without the fix, the first \
             push is NaN (no binding yet), delaying convergence to iteration 2."
        );

        // The signal history must contain no NaN — every reading is the
        // current iteration's bound value, not a stale one.
        let signal_history = convergence
            .get("signal_history")
            .and_then(|v| v.as_array())
            .expect("signal_history present");
        assert_eq!(
            signal_history.len(),
            1,
            "converged at iteration 1 → exactly one signal reading"
        );
        let signal = signal_history[0].as_f64().expect("signal is a number");
        assert!(
            signal.is_finite(),
            "the first signal reading must be finite (0.0), not NaN — \
             NaN indicates the push happened before the input_mapping binding"
        );
        assert!(
            (signal - 0.0).abs() < 1e-9,
            "signal must be 0.0, got {signal}"
        );
    }

    /// Fix #2 regression: `execute_flowdef` must report the sub-cascade's ACTUAL
    /// gas/rjoule usage (from the returned `BudgetSnapshot`), not the capped cap.
    /// The prior implementation reported `sub_gas_cap` as consumed — conservative
    /// but distorted the parent's gas feedback loop: a sub-cascade that converged
    /// in 1 iteration using 1 gas of a 1000 cap still deducted 1000 from the
    /// parent, which could prematurely exhaust the parent's budget.
    ///
    /// This test constructs a sub-manifest with `gas.cap: 1000` and
    /// `cost_per_iteration: 1` that runs a single `render` step (no inference,
    /// no gas charge — `render` doesn't call `charge_iteration`). The sub-cascade
    /// uses 0 gas. The parent has `gas.cap: 100`. The parent's `gas_used` after
    /// the flowdef step must be 0 (actual usage), not 1000 (the capped cap, which
    /// would exceed the parent's own cap and be a clear bug) or 100 (clamped to
    /// parent remaining).
    #[tokio::test]
    async fn flowdef_reports_actual_gas_usage_not_capped_cap() {
        let executor = ManifestExecutor::new(
            Arc::new(StubInference),
            Arc::new(StubToolPort { discover: vec![] }),
            LLMParameters::default(),
        );

        let tmp = std::env::temp_dir().join("hkask-flowdef-gas-actual-test");
        std::fs::create_dir_all(&tmp).expect("create temp template dir");
        // Sub-manifest: a single render step. `render` does not call
        // `budget.charge_iteration` (only `select` does), so gas_used stays 0.
        // The sub-manifest declares gas.cap: 1000 — the prior code reported
        // this full cap as consumed, not the actual 0.
        std::fs::write(
            tmp.join("gas-actual-sub.yaml"),
            r#"
manifest:
  id: gas-actual-sub
steps:
  - ordinal: 1
    action: render
    description: produce a result without inference (no gas charge)
    template_ref: "sub-cascade output"
convergence:
  max_iterations: 1
  min_iterations: 0
  on_not_reached: abort
  threshold: 0.0
  convergence_field: composite
gas:
  cap: 1000
  cost_per_iteration: 1
  alert_threshold: 0.8
  hard_limit: true
rjoule:
  cap: 0
  alert_threshold: 0.8
  hard_limit: true
error_handling:
  on_capability_denied: escalate
ledger:
  span_namespace: reg.skill.test
  telemetry_namespace: hkask.template.test
audit:
  enabled: false
"#,
        )
        .expect("write sub-manifest");
        let executor = executor.with_template_base_path(tmp.clone());

        let step = BundleManifestStep {
            ordinal: 7,
            action: "flowdef".to_string(),
            description: "run sub-cascade that uses 0 gas".to_string(),
            renderer: None,
            template_ref: Some("gas-actual-sub".to_string()),
            mcp: None,
            compute_ref: None,
            gas_cap: 0,
            timeout_seconds: 0,
            input_mapping: None,
            output_schema: None,
            phase: crate::bundle::cascade::CascadePhase::default(),
            condition: None,
            branching: None,
            branching_field: None,
            profile: None,
        };

        // Parent has gas.cap: 100. The sub-cascade's cap (1000) is capped to
        // the parent's remaining (100). The prior code reported 100 (the
        // capped cap) as consumed; the fix reports 0 (actual usage).
        let (_parent_context, gas_consumed, _rjoule_consumed) = executor
            .execute_flowdef(&step, HashMap::new(), 100, 0.0, 0)
            .await
            .expect("sub-cascade should succeed");

        assert_eq!(
            gas_consumed, 0,
            "execute_flowdef must report the sub-cascade's actual gas usage (0), \
             not the capped cap (100). The sub-cascade's only step is `render`, \
             which does not call `budget.charge_iteration`."
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── normalize_model_output: <thinking> tag stripping (Wang 2026, A.4) ──

    #[test]
    fn normalize_model_output_passes_through_non_string() {
        let value = serde_json::json!({"answer": 42});
        let out = normalize_model_output(&value);
        assert!(matches!(out, Cow::Borrowed(_)), "non-string must borrow");
        assert_eq!(*out, value);
    }

    #[test]
    fn normalize_model_output_borrows_clean_string() {
        let value = serde_json::json!("Answer: 5");
        let out = normalize_model_output(&value);
        assert!(matches!(out, Cow::Borrowed(_)), "clean string must borrow");
        assert_eq!(*out, value);
    }

    #[test]
    fn normalize_model_output_strips_paired_thinking_tags() {
        let value = serde_json::json!("<thinking>let me reason</thinking>Answer: 5");
        let out = normalize_model_output(&value);
        assert!(matches!(out, Cow::Owned(_)), "dirty string must own");
        assert_eq!(*out, serde_json::json!("Answer: 5"));
    }

    #[test]
    fn normalize_model_output_strips_multiple_paired_blocks() {
        let value = serde_json::json!(
            "<thinking>first</thinking>Step 1 done<thinking>second</thinking>Step 2 done"
        );
        let out = normalize_model_output(&value);
        assert_eq!(*out, serde_json::json!("Step 1 doneStep 2 done"));
    }

    #[test]
    fn normalize_model_output_strips_stray_closing_tag() {
        // Streaming chunk boundary may truncate the opening tag, leaving
        // only the closing tag. This is the Kimi K2 failure mode from the
        // paper's Appendix A.4.
        let value = serde_json::json!("Answer: 5</thinking>");
        let out = normalize_model_output(&value);
        assert_eq!(*out, serde_json::json!("Answer: 5"));
    }

    #[test]
    fn normalize_model_output_leaves_unclosed_opening_tag_untouched() {
        // An unclosed `<thinking` (no `>`) breaks out of the strip loop; the
        // stray `</thinking>` pass removes only closing tags. The opening
        // fragment is left as-is rather than corrupting the string.
        let value = serde_json::json!("<thinking without close Answer: 5");
        let out = normalize_model_output(&value);
        assert_eq!(*out, value, "unclosed opening must pass through unchanged");
    }

    #[test]
    fn extract_final_step_result_strips_thinking_tags_from_result() {
        let mut map: HashMap<String, Value> = HashMap::new();
        map.insert(
            "step_1_result".to_string(),
            serde_json::json!("<thinking>reasoning</thinking>{\"answer\": 5}"),
        );
        let out = extract_final_step_result(&map);
        assert_eq!(out, serde_json::json!("{\"answer\": 5}"));
    }

    proptest! {
        /// P1 invariant: `extract_referenced_keys` must recognize any key
        /// present in `taint_labels` — not just `step_`-prefixed keys. This
        /// pins the fix that closed the FIDES L4 taint blind spot: a
        /// Source-tainted value bound under a non-`step_`-prefixed name
        /// (e.g. `user_query`, `crafted_url`) must propagate its taint label
        /// so the Source→Sink block fires (RR-0053 companion).
        #[test]
        fn extract_referenced_keys_recognizes_taint_labels_key(
            key in "[a-z_][a-z0-9_]{0,20}"
        ) {
            let executor = test_executor_with_taint(vec![(&key, ToolTaint::Source)]);
            // Reference the key via inline Jinja.
            let value = serde_json::json!(format!("{{{{ {key} }}}}"));
            let referenced = executor.extract_referenced_keys(&value);
            prop_assert!(
                referenced.contains(&key),
                "extract_referenced_keys must recognize taint-labels key '{}': got {:?}",
                key, referenced
            );
        }

        /// P1 invariant: `propagate_taint_for_binding` must label the new key
        /// with Source taint when the original value references a Source-tainted
        /// key — for any key name, not just `step_`-prefixed keys.
        #[test]
        fn propagate_taint_for_non_step_prefixed_key(
            source_key in "[a-z_][a-z0-9_]{0,20}",
            bound_key in "[a-z_][a-z0-9_]{0,20}"
        ) {
            prop_assume!(source_key != bound_key);
            let executor = test_executor_with_taint(vec![(&source_key, ToolTaint::Source)]);
            let value = serde_json::json!(format!("{{{{ {source_key} }}}}"));
            executor.propagate_taint_for_binding(&value, &bound_key);
            let labels = executor
                .taint_labels
                .lock()
                .expect("taint labels mutex not poisoned");
            prop_assert_eq!(
                labels.get(&bound_key).copied(),
                Some(ToolTaint::Source),
                "Source taint must propagate from '{}' to '{}' for non-step_-prefixed keys",
                source_key, bound_key
            );
        }

        /// P1 invariant: `check_untrusted_input` must return true when a value
        /// references a Source-tainted key — for any key name in taint_labels.
        #[test]
        fn check_untrusted_input_recognizes_taint_labels_key(
            key in "[a-z_][a-z0-9_]{0,20}"
        ) {
            let executor = test_executor_with_taint(vec![(&key, ToolTaint::Source)]);
            let value = serde_json::json!(format!("{{{{ {key} }}}}"));
            prop_assert!(
                executor.check_untrusted_input(&value),
                "check_untrusted_input must return true for Source-tainted key '{}'",
                key
            );
        }
    }
}
