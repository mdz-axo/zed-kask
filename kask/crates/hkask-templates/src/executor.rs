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
use crate::convergence::{ConvergenceStatus, ConvergenceTracker};
use crate::load_manifest_from_yaml;
use crate::ports::{Result, TemplateError};
use crate::template_renderer::{TemplateRenderer, render_minijinja};
use hkask_capability::{DelegationAction, DelegationResource, DelegationToken};
use hkask_capability::{ToolPort, ToolPortError};
use hkask_guard::{SpotlightMode, Spotlighter};
use hkask_regulation::SkillFeedbackSpan;
use hkask_types::NotFound;
use hkask_types::ToolTaint;
use hkask_types::WebID;
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, ChatToolFunction, InferencePort, InferenceResult};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};
use zeroize::Zeroizing;

/// Error healing callback: (error_string, operation_name).
type HealCallback = Arc<dyn Fn(&str, &str) + Send + Sync>;

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
    /// Secret for minting delegation tokens. Zeroized on drop.
    a2a_secret: Zeroizing<Vec<u8>>,
    /// Base filesystem path for resolving template_ref values.
    /// When `step.renderer == "minijinja"`, `step.template_ref` is resolved
    /// relative to this path. Defaults to `registry/templates/`.
    template_renderer: TemplateRenderer,
    /// Optional heal callback: (error_string, operation_name).
    heal_error_cb: Option<HealCallback>,
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
}

