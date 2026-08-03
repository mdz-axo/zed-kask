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
//!   incrementing the iteration counter. Respects matryoshka depth limit (7).
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

use crate::budget::BudgetTracker;
use crate::bundle::BundleManifest;
use crate::bundle::BundleManifestStep;
use crate::compute::dispatch_compute;
use crate::condition::{evaluate_step_condition, parse_choice_condition};
use crate::convergence::{ConvergenceStatus, ConvergenceTracker};
use crate::input_mapping::{bind_parameters, resolve_mapping_value};
use crate::load_manifest_from_yaml;
use crate::output_schema::{build_structured_output_tool, resolve_output_schema};
use crate::ports::{Result, TemplateError};
use crate::template_renderer::TemplateRenderer;
use hkask_capability::{DelegationAction, DelegationResource};
use hkask_capability::{ToolPort, ToolPortError};
use hkask_guard::{SpotlightMode, Spotlighter};
use hkask_regulation::SkillFeedbackSpan;
use hkask_types::NotFound;
use hkask_types::ToolTaint;
use hkask_types::WebID;
use hkask_types::json_extract as llm_json;
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, InferencePort, InferenceResult};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

/// Extract the PDCA feedback phase from a template_ref string.
///
/// Template refs look like "sankey-flow/sankey-classify" or
/// "diataxis-diagram/diataxis-diagram-generate". The phase is extracted
/// from the last segment after the final '-' or '/'. Returns None if the
/// segment doesn't match one of the six canonical phases.
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
    // Order matters: check longer/more-specific patterns first to avoid
    // false positives (e.g. "convergence" before "converge", "operator_feedback"
    // before "feedback").
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
    } else if last_segment.contains("write") {
        Some(SkillFeedbackSpan::Write.phase())
    } else if last_segment.contains("operator_feedback") || last_segment.contains("feedback") {
        Some(SkillFeedbackSpan::OperatorFeedback.phase())
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
    /// Trust provenance of the manifest being executed. Used to emit
    /// `tracing::warn!` when high-risk actions (`flowdef` sub-cascades,
    /// `compute` primitives) execute from filesystem (untrusted) manifests.
    /// Defaults to `Embedded` (trusted by construction). Set via
    /// `with_provenance` by the bridge when the manifest was loaded from
    /// the filesystem.
    provenance: hkask_types::Provenance,
    /// Optional callback to check if the `terminal` built-in tool is enabled
    /// for the current agent profile. Wired by the bridge with
    /// `AgentProfileSettings::is_tool_enabled("terminal")`. When present,
    /// profile enforcement uses this (the correct check — `terminal` is a
    /// built-in agent tool, not an MCP tool, so `discover_tools()` won't find
    /// it in production). When absent (unit tests), falls back to
    /// `ToolPort::discover_tools()` (which works with test stubs that
    /// advertise `terminal`).
    terminal_check: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl ManifestExecutor {
    /// Create a new executor with the given infrastructure ports.
    ///
    /// expect: "The system resolves and executes template manifest cascades"
    /// \[P3\] Motivating: Generative Space — executor for template manifest cascades
    /// \[P4\] Constraining: Clear Boundaries — requires A2A secret for delegation
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
            provenance: hkask_types::Provenance::Embedded,
            terminal_check: None,
        }
    }

    /// Set the trust provenance of the manifest being executed. Used by the
    /// bridge to indicate whether the manifest was loaded from the embedded
    /// registry (trusted) or the filesystem (untrusted). The executor emits
    /// `tracing::warn!` when high-risk actions execute from filesystem manifests.
    pub fn with_provenance(mut self, provenance: hkask_types::Provenance) -> Self {
        self.provenance = provenance;
        self
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

    /// Check whether a JSON value references any tainted (Source) context entries.
    ///
    /// This is the FIDES taint propagation check: recursively scans the value
    /// for `{"$ref": "step_N_result..."}` patterns and checks whether the
    /// referenced context entry is labeled `Source` (untrusted).
    ///
    /// Source: Microsoft Research FIDES (arXiv:2505.23643)
    ///
    /// expect: "The system detects untrusted data flowing into tool inputs"
    /// pre:  value is the bound input JSON for a tool invocation
    /// post: returns true iff any $ref in the value resolves to a Source-labeled entry
    fn check_untrusted_input(&self, value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                // Check for $ref pattern: {"$ref": "step_1_result.field"}
                if let Some(Value::String(ref_path)) = map.get("$ref") {
                    let context_key = ref_path.split('.').next().unwrap_or("");
                    let labels = self.taint_labels.lock().unwrap_or_else(|e| e.into_inner());
                    return labels.get(context_key).copied().unwrap_or(ToolTaint::Pure)
                        == ToolTaint::Source;
                }
                // Recurse into object fields.
                map.values().any(|v| self.check_untrusted_input(v))
            }
            Value::Array(arr) => arr.iter().any(|v| self.check_untrusted_input(v)),
            _ => false,
        }
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
                        // Only treat as a context key if it looks like a step
                        // result or a known context variable. Step results are
                        // the primary Source-tainted entries.
                        if tok.starts_with("step_") || tok == "task" || tok == "prev_step" {
                            keys.push(tok.to_string());
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
                    return Err(TemplateError::Manifest(format!(
                        "Runtime policy blocked tool '{tool_name}': {reason}"
                    )));
                }
                PolicyVerdict::RequireHuman(reason) => {
                    return Err(TemplateError::Manifest(format!(
                        "Runtime policy requires human confirmation for '{tool_name}': {reason}"
                    )));
                }
                PolicyVerdict::Log(message) => {
                    info!(target: "reg.guard.runtime_policy", tool = tool_name, %message, "REG");
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
        let mut context = initial_context;
        let mut steps = manifest.steps.clone();
        steps.sort_by_key(|s| s.ordinal);

        // Unified convergence tracking (extracted to `convergence.rs`).
        // Replaces 5 `let` locals (max_iterations, threshold, field,
        // improvement_enabled, min_iterations, baseline_quality) with one tracker.
        let mut convergence = ConvergenceTracker::new(&manifest.convergence);
        let max_iterations = convergence.max_iterations();
        let threshold = convergence.threshold();
        let field = convergence.field().to_string();
        let improvement_enabled = convergence.improvement_enabled();
        let mut iteration: u32 = 0;
        let mut recursion_depth: u8 = 0;
        let matryoshka_limit: u8 = hkask_capability::SYSTEM_MAX_RECURSION;
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
                    "loop" => {
                        recursion_depth += 1;
                        if recursion_depth > matryoshka_limit {
                            info!(
                                target: "reg.skill.convergence.escalated",
                                iteration = iteration,
                                reason = "matryoshka depth exceeded",
                                depth = recursion_depth,
                                limit = matryoshka_limit,
                                "REG"
                            );
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
                            return Err(TemplateError::Manifest(format!(
                                "Matryoshka depth limit ({}) exceeded at iteration {}",
                                matryoshka_limit, iteration
                            )));
                        }

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
                            depth = recursion_depth,
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

                        // Record this iteration's convergence data in the
                        // trajectory history BEFORE the convergence check.
                        // For the Kata model, the hypotenuse and Brier score
                        // are read from the context (produced by `compute`
                        // steps with compute_ref: kata.hypotenuse and
                        // kata.prediction_vs_result). For the legacy model,
                        // the self-grade metric is read from the convergence
                        // field. If the Kata fields aren't present, falls back
                        // to pushing NaN (a missing reading is not a converged
                        // reading).
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

                        // Bind loop input_mapping (except loop_target) into context so
                        // carried state (e.g. prior_probability) is available next iteration.
                        if let Some(ref mapping) = step.input_mapping
                            && let Value::Object(map) = mapping
                        {
                            for (k, v) in map {
                                if k == "loop_target" {
                                    continue;
                                }
                                let bound = resolve_mapping_value(
                                    v,
                                    &context,
                                    self.template_renderer.base_path(),
                                );
                                // Propagate taint from referenced Source entries
                                // to the new binding key (ART-3/IR-1 fix).
                                self.propagate_taint_for_binding(v, k);
                                context.insert(k.clone(), bound);
                            }
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
                        for step in steps.iter() {
                            let key = format!("step_{}_result", step.ordinal);
                            if let Some(val) = context.get(&key) {
                                let prev_key = format!("prev_{}", key);
                                // The snapshot copies the value, so it must
                                // also copy the taint label — otherwise a
                                // Source-tainted artifact silently loses its
                                // label when referenced as prev_step_N_result.
                                let label = self
                                    .taint_labels
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .get(&key)
                                    .copied();
                                if let Some(label) = label {
                                    self.taint_labels
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .insert(prev_key.clone(), label);
                                }
                                context.insert(prev_key, val.clone());
                            }
                        }

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
                        if self.provenance == hkask_types::Provenance::Filesystem {
                            tracing::warn!(
                                target: "reg.skill.provenance",
                                action = "compute",
                                step = step.ordinal,
                                compute_ref = ?step.compute_ref,
                                "High-risk action (compute) executing from filesystem-provenance manifest (untrusted)"
                            );
                        }
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
                        if self.provenance == hkask_types::Provenance::Filesystem {
                            tracing::warn!(
                                target: "reg.skill.provenance",
                                action = "flowdef",
                                step = step.ordinal,
                                template_ref = ?step.template_ref,
                                "High-risk action (flowdef sub-cascade) executing from filesystem-provenance manifest (untrusted)"
                            );
                        }
                        let (new_context, gas_consumed, rjoule_consumed) = self
                            .execute_flowdef(
                                step,
                                context,
                                budget.remaining_gas(),
                                budget.remaining_rjoule(),
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
            // hypotenuse and Brier score from the context (produced by
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

            // Implicit loop: re-enter from step 0
            recursion_depth += 1;
            if recursion_depth > matryoshka_limit {
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
                return Err(TemplateError::Manifest(format!(
                    "Matryoshka depth limit ({}) exceeded at iteration {}",
                    matryoshka_limit, iteration
                )));
            }
        }

        context.insert(
            "_recursion_depth".to_string(),
            Value::Number(recursion_depth.into()),
        );
        Ok(context)
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
            None => return Ok(None),
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
                            // Handled by subsequent abort/escalate step; return None to continue
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
                let bound = resolve_mapping_value(v, &context, self.template_renderer.base_path());
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

        let (result_text, tool_calls): (String, Vec<hkask_types::StructuredToolCall>) = {
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
            (result.text, result.tool_calls)
        };

        // rJoule tracking — cost per token comes from inference provider.
        // Token count is tracked; rJoule deduction wired when provider reports cost.
        // TODO: wire rJoule deduction once InferenceResult reports token counts.
        // For now, gas tracking (below) is the only budget enforcement; rJoule
        // is checked at the cascade-loop level via the cap, not per-call.
        // (When wired, call `budget.charge_rjoule(...)` here.)

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
        // Resolve bindings from input_mapping and merge into context.
        // Uses {"$ref": "dot.path"} syntax — same as execute_tool_invoke.
        if let Some(ref mapping) = step.input_mapping
            && let Value::Object(orig_map) = mapping
        {
            let resolved = bind_parameters(mapping, &context);
            if let Value::Object(resolved_map) = resolved {
                // Iterate original and resolved maps in lockstep: the original
                // mapping value carries the $ref / {{ }} markers that
                // propagate_taint_for_binding inspects to find referenced keys;
                // the resolved value is what gets inserted into context.
                for (k, orig_v) in orig_map {
                    let bound = resolved_map.get(k).cloned().unwrap_or(Value::Null);
                    // Propagate taint from referenced Source entries to the new
                    // binding key (RR-0027 — same FIDES closure break as RR-0026,
                    // missed here because execute_populate uses bind_parameters
                    // instead of resolve_mapping_value). Pass the *original*
                    // mapping value (with $ref markers), not the resolved value.
                    self.propagate_taint_for_binding(orig_v, k);
                    context.insert(k.clone(), bound);
                }
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
                let bound = resolve_mapping_value(v, &context, self.template_renderer.base_path());
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
    /// steps. It is loaded from the embedded registry (or filesystem fallback),
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
            return Err(TemplateError::NotFound(format!(
                "Step {}: flowdef sub-manifest '{}' not found on filesystem or in embedded registry",
                step.ordinal, template_ref
            )));
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
                let bound = resolve_mapping_value(v, &context, self.template_renderer.base_path());
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
        // sized.
        let sub_result = Box::pin(self.execute_manifest(&sub_manifest, context)).await?;

        // Extract the sub-cascade's final result value. We do NOT merge the
        // full sub-context back into the parent — only the result is stored,
        // preventing the sub-cascade from overwriting parent context keys.
        //
        // The final result is the highest-ordinal `step_N_result` key —
        // HashMap iteration order is randomized, so we can't use `.last()`.
        // This mirrors the bridge's `extract_final_step_result` logic.
        let result_value = extract_final_step_result(&sub_result);

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
        // same ordinal key extract_final_step_result picked) so a Source-
        // tainted sub-result doesn't enter the parent context unlabeled.
        let final_step_key = extract_final_step_entry(&sub_result).map(|(key, _)| key);
        if let Some(ref final_key) = final_step_key {
            let label = self
                .taint_labels
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(final_key)
                .copied();
            if let Some(label) = label {
                self.taint_labels
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(format!("step_{}_result", step.ordinal), label);
            }
        }
        parent_context.insert(format!("step_{}_result", step.ordinal), result_value);

        // Compute gas/rjoule consumed by the sub-cascade. The sub-cascade's
        // gas_cap was capped to the parent's remaining budget, so the
        // consumption is at most parent_gas_remaining. We report the capped
        // budget as consumed if the sub-cascade exhausted its budget, or the
        // actual usage if we can determine it. Since execute_manifest doesn't
        // return gas accounting, we use the capped cap as an upper bound —
        // the parent deducts the sub-cascade's budget allocation. This is
        // conservative (may over-count) but safe (never under-counts).
        let gas_consumed = sub_gas_cap;
        let rjoule_consumed = sub_rjoule_cap;

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
            .map(|mapping| {
                resolve_mapping_value(mapping, context, self.template_renderer.base_path())
            })
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
                // bound context values via Jinja (e.g. kata.convergence_check's
                // histories degraded to empty defaults; swarm.converge_accumulate
                // hard-errored on get_f64). $ref objects and literal values pass
                // through unchanged, so existing compute bindings are unaffected.
                if let Value::Object(map) = mapping {
                    let mut out = serde_json::Map::new();
                    for (k, v) in map {
                        let bound =
                            resolve_mapping_value(v, &context, self.template_renderer.base_path());
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
                // owns the embedded→filesystem ladder and the .j2/.yaml fallbacks.
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
fn extract_final_step_result(context: &HashMap<String, Value>) -> Value {
    extract_final_step_entry(context)
        .map(|(_, value)| value)
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
    /// closure-break class as RR-0026, missed because `execute_populate` uses
    /// `bind_parameters` (not `resolve_mapping_value`).
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
            .execute_flowdef(&step, HashMap::new(), 100, 100.0)
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
}