impl ManifestExecutor {
    /// Create a new executor with the given infrastructure ports.
    ///
    /// expect: "The system resolves and executes template manifest cascades"
    /// \[P3\] Motivating: Generative Space — executor for template manifest cascades
    /// \[P4\] Constraining: Clear Boundaries — requires A2A secret for delegation
    /// pre:  inference and mcp are initialized, a2a_secret is non-empty
    /// post: returns ManifestExecutor with default template_base_path
    pub fn new(
        inference: Arc<dyn InferencePort>,
        tools: Arc<dyn ToolPort>,
        default_params: LLMParameters,
        a2a_secret: Vec<u8>,
    ) -> Self {
        Self {
            inference,
            tools,
            default_params,
            a2a_secret: Zeroizing::new(a2a_secret),
            template_renderer: TemplateRenderer::new(std::path::PathBuf::from(
                crate::template_renderer::DEFAULT_TEMPLATE_BASE_PATH,
            )),
            heal_error_cb: None,
            spotlighter: Spotlighter::new(SpotlightMode::Delimit),
            runtime_policy: None,
            taint_labels: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Set the template base path for resolving template_ref values.
    /// Useful for integration tests that need to point to a test fixture directory.
    #[must_use]
    pub fn with_template_base_path(mut self, path: std::path::PathBuf) -> Self {
        self.template_renderer = TemplateRenderer::new(path);
        self
    }

    /// Attach a self-healing callback for automatic error recovery.
    pub fn with_heal_cb(mut self, cb: HealCallback) -> Self {
        self.heal_error_cb = Some(cb);
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
                    let labels = self
                        .taint_labels
                        .lock()
                        .expect("taint_labels mutex poisoned");
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
        let mut labels = self
            .taint_labels
            .lock()
            .expect("taint_labels mutex poisoned");
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

    /// Execute a single KnowAct template — render, infer, parse, return.
    ///
    /// This is the minimal template invocation path: no manifest cascade,
    /// no PDCA loop, no gas/rJoule tracking. Designed for programmatic
    /// invocation by the persona layer (MetacognitionLoop) when it needs
    /// LLM-driven decisions from a KnowAct template.
    ///
    /// `template_ref` is a path relative to `template_base_path`
    /// (e.g. `curator/metacognition-diagnose.j2`).
    /// `context` provides the template variables.
    ///
    /// Returns the parsed JSON response as a `serde_json::Value`,
    /// or a `TemplateError` if rendering, inference, or parsing fails.
    ///
    /// expect: "The system resolves and executes template manifest cascades"
    /// \[P3\] Motivating: Generative Space — single-template KnowAct invocation
    /// pre:  `template_ref` is a valid relative path within `template_base_path`;
    ///       `context` contains the variables referenced by the template.
    /// post: Returns the parsed JSON response from the LLM on success;
    ///       returns `TemplateError` on rendering, inference, or parse failure.
    pub async fn execute_knowact(
        &self,
        template_ref: &str,
        context: &HashMap<String, Value>,
    ) -> Result<Value> {
        let prompt = self.load_template(template_ref, context)?;

        let params = self.default_params.clone();
        const DEFAULT_TIMEOUT_SECS: u64 = 120;
        let timeout_dur = std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS);

        let result: InferenceResult = match tokio::time::timeout(
            timeout_dur,
            self.inference.generate(&prompt, &params, None),
        )
        .await
        {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return Err(TemplateError::Inference(e)),
            Err(_elapsed) => {
                return Err(TemplateError::Manifest(format!(
                    "KnowAct template {} timed out after {}s",
                    template_ref, DEFAULT_TIMEOUT_SECS
                )));
            }
        };

        parse_json_response(&result.text, 0)
    }

    /// Load and render a template, preferring the embedded (build-time)
    /// copy and falling back to the filesystem path. The embedded copy is
    /// authoritative for installed binaries — it works regardless of CWD or
    /// install location. The filesystem fallback exists for dev workflows
    /// where a template has been edited but not yet rebuilt.
    ///
    /// Delegates to `TemplateRenderer` — the resolution ladder lives there.
    fn load_template(
        &self,
        template_ref: &str,
        context: &HashMap<String, Value>,
    ) -> Result<String> {
        let template_content = self.template_renderer.load(template_ref, 0)?;
        let prompt = self.template_renderer.render(&template_content, context)?;
        Ok(prompt)
    }

    /// Invoke an MCP tool directly by server/tool name.
    ///
    /// Creates a delegation token internally. Used by callers that need
    /// to call MCP tools outside of template manifest execution.
    pub async fn call_tool(
        &self,
        tool_ref: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.invoke_tool(tool_ref, input, 0)
            .await
            .map(|(result, _)| result)
    }

    async fn invoke_tool(
        &self,
        tool_name: &str,
        input: Value,
        action_number: u64,
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
                self.check_untrusted_input(&input),
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

        let secret_bytes: [u8; 32] = self.a2a_secret[..32]
            .try_into()
            .map_err(|_| TemplateError::Manifest("A2A secret must be at least 32 bytes".into()))?;
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&secret_bytes);
        let executor_webid = WebID::from_persona(b"manifest-executor");
        let token = DelegationToken::new(
            DelegationResource::Tool,
            tool_name.to_string(),
            DelegationAction::Execute,
            executor_webid,
            executor_webid,
            &signing_key,
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

        // Manifest-level fusion control: when manifest.fusion is Some(config),
        // all steps use this per-manifest fusion config (custom judge/panel/mode).
        // When None, follows the global default (global fusion if configured).
        // Per-step fusion: Some(false) bypasses, Some(true) forces manifest config,
        // None inherits the manifest behavior.
        let manifest_fusion_config = manifest.fusion.clone();

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
                        context = self
                            .execute_select(
                                step,
                                context,
                                &mut budget,
                                manifest_fusion_config.as_ref(),
                            )
                            .await?;
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
                        context = self.execute_tool_invoke(step, context).await?;
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
        manifest_fusion_config: Option<&hkask_types::fusion::FusionConfig>,
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

        let mut params = self.default_params.clone();

        // Resolve per-step fusion override: step.fusion takes priority,
        // then manifest-level config, then the global default.
        // Some(false) -> bypass fusion (single-model, for deterministic rubrics).
        // Some(true) -> force manifest config (or global if manifest has none).
        // None -> inherit: use manifest config if present, else global default.
        match step.fusion {
            Some(false) => {
                params.bypass_fusion = true;
            }
            Some(true) | None => {
                if let Some(config) = manifest_fusion_config {
                    params.fusion_config = Some(config.clone());
                }
            }
        }

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
        if let Some(ref mapping) = step.input_mapping {
            let resolved = bind_parameters(mapping, &context);
            if let Value::Object(map) = resolved {
                for (k, v) in map {
                    context.insert(k, v);
                }
            }
        }

        let populated = self.render_step_template(step, &context)?;
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

        // Load the sub-manifest YAML. Try embedded .yaml first, then embedded
        // .j2 (shouldn't happen for flowdef, but handle gracefully), then
        // filesystem fallback.
        let manifest_yaml = if let Some(content) = crate::template_yaml_file(&template_ref) {
            content.to_string()
        } else if let Some(content) = crate::template_file(&template_ref) {
            content.to_string()
        } else {
            self.template_renderer
                .load_from_disk(&template_ref, step.ordinal)?
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
        for (k, v) in sub_result {
            if parent_keys.contains(&k) {
                parent_context.insert(k, v);
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
        mut context: HashMap<String, Value>,
    ) -> Result<HashMap<String, Value>> {
        let mcp_ref_raw = step.mcp.as_deref().ok_or_else(|| {
            TemplateError::Manifest(format!(
                "Execute step {} has no mcp reference",
                step.ordinal
            ))
        })?;

        // Resolve ${variable} references in the MCP reference against context
        let mcp_ref = TemplateRenderer::render_inline(mcp_ref_raw, &context);

        let input: Value = step
            .input_mapping
            .as_ref()
            .map(|mapping| bind_parameters(mapping, &context))
            .unwrap_or_else(|| {
                Value::Object(
                    context
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                )
            });

        let (result, tool_taint) = self
            .invoke_tool(&mcp_ref, input, context.len() as u64)
            .await?;

        let result_key = format!("step_{}_result", step.ordinal);

        // FIDES taint propagation: if this tool is a Source (returns untrusted
        // data from external sources), mark the result as tainted so downstream
        // Sink tools can detect it via check_untrusted_input.
        // Layer 5 defense (Microsoft Research FIDES arXiv:2505.23643).
        if tool_taint == ToolTaint::Source {
            self.taint_labels
                .lock()
                .expect("taint_labels mutex poisoned")
                .insert(result_key.clone(), ToolTaint::Source);
        }

        context.insert(result_key, result);

        Ok(context)
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
            .map(|mapping| bind_parameters(mapping, &context))
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

/// Dispatch a `compute_ref` string to the matching `hkask_forecast` primitive.
///
/// The `input` JSON object carries the function's arguments, bound from prior
/// step results by `execute_compute`. Returns the function's result as a JSON
/// value consumable by downstream steps.
///
/// Supported `compute_ref` values (must match the conformance contract in
/// `registry/templates/superforecasting/README.md`):
/// - `calibrate_from_fermi` — in: `{questions: [{question, estimate, confidence}, ...]}`
/// - `outside_view_adjustment` — in: `{base_rate, inside_estimate, reference_count}`
/// - `bayesian_update` — in: `{prior, evidence_likelihood, evidence_base_rate}`
/// - `apply_calibration_adjustment` — in: `{prior, overconfidence_bias}`
/// - `brier_score` — in: `{probability, outcome_occurred}`
/// - `brier_score_multi` — in: `{probabilities: [f64], outcomes: [bool]}`
/// - `brier_interpretation` — in: `{score}`
fn dispatch_compute(compute_ref: &str, input: &Value) -> Result<Value> {
    use hkask_forecast as forecast;
    let get_f64 = |key: &str| -> Result<f64> {
        input.get(key).and_then(|v| v.as_f64()).ok_or_else(|| {
            TemplateError::Manifest(format!(
                "compute '{}': missing or non-numeric input '{}'",
                compute_ref, key
            ))
        })
    };
    let get_bool = |key: &str| -> Result<bool> {
        input.get(key).and_then(|v| v.as_bool()).ok_or_else(|| {
            TemplateError::Manifest(format!(
                "compute '{}': missing or non-boolean input '{}'",
                compute_ref, key
            ))
        })
    };
    let get_u64 = |key: &str| -> Result<u64> {
        input.get(key).and_then(|v| v.as_u64()).ok_or_else(|| {
            TemplateError::Manifest(format!(
                "compute '{}': missing or non-integer input '{}'",
                compute_ref, key
            ))
        })
    };

    match compute_ref {
        "calibrate_from_fermi" => {
            let questions = input
                .get("questions")
                .and_then(|v| v.as_array())
                .ok_or_else(|| {
                    TemplateError::Manifest(
                        "compute 'calibrate_from_fermi': missing 'questions' array".into(),
                    )
                })?;
            let fqs: Vec<forecast::FermiQuestion> = questions
                .iter()
                .map(|q| forecast::FermiQuestion {
                    question: q
                        .get("question")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    estimate: q.get("estimate").and_then(|v| v.as_f64()).unwrap_or(0.5),
                    confidence: q.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.5),
                })
                .collect();
            let calibrated = forecast::calibrate_from_fermi(&fqs)
                .map_err(|e| TemplateError::Manifest(format!("calibrate_from_fermi: {e}")))?;
            Ok(serde_json::json!({ "calibrated": calibrated }))
        }
        "outside_view_adjustment" => {
            let base_rate = get_f64("base_rate")?;
            let inside_estimate = get_f64("inside_estimate")?;
            let reference_count = get_u64("reference_count")?;
            let (calibrated, confidence) =
                forecast::outside_view_adjustment(base_rate, inside_estimate, reference_count);
            Ok(serde_json::json!({ "calibrated": calibrated, "confidence": confidence }))
        }
        "bayesian_update" => {
            let prior = get_f64("prior")?;
            let likelihood = get_f64("evidence_likelihood")?;
            let base_rate = get_f64("evidence_base_rate")?;
            let posterior = forecast::bayesian_update(prior, likelihood, base_rate);
            Ok(serde_json::json!({ "posterior": posterior }))
        }
        "apply_calibration_adjustment" => {
            let prior = get_f64("prior")?;
            let bias = get_f64("overconfidence_bias")?;
            let adjusted = forecast::apply_calibration_adjustment(prior, bias);
            Ok(serde_json::json!({ "adjusted": adjusted }))
        }
        "brier_score" => {
            let probability = get_f64("probability")?;
            let occurred = get_bool("outcome_occurred")?;
            let score = forecast::brier_score(probability, occurred);
            Ok(serde_json::json!({ "score": score }))
        }
        "brier_score_multi" => {
            let probabilities = input
                .get("probabilities")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.iter().map(|v| v.as_f64()).collect::<Option<Vec<f64>>>())
                .ok_or_else(|| {
                    TemplateError::Manifest(
                        "compute 'brier_score_multi': missing 'probabilities' f64 array".into(),
                    )
                })?;
            let outcomes = input
                .get("outcomes")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .map(|v| v.as_bool())
                        .collect::<Option<Vec<bool>>>()
                })
                .ok_or_else(|| {
                    TemplateError::Manifest(
                        "compute 'brier_score_multi': missing 'outcomes' bool array".into(),
                    )
                })?;
            let score = forecast::brier_score_multi(&probabilities, &outcomes)
                .map_err(|e| TemplateError::Manifest(format!("brier_score_multi: {e}")))?;
            Ok(serde_json::json!({ "score": score }))
        }
        "brier_interpretation" => {
            let score = get_f64("score")?;
            Ok(serde_json::json!({ "interpretation": forecast::brier_interpretation(score) }))
        }
        // ── Kata convergence primitives ──
        //
        // These implement the Improvement Kata convergence model: the agent has
        // a target condition and a current condition, measured in two orthogonal
        // spaces (Dublin Core object space + PKO process space). The total
        // distance is the hypotenuse of the right triangle formed by the two
        // gaps. Each PDCA cycle produces a prediction (with confidence) and a
        // result; the Brier score tracks prediction calibration.
        //
        // These are deterministic `compute` steps — no inference, no timeout.
        // They replace the old LLM self-grade convergence-check templates that
        // caused the 30s timeouts across 12+ skills.
        //
        // Distance functions start with edge-counting (simplest well-defined
        // measure) and iterate based on Brier feedback. If the Brier score
        // converges, the distance function is good enough; if not, escalate to
        // information-content-weighted measures (Resnik/Lin).

        // Object-space gap (Dublin Core): artifact completeness.
        // Counts missing fields and ungrounded fields in the current artifacts
        // vs the target spec. Normalized to [0, 1].
        "kata.object_gap" => {
            let current = input.get("current_artifacts").ok_or_else(|| {
                TemplateError::Manifest(
                    "compute 'kata.object_gap': missing 'current_artifacts'".into(),
                )
            })?;
            let target = input.get("target_artifacts").ok_or_else(|| {
                TemplateError::Manifest(
                    "compute 'kata.object_gap': missing 'target_artifacts'".into(),
                )
            })?;
            let (gap, missing, ungrounded) = compute_object_gap(current, target);
            Ok(serde_json::json!({
                "object_gap": gap,
                "missing_fields": missing,
                "ungrounded_fields": ungrounded,
            }))
        }
        // Process-space gap (PKO): procedure progress.
        // Counts incomplete steps in the current procedure vs the target spec.
        // Steps in-progress are half-weighted. Normalized to [0, 1].
        "kata.process_gap" => {
            let current = input.get("current_procedure").ok_or_else(|| {
                TemplateError::Manifest(
                    "compute 'kata.process_gap': missing 'current_procedure'".into(),
                )
            })?;
            let target = input.get("target_procedure").ok_or_else(|| {
                TemplateError::Manifest(
                    "compute 'kata.process_gap': missing 'target_procedure'".into(),
                )
            })?;
            let (gap, incomplete) = compute_process_gap(current, target);
            Ok(serde_json::json!({
                "process_gap": gap,
                "incomplete_steps": incomplete,
            }))
        }
        // Hypotenuse: sqrt(object_gap² + process_gap²).
        // The total distance to the target in the combined object-process space.
        "kata.hypotenuse" => {
            let object_gap = get_f64("object_gap")?;
            let process_gap = get_f64("process_gap")?;
            let hypotenuse = (object_gap * object_gap + process_gap * process_gap).sqrt();
            Ok(serde_json::json!({
                "hypotenuse": hypotenuse,
                "object_gap": object_gap,
                "process_gap": process_gap,
            }))
        }
        // Prediction vs result: Brier score for one PDCA cycle.
        // The prediction carries a confidence in [0,1]; the result is whether
        // the predicted outcome occurred (bool) or the actual delta (f64).
        "kata.prediction_vs_result" => {
            let confidence = input
                .get("prediction")
                .and_then(|p| p.get("confidence"))
                .and_then(|v| v.as_f64())
                .ok_or_else(|| {
                    TemplateError::Manifest(
                        "compute 'kata.prediction_vs_result': missing prediction.confidence".into(),
                    )
                })?;
            // The outcome: either a bool (occurred) or a f64 (actual delta
            // normalized to [0,1]).
            let outcome = input
                .get("result")
                .and_then(|r| {
                    r.get("occurred")
                        .and_then(|v| v.as_bool())
                        .map(|b| if b { 1.0 } else { 0.0 })
                        .or_else(|| r.get("actual_delta").and_then(|v| v.as_f64()))
                })
                .ok_or_else(|| {
                    TemplateError::Manifest(
                        "compute 'kata.prediction_vs_result': missing result.occurred or result.actual_delta".into(),
                    )
                })?;
            let brier = (confidence - outcome).powi(2);
            let prediction_error = (confidence - outcome).abs();
            Ok(serde_json::json!({
                "brier": brier,
                "prediction_error": prediction_error,
                "confidence": confidence,
                "outcome": outcome,
            }))
        }
        // Full convergence check: combines hypotenuse and Brier trajectory.
        // Reads the histories from _convergence context (injected by the
        // tracker) and returns the convergence decision.
        "kata.convergence_check" => {
            // Full convergence check: combines gap, Cauchy, and calibration.
            // Reads the histories from _convergence context (injected by the
            // tracker) and returns the convergence decision.
            //
            // Three canonical stop conditions (any active one triggers):
            // 1. Gap: hypotenuse < hypotenuse_epsilon (limit of a sequence)
            // 2. Cauchy: max pairwise delta in cauchy_window < cauchy_epsilon
            //    (iterates stopped moving — learning exhausted)
            // 3. Calibration: rolling Brier < brier_threshold for brier_window
            //    (predictions are calibrated)
            let hypotenuse = get_f64("hypotenuse")?;
            let hypotenuse_epsilon = input
                .get("hypotenuse_epsilon")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.05);
            let cauchy_epsilon = input
                .get("cauchy_epsilon")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.03);
            let cauchy_window = input
                .get("cauchy_window")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as usize;
            let brier_history = input
                .get("brier_history")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.iter().map(|v| v.as_f64()).collect::<Option<Vec<f64>>>())
                .unwrap_or_default();
            let hypotenuse_history = input
                .get("hypotenuse_history")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.iter().map(|v| v.as_f64()).collect::<Option<Vec<f64>>>())
                .unwrap_or_default();
            let brier_threshold = input
                .get("brier_threshold")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.15);
            let brier_window = input
                .get("brier_window")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as usize;
            let mode = input
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("gap_or_cauchy_or_calibration");

            // 1. Gap convergence
            let gap_converged = hypotenuse.is_finite() && hypotenuse < hypotenuse_epsilon;

            // 2. Cauchy convergence: max pairwise delta in window < epsilon
            let cauchy_converged = if hypotenuse_history.len() >= cauchy_window {
                let start = hypotenuse_history.len().saturating_sub(cauchy_window);
                let finite: Vec<f64> = hypotenuse_history[start..]
                    .iter()
                    .copied()
                    .filter(|f| f.is_finite())
                    .collect();
                if finite.len() >= cauchy_window {
                    let mut max_delta = 0.0_f64;
                    for i in 0..finite.len() {
                        for j in (i + 1)..finite.len() {
                            let delta = (finite[i] - finite[j]).abs();
                            if delta > max_delta {
                                max_delta = delta;
                            }
                        }
                    }
                    max_delta < cauchy_epsilon
                } else {
                    false
                }
            } else {
                false
            };

            // 3. Calibration convergence: rolling Brier < threshold
            let calibration_converged = if brier_history.len() >= brier_window {
                let start = brier_history.len().saturating_sub(brier_window);
                let recent: Vec<f64> = brier_history[start..]
                    .iter()
                    .copied()
                    .filter(|f| f.is_finite())
                    .collect();
                if recent.len() >= brier_window {
                    let rolling: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
                    rolling < brier_threshold
                } else {
                    false
                }
            } else {
                false
            };

            let (converged, conv_mode, reason) = match mode {
                "gap" => (
                    gap_converged,
                    if gap_converged { "gap" } else { "none" },
                    if gap_converged {
                        format!("gap {hypotenuse:.4} < epsilon {hypotenuse_epsilon:.4}")
                    } else {
                        format!("gap {hypotenuse:.4} >= epsilon {hypotenuse_epsilon:.4}")
                    },
                ),
                "cauchy" => (
                    cauchy_converged,
                    if cauchy_converged { "cauchy" } else { "none" },
                    if cauchy_converged {
                        "iterates stabilized (Cauchy criterion met)".to_string()
                    } else {
                        "iterates not yet stabilized".to_string()
                    },
                ),
                "calibration" => (
                    calibration_converged,
                    if calibration_converged {
                        "calibration"
                    } else {
                        "none"
                    },
                    if calibration_converged {
                        "Brier score calibrated".to_string()
                    } else {
                        "Brier score not yet calibrated".to_string()
                    },
                ),
                "gap_or_cauchy" => {
                    if gap_converged {
                        (
                            true,
                            "gap",
                            format!("gap {hypotenuse:.4} < epsilon {hypotenuse_epsilon:.4}"),
                        )
                    } else if cauchy_converged {
                        (
                            true,
                            "cauchy",
                            "iterates stabilized (Cauchy criterion met)".to_string(),
                        )
                    } else {
                        (
                            false,
                            "none",
                            format!("gap {hypotenuse:.4} >= epsilon, not Cauchy"),
                        )
                    }
                }
                "gap_or_calibration" => {
                    if gap_converged {
                        (
                            true,
                            "gap",
                            format!("gap {hypotenuse:.4} < epsilon {hypotenuse_epsilon:.4}"),
                        )
                    } else if calibration_converged {
                        (true, "calibration", "Brier score calibrated".to_string())
                    } else {
                        (
                            false,
                            "none",
                            format!("gap {hypotenuse:.4} >= epsilon, Brier not calibrated"),
                        )
                    }
                }
                "cauchy_or_calibration" => {
                    if cauchy_converged {
                        (
                            true,
                            "cauchy",
                            "iterates stabilized (Cauchy criterion met)".to_string(),
                        )
                    } else if calibration_converged {
                        (true, "calibration", "Brier score calibrated".to_string())
                    } else {
                        (
                            false,
                            "none",
                            "not Cauchy, Brier not calibrated".to_string(),
                        )
                    }
                }
                _ => {
                    // gap_or_cauchy_or_calibration (default)
                    if gap_converged {
                        (
                            true,
                            "gap",
                            format!("gap {hypotenuse:.4} < epsilon {hypotenuse_epsilon:.4}"),
                        )
                    } else if cauchy_converged {
                        (
                            true,
                            "cauchy",
                            "iterates stabilized (Cauchy criterion met)".to_string(),
                        )
                    } else if calibration_converged {
                        (true, "calibration", "Brier score calibrated".to_string())
                    } else {
                        (
                            false,
                            "none",
                            format!(
                                "gap {hypotenuse:.4} >= epsilon, not Cauchy, Brier not calibrated"
                            ),
                        )
                    }
                }
            };

            Ok(serde_json::json!({
                "converged": converged,
                "mode": conv_mode,
                "reason": reason,
                "hypotenuse": hypotenuse,
                "gap_converged": gap_converged,
                "cauchy_converged": cauchy_converged,
                "calibration_converged": calibration_converged,
            }))
        }
        // ── Lisp evaluation primitive ──
        //
        // Deterministic evaluation of a Lisp form against a JSON environment.
        // No LLM round-trip, no I/O, no filesystem, no network. Bounded
        // recursion depth (64) and bounded evaluation steps (100000).
        // Used for recursive predicates over the context map — e.g.
        // capability-tree walks, structural invariant checks, falsifiability
        // counterfactuals that the LLM cannot reliably evaluate itself.
        //
        // Security: the interpreter has no `eval` builtin (Lisp code cannot
        // evaluate arbitrary strings), no `load`/`require`, and the
        // environment is immutable from Lisp's perspective. The caller must
        // gate `lisp.eval` to `category: skill` manifests only — infrastructure
        // manifests run without human review and a Turing-complete step
        // language is an attack surface (see .rules trap on manifests).
        "lisp.eval" => {
            let form = input.get("form").and_then(|v| v.as_str()).ok_or_else(|| {
                TemplateError::Manifest("compute 'lisp.eval': missing 'form' string".into())
            })?;
            let env_input = input.get("env").cloned().unwrap_or(Value::Null);
            let max_steps = input
                .get("max_steps")
                .and_then(|v| v.as_u64())
                .unwrap_or(100000);
            let max_depth = input
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(64);
            let result =
                hkask_lisp::eval_sandboxed_with_budget(form, &env_input, max_steps, max_depth)
                    .map_err(|e| TemplateError::Manifest(format!("lisp.eval: {e}")))?;
            Ok(result)
        }
        other => Err(TemplateError::Manifest(format!(
            "Unknown compute_ref: '{}'. Supported: calibrate_from_fermi, outside_view_adjustment, bayesian_update, apply_calibration_adjustment, brier_score, brier_score_multi, brier_interpretation, kata.object_gap, kata.process_gap, kata.hypotenuse, kata.prediction_vs_result, kata.convergence_check, lisp.eval",
            other
        ))),
    }
}

/// Compute the object-space gap (Dublin Core artifact completeness).
///
/// Edge-counting distance: counts fields present in the target spec but
/// missing from the current artifacts (weight 1.0 each), plus fields that are
/// present but ungrounded (weight 0.5 each — an ungrounded field is halfway
/// between missing and complete). Normalized to [0, 1] by dividing by the
/// total field count in the target spec.
///
/// This is the simplest well-defined distance measure for object space.
/// If Brier scores don't converge with this measure, escalate to
/// information-content-weighted measures (Resnik/Lin).
fn compute_object_gap(
    current: &serde_json::Value,
    target: &serde_json::Value,
) -> (f64, Vec<String>, Vec<String>) {
    let target_fields = collect_field_keys(target);
    let mut missing: Vec<String> = Vec::new();
    let mut ungrounded: Vec<String> = Vec::new();
    let total = target_fields.len().max(1) as f64;

    for field in &target_fields {
        match current.get(field) {
            None | Some(serde_json::Value::Null) => {
                missing.push(field.clone());
            }
            Some(val) if is_ungrounded(val) => {
                ungrounded.push(field.clone());
            }
            Some(_) => { /* complete */ }
        }
    }

    let gap = (missing.len() as f64 + 0.5 * ungrounded.len() as f64) / total;
    (gap.min(1.0), missing, ungrounded)
}

/// A field value is "ungrounded" if it's an empty string, empty array, empty
/// object, or a string that looks like a placeholder ("TODO", "TBD", "?").
fn is_ungrounded(val: &serde_json::Value) -> bool {
    match val {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            trimmed.is_empty()
                || matches!(
                    trimmed.to_lowercase().as_str(),
                    "todo" | "tbd" | "?" | "n/a" | "placeholder"
                )
        }
        serde_json::Value::Array(arr) => arr.is_empty(),
        serde_json::Value::Object(obj) => obj.is_empty(),
        _ => false,
    }
}

/// Compute the process-space gap (PKO procedure progress).
///
/// Edge-counting distance: counts steps in the target procedure that are not
/// yet complete in the current procedure. Steps that are "in_progress" are
/// half-weighted (halfway between not-started and complete). Normalized to
/// [0, 1] by dividing by the total step count.
///
/// The procedure is represented as an array of step objects, each with a
/// `status` field: "complete", "in_progress", "not_started" (or missing).
fn compute_process_gap(
    current: &serde_json::Value,
    target: &serde_json::Value,
) -> (f64, Vec<String>) {
    let target_steps = target
        .get("steps")
        .and_then(|v| v.as_array())
        .or_else(|| target.as_array())
        .cloned()
        .unwrap_or_default();
    let current_steps = current
        .get("steps")
        .and_then(|v| v.as_array())
        .or_else(|| current.as_array())
        .cloned()
        .unwrap_or_default();

    let total = target_steps.len().max(1) as f64;
    let mut incomplete: Vec<String> = Vec::new();
    let mut weighted_incomplete = 0.0_f64;

    for (i, target_step) in target_steps.iter().enumerate() {
        let step_name = target_step
            .get("name")
            .or_else(|| target_step.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unnamed")
            .to_string();
        let current_status = current_steps
            .get(i)
            .and_then(|s| s.get("status"))
            .and_then(|v| v.as_str())
            .unwrap_or("not_started");
        match current_status {
            "complete" => { /* done */ }
            "in_progress" => {
                weighted_incomplete += 0.5;
                incomplete.push(format!("{step_name} (in_progress)"));
            }
            _ => {
                weighted_incomplete += 1.0;
                incomplete.push(format!("{step_name} (not_started)"));
            }
        }
    }

    let gap = weighted_incomplete / total;
    (gap.min(1.0), incomplete)
}

/// Collect the top-level keys from a JSON object (for object-gap field
/// comparison). If the value is an array, collects the `name` or `id` field
/// from each element.
fn collect_field_keys(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Object(map) => map.keys().cloned().collect(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .enumerate()
            .map(|(i, item)| {
                item.get("name")
                    .or_else(|| item.get("id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or(format!("item_{i}"))
            })
            .collect(),
        _ => Vec::new(),
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
    context
        .iter()
        .filter_map(|(key, value)| {
            key.strip_prefix("step_")
                .and_then(|rest| rest.strip_suffix("_result"))
                .and_then(|n| n.parse::<u32>().ok())
                .map(|ordinal| (ordinal, value))
        })
        .max_by_key(|(ordinal, _)| *ordinal)
        .map(|(_, v)| v.clone())
        .unwrap_or(Value::Null)
}

/// Parse a JSON response from an inference call.
///
/// Attempts to extract JSON from the response text, handling cases where
/// the model wraps the JSON in markdown code fences.
fn parse_json_response(text: &str, step_ordinal: u32) -> Result<Value> {
    if let Ok(v) = serde_json::from_str(text) {
        return Ok(v);
    }
    let trimmed = text.trim();
    if let Some(json_start) = trimmed.find("```json") {
        let after_fence = &trimmed[json_start + 7..];
        if let Some(json_end) = after_fence.find("```") {
            return serde_json::from_str(after_fence[..json_end].trim()).map_err(|e| {
                TemplateError::Manifest(format!(
                    "Step {}: Failed to parse JSON response: {}",
                    step_ordinal, e
                ))
            });
        }
    }
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        return serde_json::from_str(&trimmed[start..=end]).map_err(|e| {
            TemplateError::Manifest(format!(
                "Step {}: Failed to parse JSON response: {}",
                step_ordinal, e
            ))
        });
    }
    Err(TemplateError::Manifest(format!(
        "Step {}: No JSON found in inference response",
        step_ordinal
    )))
}

/// Extract the `contract.output` block from a `.j2` template's frontmatter.
///
/// The frontmatter is YAML between the start of the file and the `---`
/// separator. The `contract.output` block declares field names and their
/// types as a simple `name: type` mapping (e.g. `convergence_metric: number`).
/// This function parses that block and returns it as a `serde_json::Value`
/// map (field name → type string), or `None` if no contract is found.
///
/// This is the schema source for structured-output tool calling — the
/// executor converts this into a JSON Schema and passes it as a synthetic
/// tool so the model is forced to emit JSON conforming to the contract,
/// instead of emitting prose and hoping `parse_json_response` can extract
/// JSON from it.
fn extract_contract_output(template_content: &str) -> Option<Value> {
    // hKask templates use a frontmatter format where:
    // - The frontmatter starts at the beginning of the file (optionally after
    //   leading Jinja comments `{# ... #}` and a `[inference]` marker line)
    //   and ends at the first `---` separator.
    // - The frontmatter is YAML containing `template_type`,
    //   `contract`, `energy_cap`, `visibility`, etc.
    // - The body after `---` is the Jinja2 template.
    //
    // We find the `\n---\n` separator and parse everything before it as YAML.
    // Leading Jinja comments (`{# ... #}`) are stripped — they're not valid YAML
    // and would cause the parser to fail. The `[inference]` marker is also
    // stripped for the same reason.
    let separator_pos = template_content.find("\n---\n")?;
    let frontmatter = &template_content[..separator_pos];

    // Strip Jinja comments ({# ... #}) — they can appear anywhere in the
    // frontmatter and are not valid YAML.
    let stripped = strip_jinja_comments(frontmatter);
    let frontmatter = stripped.trim();
    let frontmatter = frontmatter
        .strip_prefix("[inference]")
        .unwrap_or(frontmatter)
        .trim();

    let parsed: Value = serde_yaml_neo::from_str(frontmatter).ok()?;
    let contract = parsed.get("contract")?;
    let output = contract.get("output")?;
    Some(output.clone())
}

/// Strip Jinja comments (`{# ... #}`) from a string. Comments can span
/// multiple lines. Uses a simple state machine rather than regex to avoid
/// the regex dependency.
fn strip_jinja_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'#') {
            // Skip until we find #}
            chars.next(); // consume '#'
            let mut found_close = false;
            while let Some(c) = chars.next() {
                if c == '#' && chars.peek() == Some(&'}') {
                    chars.next(); // consume '}'
                    found_close = true;
                    break;
                }
            }
            if !found_close {
                // Unterminated comment — append the rest as-is
                result.push('{');
                result.push('#');
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Convert a `contract.output` block (field name → type string) into a
/// JSON Schema suitable for tool-calling.
///
/// The contract output is a simple mapping like:
/// ```yaml
/// output:
///   convergence_metric: number
///   rationale: string
///   blockers: array
/// ```
///
/// This converts to a JSON Schema object with `type: object`, `properties`
/// mapping each field to its JSON type, and no `required` fields (the model
/// can omit optional fields). The type mapping is:
/// - `string` → `{"type": "string"}`
/// - `number` / `float` / `integer` → `{"type": "number"}`
/// - `boolean` → `{"type": "boolean"}`
/// - `array` → `{"type": "array"}`
/// - `object` → `{"type": "object"}`
/// - any other type → `{"type": "string"}` (safe default)
///
/// If the contract output is already a JSON Schema (has `type` or `properties`
/// at the top level), it's returned as-is.
fn contract_output_to_schema(output: &Value) -> Value {
    // If it's already a JSON Schema object, return as-is.
    if output.is_object() && (output.get("type").is_some() || output.get("properties").is_some()) {
        return output.clone();
    }

    // Otherwise, it's a field-name → type-string mapping.
    let Some(fields) = output.as_object() else {
        return output.clone();
    };

    let mut properties = serde_json::Map::new();
    for (field_name, field_type) in fields {
        let type_str = field_type.as_str().unwrap_or("string");
        let json_type = match type_str {
            "string" | "str" => "string",
            "number" | "float" | "double" => "number",
            "integer" | "int" | "i32" | "i64" | "u32" | "u64" => "number",
            "boolean" | "bool" => "boolean",
            "array" => "array",
            "object" => "object",
            _ => "string", // safe default for unknown types
        };
        properties.insert(field_name.clone(), serde_json::json!({"type": json_type}));
    }

    serde_json::json!({
        "type": "object",
        "properties": properties,
    })
}

/// Build a synthetic `ChatToolDefinition` for structured output.
///
/// The tool is named `emit_result` and its parameters are the JSON Schema
/// derived from the contract output. When passed to the inference call,
/// the model is forced to call this tool (emitting JSON conforming to the
/// schema) instead of emitting free-text prose. The executor then extracts
/// the result from `InferenceResult.tool_calls[0].args`.
///
/// This is the LangGraph/Swarm pattern: enforce the output contract at the
/// inference API layer, not the prompt layer. The model physically cannot
/// emit prose when a tool is the only allowed response format.
fn build_structured_output_tool(schema: Value) -> ChatToolDefinition {
    ChatToolDefinition {
        tool_type: "function".to_string(),
        function: ChatToolFunction {
            name: "emit_result".to_string(),
            description: "Emit the structured result for this step. Call this tool with the JSON object matching the schema.".to_string(),
            parameters: schema,
        },
    }
}

/// Resolve the output schema for a `select` step.
///
/// Priority:
/// 1. `step.output_schema` (manifest-declared, if present)
/// 2. `contract.output` from the template frontmatter (parsed at runtime)
///
/// Returns a JSON Schema suitable for tool-calling, or `None` if no schema
/// is available (in which case the executor falls back to text parsing).
fn resolve_output_schema(step: &BundleManifestStep, template_content: &str) -> Option<Value> {
    // Priority 1: manifest-declared output_schema.
    if let Some(ref schema) = step.output_schema
        && schema.is_object()
    {
        return Some(schema.clone());
    }

    // Priority 2: contract.output from the template frontmatter.
    let contract_output = extract_contract_output(template_content)?;
    Some(contract_output_to_schema(&contract_output))
}

/// Bind parameters from an input mapping to values from the context.
///
/// The input mapping is a JSON object where values are either:
/// - Direct values (strings, numbers, etc.)
/// - Context references: {"$ref": "step_1_result.field"}
fn bind_parameters(mapping: &Value, context: &HashMap<String, Value>) -> Value {
    match mapping {
        Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (key, value) in map {
                let bound = bind_single_parameter(value, context);
                result.insert(key.clone(), bound);
            }
            Value::Object(result)
        }
        other => other.clone(),
    }
}

/// Bind a single parameter value from the context.
fn bind_single_parameter(value: &Value, context: &HashMap<String, Value>) -> Value {
    match value {
        Value::Object(map) => {
            // Check for context reference: {"$ref": "variable_name"}
            if let Some(Value::String(ref_path)) = map.get("$ref") {
                if let Some(context_val) = context.get(ref_path.as_str()) {
                    return context_val.clone();
                }
                // Fallback: try dot notation
                if let Some(nested) = resolve_dot_path(ref_path, context) {
                    return nested;
                }
            }
            // Not a reference — recurse
            bind_parameters(value, context)
        }
        other => other.clone(),
    }
}

/// Resolve a dot-path like "step_1_result.field" from the context.
fn resolve_dot_path(path: &str, context: &HashMap<String, Value>) -> Option<Value> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return None;
    }

    let first = context.get(parts[0])?.clone();
    let mut current = first;
    for part in &parts[1..] {
        match current {
            Value::Object(map) => {
                current = map.get(*part)?.clone();
            }
            _ => return None,
        }
    }
    Some(current)
}

/// Resolve an input_mapping value into a concrete JSON value for template binding.
///
/// Handles three forms used in manifests:
/// - `{{ expr }}` string → rendered through minijinja with `| tojson` and parsed back
///   to a JSON value (so `{{ tasks }}` in a template receives the real array/object,
///   not a stringified repr that would double-encode under `| tojson`).
/// - `{"$ref": "dot.path"}` object → the referenced context value (populate-style).
/// - literal (string/number/bool/array) → as-is, recursing into containers.
fn resolve_mapping_value(
    value: &Value,
    context: &HashMap<String, Value>,
    base: &std::path::Path,
) -> Value {
    match value {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
                let inner = trimmed[2..trimmed.len() - 2].trim();
                let wrapped = format!("{{{{ ({inner}) | tojson }}}}");
                match render_minijinja(&wrapped, context, base) {
                    Ok(json_str) => {
                        serde_json::from_str(json_str.trim()).unwrap_or_else(|_| value.clone())
                    }
                    Err(_) => value.clone(),
                }
            } else if trimmed.contains("{{") {
                render_minijinja(s, context, base)
                    .map(Value::String)
                    .unwrap_or_else(|_| value.clone())
            } else {
                value.clone()
            }
        }
        Value::Object(map) => {
            if let Some(Value::String(ref_path)) = map.get("$ref") {
                if let Some(v) = context.get(ref_path.as_str()) {
                    return v.clone();
                }
                if let Some(v) = resolve_dot_path(ref_path, context) {
                    return v;
                }
            }
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), resolve_mapping_value(v, context, base));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| resolve_mapping_value(v, context, base))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Evaluate a step condition expression against the context.
/// Supported: "var_name" (truthy), "NOT var_name" (falsy),
/// "a AND b" (both truthy), "a OR b" (either truthy).
fn evaluate_step_condition(condition: &str, context: &HashMap<String, Value>) -> bool {
    let condition = condition.trim();

    // Check for boolean operators
    if let Some(pos) = condition.find(" AND ") {
        let left = &condition[..pos].trim();
        let right = &condition[pos + 5..].trim();
        return evaluate_step_condition(left, context) && evaluate_step_condition(right, context);
    }
    if let Some(pos) = condition.find(" OR ") {
        let left = &condition[..pos].trim();
        let right = &condition[pos + 4..].trim();
        return evaluate_step_condition(left, context) || evaluate_step_condition(right, context);
    }

    // Check for negation
    if let Some(inner) = condition.strip_prefix("NOT ") {
        return !evaluate_step_condition(inner.trim(), context);
    }

    // Comparison: <lhs> <op> <rhs>  (e.g. step_1_result.mode == 'plussing', count > 0)
    if let Some((lhs, op, rhs)) = parse_step_comparison(condition) {
        return eval_step_comparison(lhs, op, rhs, context);
    }

    // Simple variable check: is it truthy in context?
    // Also resolve dot-paths like "step_1_result.intervention_needed"
    let key = condition;
    let resolved = resolve_dot_path(key, context);
    let val: Option<&Value> = context.get(key).or(resolved.as_ref());
    match val {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Some(Value::String(s)) => !s.is_empty() && s != "false" && s != "0",
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Null) => false,
        None => false,
    }
}

/// Parse a leaf comparison expression into (lhs, operator, rhs).
/// Operators: <=, >=, ==, !=, <, > (two-char checked before one-char to avoid
/// prefix collisions). Returns None if no operator is present.
fn parse_step_comparison(condition: &str) -> Option<(&str, &str, &str)> {
    let c = condition.trim();
    for op in &["<=", ">=", "==", "!=", "<", ">"] {
        if let Some(pos) = c.find(op) {
            let lhs = c[..pos].trim();
            let rhs = c[pos + op.len()..].trim();
            if lhs.is_empty() || rhs.is_empty() {
                continue;
            }
            return Some((lhs, op, rhs));
        }
    }
    None
}

/// Resolve an operand to a JSON value: a quoted literal, a context dot-path/key,
/// a number literal, or a bare-word string literal.
fn resolve_operand(s: &str, context: &HashMap<String, Value>) -> Option<Value> {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')))
    {
        return Some(Value::String(s[1..s.len() - 1].to_string()));
    }
    if let Some(v) = context.get(s) {
        return Some(v.clone());
    }
    if let Some(v) = resolve_dot_path(s, context) {
        return Some(v);
    }
    if let Ok(n) = s.parse::<f64>() {
        return Some(serde_json::json!(n));
    }
    // SMELL 10 fix: log when an operand is not found in context — this makes a
    // silently-false condition (e.g. step_1_result.mode == 'plussing' where
    // step_1_result.mode is missing) observable for debugging.
    warn!(
        target: "reg.skill.cascade.step_executed",
        operand = s,
        "condition operand not found in context; treating as literal string"
    );
    Some(Value::String(s.to_string()))
}

/// Evaluate a leaf comparison. Numeric for ordering ops; structural (==/!=) for
/// equality. Falls back to string ordering for non-numeric <, <=, >, >=.
fn eval_step_comparison(lhs: &str, op: &str, rhs: &str, context: &HashMap<String, Value>) -> bool {
    let l = match resolve_operand(lhs, context) {
        Some(v) => v,
        None => return false,
    };
    let r = match resolve_operand(rhs, context) {
        Some(v) => v,
        None => return false,
    };
    match op {
        "==" => l == r,
        "!=" => l != r,
        "<" | "<=" | ">" | ">=" => match (l.as_f64(), r.as_f64()) {
            (Some(a), Some(b)) => match op {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                _ => a >= b,
            },
            _ => {
                let ls = l
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| l.to_string());
                let rs = r
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| r.to_string());
                match op {
                    "<" => ls < rs,
                    "<=" => ls <= rs,
                    ">" => ls > rs,
                    _ => ls >= rs,
                }
            }
        },
        _ => false,
    }
}

/// Parse a simple choice condition string like "composite < 0.15" or "findings == 0".
/// Returns `Some((field, operator, value))` or `None` if unparseable.
fn parse_choice_condition(condition: &str) -> Option<(&str, &str, &str)> {
    let condition = condition.trim();
    for op in &["<=", ">=", "==", "<", ">"] {
        if let Some(pos) = condition.find(op) {
            let field = condition[..pos].trim();
            let value = condition[pos + op.len()..].trim();
            if !field.is_empty() && !value.is_empty() {
                return Some((field, *op, value));
            }
        }
    }
    None
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

    #[test]
    fn dispatch_calibrate_from_fermi() {
        let input = serde_json::json!({
            "questions": [
                {"question": "a", "estimate": 0.8, "confidence": 0.9},
                {"question": "b", "estimate": 0.2, "confidence": 0.1}
            ]
        });
        let result = dispatch_compute("calibrate_from_fermi", &input).unwrap();
        let calibrated = result.get("calibrated").and_then(|v| v.as_f64()).unwrap();
        assert!((calibrated - 0.74).abs() < 0.01, "weighted average = 0.74");
    }

    #[test]
    fn dispatch_outside_view_adjustment() {
        let input = serde_json::json!({
            "base_rate": 0.7, "inside_estimate": 0.3, "reference_count": 1000
        });
        let result = dispatch_compute("outside_view_adjustment", &input).unwrap();
        let calibrated = result.get("calibrated").and_then(|v| v.as_f64()).unwrap();
        assert!(calibrated > 0.6, "high reference count trusts base rate");
    }

    #[test]
    fn dispatch_bayesian_update() {
        let input = serde_json::json!({
            "prior": 0.3, "evidence_likelihood": 0.9, "evidence_base_rate": 0.3
        });
        let result = dispatch_compute("bayesian_update", &input).unwrap();
        let posterior = result.get("posterior").and_then(|v| v.as_f64()).unwrap();
        assert!((posterior - 0.9).abs() < 0.01, "Bayesian update = 0.9");
    }

    #[test]
    fn dispatch_apply_calibration_adjustment() {
        let input = serde_json::json!({ "prior": 0.9, "overconfidence_bias": 0.3 });
        let result = dispatch_compute("apply_calibration_adjustment", &input).unwrap();
        let adjusted = result.get("adjusted").and_then(|v| v.as_f64()).unwrap();
        assert!(
            adjusted < 0.9 && adjusted > 0.5,
            "overconfident regresses toward 0.5"
        );
    }

    #[test]
    fn dispatch_brier_score() {
        let input = serde_json::json!({ "probability": 1.0, "outcome_occurred": true });
        let result = dispatch_compute("brier_score", &input).unwrap();
        let score = result.get("score").and_then(|v| v.as_f64()).unwrap();
        assert!((score - 0.0).abs() < 1e-9, "perfect forecast = 0 Brier");
    }

    #[test]
    fn dispatch_unknown_ref_errors() {
        let input = serde_json::json!({});
        assert!(dispatch_compute("nonexistent_fn", &input).is_err());
    }

    #[test]
    fn dispatch_lisp_eval_basic() {
        let input = serde_json::json!({
            "form": "(+ 1 2 3)"
        });
        let result = dispatch_compute("lisp.eval", &input).unwrap();
        assert_eq!(result, serde_json::json!(6));
    }

    #[test]
    fn dispatch_lisp_eval_with_env() {
        let input = serde_json::json!({
            "form": "(assoc \"score\" step_1_result)",
            "env": {
                "step_1_result": {"score": 0.85, "findings": ["a", "b"]}
            }
        });
        let result = dispatch_compute("lisp.eval", &input).unwrap();
        assert_eq!(result, serde_json::json!(0.85));
    }

    #[test]
    fn dispatch_lisp_eval_predicate() {
        let input = serde_json::json!({
            "form": "(and (> (length findings) 0) (< composite 0.15))",
            "env": {
                "findings": ["a", "b"],
                "composite": 0.12
            }
        });
        let result = dispatch_compute("lisp.eval", &input).unwrap();
        assert_eq!(result, serde_json::json!(true));
    }

    #[test]
    fn dispatch_lisp_eval_missing_form_errors() {
        let input = serde_json::json!({"env": {}});
        assert!(dispatch_compute("lisp.eval", &input).is_err());
    }

    #[test]
    fn dispatch_lisp_eval_step_limit() {
        let input = serde_json::json!({
            "form": "(begin (define loop (lambda () (loop))) (loop))",
            "max_steps": 100,
            "max_depth": 1000
        });
        assert!(dispatch_compute("lisp.eval", &input).is_err());
    }

    #[test]
    fn dispatch_kata_object_gap_complete() {
        let input = serde_json::json!({
            "current_artifacts": {"title": "My Plan", "obstacles": ["a", "b"], "assessment": "grounded"},
            "target_artifacts": {"title": "", "obstacles": [], "assessment": ""}
        });
        let result = dispatch_compute("kata.object_gap", &input).unwrap();
        let gap = result.get("object_gap").and_then(|v| v.as_f64()).unwrap();
        assert!((gap - 0.0).abs() < 1e-9, "all fields present = gap 0");
    }

    #[test]
    fn dispatch_kata_object_gap_missing_fields() {
        let input = serde_json::json!({
            "current_artifacts": {"title": "My Plan"},
            "target_artifacts": {"title": "", "obstacles": [], "assessment": "", "prediction": ""}
        });
        let result = dispatch_compute("kata.object_gap", &input).unwrap();
        let gap = result.get("object_gap").and_then(|v| v.as_f64()).unwrap();
        // 3 missing out of 4 = 0.75
        assert!(
            (gap - 0.75).abs() < 1e-9,
            "3/4 missing = gap 0.75, got {gap}"
        );
    }

    #[test]
    fn dispatch_kata_object_gap_ungrounded_half_weighted() {
        let input = serde_json::json!({
            "current_artifacts": {"title": "My Plan", "obstacles": [], "assessment": "TODO"},
            "target_artifacts": {"title": "", "obstacles": [], "assessment": ""}
        });
        let result = dispatch_compute("kata.object_gap", &input).unwrap();
        let gap = result.get("object_gap").and_then(|v| v.as_f64()).unwrap();
        // 1 ungrounded (obstacles empty) at 0.5 + 1 ungrounded (assessment=TODO) at 0.5 = 1.0 / 3
        assert!(
            (gap - (1.0 / 3.0)).abs() < 1e-9,
            "2 ungrounded at 0.5 each = 1.0/3, got {gap}"
        );
    }

    #[test]
    fn dispatch_kata_process_gap_all_complete() {
        let input = serde_json::json!({
            "current_procedure": {"steps": [
                {"name": "grasp", "status": "complete"},
                {"name": "target", "status": "complete"},
                {"name": "experiment", "status": "complete"}
            ]},
            "target_procedure": {"steps": [
                {"name": "grasp"},
                {"name": "target"},
                {"name": "experiment"}
            ]}
        });
        let result = dispatch_compute("kata.process_gap", &input).unwrap();
        let gap = result.get("process_gap").and_then(|v| v.as_f64()).unwrap();
        assert!((gap - 0.0).abs() < 1e-9, "all complete = gap 0");
    }

    #[test]
    fn dispatch_kata_process_gap_mixed() {
        let input = serde_json::json!({
            "current_procedure": {"steps": [
                {"name": "grasp", "status": "complete"},
                {"name": "target", "status": "in_progress"},
                {"name": "experiment", "status": "not_started"}
            ]},
            "target_procedure": {"steps": [
                {"name": "grasp"},
                {"name": "target"},
                {"name": "experiment"}
            ]}
        });
        let result = dispatch_compute("kata.process_gap", &input).unwrap();
        let gap = result.get("process_gap").and_then(|v| v.as_f64()).unwrap();
        // 1 complete (0) + 1 in_progress (0.5) + 1 not_started (1.0) = 1.5 / 3 = 0.5
        assert!((gap - 0.5).abs() < 1e-9, "mixed = gap 0.5, got {gap}");
    }

    #[test]
    fn dispatch_kata_hypotenuse() {
        let input = serde_json::json!({ "object_gap": 0.3, "process_gap": 0.4 });
        let result = dispatch_compute("kata.hypotenuse", &input).unwrap();
        let h = result.get("hypotenuse").and_then(|v| v.as_f64()).unwrap();
        assert!((h - 0.5).abs() < 1e-9, "sqrt(0.09 + 0.16) = 0.5, got {h}");
    }

    #[test]
    fn dispatch_kata_prediction_vs_result_correct() {
        let input = serde_json::json!({
            "prediction": {"confidence": 0.9},
            "result": {"occurred": true}
        });
        let result = dispatch_compute("kata.prediction_vs_result", &input).unwrap();
        let brier = result.get("brier").and_then(|v| v.as_f64()).unwrap();
        assert!(
            (brier - 0.01).abs() < 1e-9,
            "(0.9-1.0)^2 = 0.01, got {brier}"
        );
    }

    #[test]
    fn dispatch_kata_prediction_vs_result_wrong() {
        let input = serde_json::json!({
            "prediction": {"confidence": 0.9},
            "result": {"occurred": false}
        });
        let result = dispatch_compute("kata.prediction_vs_result", &input).unwrap();
        let brier = result.get("brier").and_then(|v| v.as_f64()).unwrap();
        assert!(
            (brier - 0.81).abs() < 1e-9,
            "(0.9-0.0)^2 = 0.81, got {brier}"
        );
    }

    #[test]
    fn dispatch_kata_convergence_check_gap_converged() {
        let input = serde_json::json!({
            "hypotenuse": 0.02,
            "hypotenuse_epsilon": 0.05,
            "cauchy_epsilon": 0.03,
            "cauchy_window": 3,
            "brier_history": [0.5],
            "hypotenuse_history": [0.5, 0.02],
            "brier_threshold": 0.15,
            "brier_window": 3,
            "mode": "gap_or_cauchy_or_calibration"
        });
        let result = dispatch_compute("kata.convergence_check", &input).unwrap();
        assert!(result.get("converged").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(result.get("mode").and_then(|v| v.as_str()).unwrap(), "gap");
    }

    #[test]
    fn dispatch_kata_convergence_check_cauchy_converged() {
        let input = serde_json::json!({
            "hypotenuse": 0.30,
            "hypotenuse_epsilon": 0.05,
            "cauchy_epsilon": 0.03,
            "cauchy_window": 3,
            "brier_history": [0.5],
            "hypotenuse_history": [0.30, 0.31, 0.30],
            "brier_threshold": 0.15,
            "brier_window": 3,
            "mode": "gap_or_cauchy_or_calibration"
        });
        let result = dispatch_compute("kata.convergence_check", &input).unwrap();
        assert!(result.get("converged").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(
            result.get("mode").and_then(|v| v.as_str()).unwrap(),
            "cauchy"
        );
    }

    #[test]
    fn dispatch_kata_convergence_check_calibration_converged() {
        let input = serde_json::json!({
            "hypotenuse": 0.30,
            "hypotenuse_epsilon": 0.05,
            "cauchy_epsilon": 0.03,
            "cauchy_window": 3,
            "brier_history": [0.05, 0.05, 0.05],
            "hypotenuse_history": [0.50, 0.30, 0.10],
            "brier_threshold": 0.15,
            "brier_window": 3,
            "mode": "gap_or_cauchy_or_calibration"
        });
        let result = dispatch_compute("kata.convergence_check", &input).unwrap();
        assert!(result.get("converged").and_then(|v| v.as_bool()).unwrap());
        assert_eq!(
            result.get("mode").and_then(|v| v.as_str()).unwrap(),
            "calibration"
        );
    }

    #[test]
    fn dispatch_kata_convergence_check_not_converged() {
        let input = serde_json::json!({
            "hypotenuse": 0.3,
            "hypotenuse_epsilon": 0.05,
            "cauchy_epsilon": 0.03,
            "cauchy_window": 3,
            "brier_history": [0.5, 0.5],
            "hypotenuse_history": [0.5, 0.3],
            "brier_threshold": 0.15,
            "brier_window": 3,
            "mode": "gap_or_cauchy_or_calibration"
        });
        let result = dispatch_compute("kata.convergence_check", &input).unwrap();
        assert!(!result.get("converged").and_then(|v| v.as_bool()).unwrap());
    }

    #[test]
    fn dispatch_missing_input_errors() {
        let input = serde_json::json!({});
        assert!(
            dispatch_compute("bayesian_update", &input).is_err(),
            "missing prior errors"
        );
    }

    // ── Path traversal regression tests (CWE-22) ──────────────────────────

    #[test]
    fn render_minijinja_rejects_include_traversal() {
        // A template that tries to {% include %} a path outside the base.
        // safe_join rejects any segment starting with '.', so the include
        // fails to resolve and the render errors out.
        let tmp = std::env::temp_dir().join("hkask-include-traversal-test");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("legit.j2"), "hello").unwrap();

        let malicious_template = r#"{% include "../../../etc/passwd" %}"#;
        let ctx = HashMap::new();
        let result = render_minijinja(malicious_template, &ctx, &tmp);
        // The include should fail to resolve (safe_join returns None),
        // producing a render error — not a file read from outside the base.
        assert!(
            result.is_err(),
            "expected render error for traversal include, got: {result:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn render_minijinja_rejects_backslash_include_traversal() {
        let tmp = std::env::temp_dir().join("hkask-backslash-include-test");
        std::fs::create_dir_all(&tmp).unwrap();

        // safe_join rejects segments containing backslashes.
        let malicious_template = r#"{% include "..\\..\\etc\\passwd" %}"#;
        let ctx = HashMap::new();
        let result = render_minijinja(malicious_template, &ctx, &tmp);
        assert!(
            result.is_err(),
            "expected render error for backslash traversal, got: {result:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn render_minijinja_allows_legit_include() {
        // Sanity check: legitimate includes within the base path still work.
        let tmp = std::env::temp_dir().join("hkask-legit-include-test");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("fragment.j2"), "world").unwrap();

        let template = r#"hello {% include "fragment.j2" %}"#;
        let ctx = HashMap::new();
        let result = render_minijinja(template, &ctx, &tmp);
        assert!(
            result.is_ok(),
            "legitimate include should succeed, got: {result:?}"
        );
        assert_eq!(result.unwrap(), "hello world");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── Structured output (tool-calling) tests ───────────────────────────

    use crate::bundle::cascade::CascadePhase;

    #[test]
    fn extract_contract_output_parses_simple_types() {
        let template = "[inference]\ntemplate_type: KnowAct\ncontract:\n  input:\n    topic: string\n  output:\n    convergence_metric: number\n    rationale: string\n    blockers: array\n---\nYou are a convergence evaluator.\n";
        let output = extract_contract_output(template).expect("should find contract.output");
        let fields = output.as_object().expect("output should be an object");
        assert_eq!(fields.len(), 3);
        assert_eq!(
            fields.get("convergence_metric").and_then(|v| v.as_str()),
            Some("number")
        );
        assert_eq!(
            fields.get("rationale").and_then(|v| v.as_str()),
            Some("string")
        );
        assert_eq!(
            fields.get("blockers").and_then(|v| v.as_str()),
            Some("array")
        );
    }

    #[test]
    fn extract_contract_output_returns_none_without_frontmatter() {
        let template = "You are an evaluator. Respond with JSON.";
        assert!(extract_contract_output(template).is_none());
    }

    #[test]
    fn extract_contract_output_returns_none_without_contract() {
        let template = "[inference]\ntemplate_type: KnowAct\n---\nYou are an evaluator.\n";
        assert!(extract_contract_output(template).is_none());
    }

    #[test]
    fn extract_contract_output_strips_jinja_comments() {
        // Some templates have leading Jinja comments ({# ... #}) before the
        // [inference] block. The parser must strip them before YAML parsing.
        let template = "{# goal: Test comment stripping #}\n{# Second comment #}\n[inference]\ntemplate_type: KnowAct\ncontract:\n  output:\n    result: string\n---\nBody\n";
        let output = extract_contract_output(template)
            .expect("should find contract.output despite Jinja comments");
        assert_eq!(
            output.get("result").and_then(|v| v.as_str()),
            Some("string")
        );
    }

    #[test]
    fn contract_output_to_schema_converts_simple_types() {
        let output = serde_json::json!({
            "convergence_metric": "number",
            "rationale": "string",
            "blockers": "array",
            "passed": "boolean",
            "metadata": "object"
        });
        let schema = contract_output_to_schema(&output);
        assert_eq!(schema.get("type").and_then(|v| v.as_str()), Some("object"));
        let props = schema
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("should have properties");
        assert_eq!(
            props
                .get("convergence_metric")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("number")
        );
        assert_eq!(
            props
                .get("rationale")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("string")
        );
        assert_eq!(
            props
                .get("blockers")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("array")
        );
        assert_eq!(
            props
                .get("passed")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("boolean")
        );
        assert_eq!(
            props
                .get("metadata")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("object")
        );
    }

    #[test]
    fn contract_output_to_schema_passes_through_json_schema() {
        let output = serde_json::json!({
            "type": "object",
            "properties": {
                "score": {"type": "number"}
            },
            "required": ["score"]
        });
        let schema = contract_output_to_schema(&output);
        assert_eq!(
            schema, output,
            "pre-existing JSON Schema should pass through"
        );
    }

    #[test]
    fn contract_output_to_schema_defaults_unknown_types_to_string() {
        let output = serde_json::json!({
            "custom_field": "some_unknown_type"
        });
        let schema = contract_output_to_schema(&output);
        let props = schema
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("should have properties");
        assert_eq!(
            props
                .get("custom_field")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("string"),
            "unknown types should default to string"
        );
    }

    #[test]
    fn build_structured_output_tool_creates_valid_definition() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "result": {"type": "string"}
            }
        });
        let tool = build_structured_output_tool(schema.clone());
        assert_eq!(tool.tool_type, "function");
        assert_eq!(tool.function.name, "emit_result");
        assert!(!tool.function.description.is_empty());
        assert_eq!(tool.function.parameters, schema);
    }

    #[test]
    fn resolve_output_schema_prefers_manifest_schema() {
        let step = BundleManifestStep {
            ordinal: 1,
            action: "select".to_string(),
            description: "test".to_string(),
            renderer: Some("minijinja".to_string()),
            template_ref: Some("test/template".to_string()),
            mcp: None,
            compute_ref: None,
            gas_cap: 1000,
            timeout_seconds: 30,
            input_mapping: None,
            output_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "from_manifest": {"type": "string"}
                }
            })),
            phase: CascadePhase::Core,
            condition: None,
            fusion: None,
        };
        let template_content =
            "[inference]\ncontract:\n  output:\n    from_manifest: string\n---\nbody\n";
        let schema = resolve_output_schema(&step, template_content).expect("should resolve");
        assert_eq!(
            schema
                .get("properties")
                .and_then(|v| v.get("from_manifest"))
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("string"),
            "manifest schema should take priority"
        );
        assert!(
            schema
                .get("properties")
                .and_then(|v| v.get("from_template"))
                .is_none(),
            "template schema should not be used when manifest schema is present"
        );
    }

    #[test]
    fn resolve_output_schema_falls_back_to_template_contract() {
        let step = BundleManifestStep {
            ordinal: 1,
            action: "select".to_string(),
            description: "test".to_string(),
            renderer: Some("minijinja".to_string()),
            template_ref: Some("test/template".to_string()),
            mcp: None,
            compute_ref: None,
            gas_cap: 1000,
            timeout_seconds: 30,
            input_mapping: None,
            output_schema: None,
            phase: CascadePhase::Core,
            condition: None,
            fusion: None,
        };
        let template_content = "[inference]\ncontract:\n  output:\n    from_template: string\n    score: number\n---\nbody\n";
        let schema =
            resolve_output_schema(&step, template_content).expect("should resolve from template");
        let props = schema
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("should have properties");
        assert_eq!(
            props
                .get("from_template")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("string")
        );
        assert_eq!(
            props
                .get("score")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("number")
        );
    }

    #[test]
    fn resolve_output_schema_returns_none_without_any_schema() {
        let step = BundleManifestStep {
            ordinal: 1,
            action: "select".to_string(),
            description: "test".to_string(),
            renderer: Some("minijinja".to_string()),
            template_ref: Some("test/template".to_string()),
            mcp: None,
            compute_ref: None,
            gas_cap: 1000,
            timeout_seconds: 30,
            input_mapping: None,
            output_schema: None,
            phase: CascadePhase::Core,
            condition: None,
            fusion: None,
        };
        let template_content = "No frontmatter here.";
        assert!(resolve_output_schema(&step, template_content).is_none());
    }

    // ── ART-3/IR-1: taint propagation through input_mapping bindings ──

    /// Build a minimal executor with only taint_labels populated, for testing
    /// `propagate_taint_for_binding` and `extract_referenced_keys` in isolation.
    ///
    /// The inference/tool ports are stubs — the taint methods don't call them.
    fn test_executor_with_taint(taint: Vec<(&str, ToolTaint)>) -> ManifestExecutor {
        let inference = Arc::new(StubInferencePort);
        let tools = Arc::new(StubToolPort);
        let executor =
            ManifestExecutor::new(inference, tools, LLMParameters::default(), vec![0u8; 32]);
        // Populate taint_labels directly.
        let mut labels = executor.taint_labels.lock().expect("taint mutex");
        for (key, taint) in taint {
            labels.insert(key.to_string(), taint);
        }
        drop(labels);
        executor
    }

    /// Stub inference port for taint-propagation tests. The taint methods
    /// never call inference, so this just needs to satisfy the constructor.
    struct StubInferencePort;

    impl InferencePort for StubInferencePort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = std::result::Result<InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(hkask_types::InferenceError::Generation(
                    "StubInferencePort: inference should not be called for taint tests".into(),
                ))
            })
        }
    }

    /// Stub tool port for taint-propagation tests.
    struct StubToolPort;

    impl hkask_capability::ToolPort for StubToolPort {
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
            Box::pin(async {
                Err(hkask_capability::ToolPortError::NotFound(
                    hkask_types::NotFound {
                        entity_type: "tool".to_string(),
                        id: "stub".to_string(),
                    },
                ))
            })
        }

        fn get_tool_info<'a>(
            &'a self,
            _tool_name: &'a str,
        ) -> hkask_capability::ToolFuture<'a, Option<hkask_capability::ToolInfo>> {
            Box::pin(async { None })
        }

        fn discover_tools<'a>(&'a self) -> hkask_capability::ToolFuture<'a, Vec<String>> {
            Box::pin(async { vec![] })
        }
    }

    #[test]
    fn extract_referenced_keys_from_ref() {
        let executor = test_executor_with_taint(vec![]);
        let value = serde_json::json!({"$ref": "step_1_result.field"});
        let keys = executor.extract_referenced_keys(&value);
        assert_eq!(keys, vec!["step_1_result".to_string()]);
    }

    #[test]
    fn extract_referenced_keys_from_inline_jinja() {
        let executor = test_executor_with_taint(vec![]);
        let value = serde_json::json!("{{ step_2_result.data }}");
        let keys = executor.extract_referenced_keys(&value);
        assert_eq!(keys, vec!["step_2_result".to_string()]);
    }

    #[test]
    fn extract_referenced_keys_ignores_jinja_keywords() {
        let executor = test_executor_with_taint(vec![]);
        // `if` and `for` are Jinja keywords, not context keys.
        let value = serde_json::json!("{% if step_1_result %}{{ step_1_result }}{% endif %}");
        let keys = executor.extract_referenced_keys(&value);
        assert_eq!(keys, vec!["step_1_result".to_string()]);
    }

    #[test]
    fn extract_referenced_keys_from_nested_object() {
        let executor = test_executor_with_taint(vec![]);
        let value = serde_json::json!({
            "items": [
                {"$ref": "step_1_result.a"},
                "{{ step_2_result.b }}"
            ]
        });
        let mut keys = executor.extract_referenced_keys(&value);
        keys.sort();
        assert_eq!(
            keys,
            vec!["step_1_result".to_string(), "step_2_result".to_string()]
        );
    }

    #[test]
    fn propagate_taint_from_ref_source() {
        let executor = test_executor_with_taint(vec![("step_1_result", ToolTaint::Source)]);
        let value = serde_json::json!({"$ref": "step_1_result.field"});
        executor.propagate_taint_for_binding(&value, "new_key");
        let labels = executor.taint_labels.lock().expect("taint mutex");
        assert_eq!(
            labels.get("new_key").copied(),
            Some(ToolTaint::Source),
            "Source taint must propagate through $ref bindings"
        );
    }

    #[test]
    fn propagate_taint_from_inline_jinja_source() {
        // This is the ART-3/IR-1 regression test: inline Jinja binding of a
        // Source-tainted entry must propagate the taint label. Before the fix,
        // `context.insert` (not `insert_tainted`) was used, so the new key
        // was silently labeled Pure — bypassing the FIDES Source→Sink block.
        let executor = test_executor_with_taint(vec![("step_1_result", ToolTaint::Source)]);
        let value = serde_json::json!("{{ step_1_result }}");
        executor.propagate_taint_for_binding(&value, "bound_data");
        let labels = executor.taint_labels.lock().expect("taint mutex");
        assert_eq!(
            labels.get("bound_data").copied(),
            Some(ToolTaint::Source),
            "Source taint must propagate through inline-Jinja bindings"
        );
    }

    #[test]
    fn propagate_taint_does_not_label_pure_references() {
        let executor = test_executor_with_taint(vec![]); // no tainted entries
        let value = serde_json::json!("{{ step_1_result }}");
        executor.propagate_taint_for_binding(&value, "new_key");
        let labels = executor.taint_labels.lock().expect("taint mutex");
        assert_eq!(
            labels.get("new_key"),
            None,
            "Pure references must not acquire a taint label"
        );
    }

    #[test]
    fn propagate_taint_endorser_is_preserved_but_not_upgraded() {
        let executor = test_executor_with_taint(vec![("step_1_result", ToolTaint::Endorser)]);
        let value = serde_json::json!("{{ step_1_result }}");
        executor.propagate_taint_for_binding(&value, "endorsed_key");
        let labels = executor.taint_labels.lock().expect("taint mutex");
        assert_eq!(
            labels.get("endorsed_key").copied(),
            Some(ToolTaint::Endorser),
            "Endorser taint must propagate (audit trail) but not upgrade to Source"
        );
    }

    #[test]
    fn check_untrusted_input_detects_tainted_ref_after_propagation() {
        // End-to-end: after propagate_taint_for_binding labels a new key as
        // Source, check_untrusted_input on a $ref to that key must return true.
        let executor = test_executor_with_taint(vec![("step_1_result", ToolTaint::Source)]);
        // Simulate: input_mapping binds step_1_result (Source) to "data".
        let mapping_value = serde_json::json!("{{ step_1_result }}");
        executor.propagate_taint_for_binding(&mapping_value, "data");
        // Now a Sink tool receives input referencing "data" via $ref.
        let sink_input = serde_json::json!({"$ref": "data.field"});
        assert!(
            executor.check_untrusted_input(&sink_input),
            "After taint propagation, $ref to the bound key must be detected as untrusted"
        );
    }
}
