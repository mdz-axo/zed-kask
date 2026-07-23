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

use crate::bundle::BundleManifest;
use crate::bundle::BundleManifestStep;
use crate::ports::{Result, TemplateError};
use hkask_capability::{DelegationAction, DelegationResource, DelegationToken};
use hkask_capability::{ToolPort, ToolPortError};
use hkask_guard::{SpotlightMode, Spotlighter};
use hkask_types::NotFound;
use hkask_types::ToolTaint;
use hkask_types::WebID;
use hkask_types::template::LLMParameters;
use hkask_types::{InferencePort, InferenceResult};
use minijinja::UndefinedBehavior;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};
use zeroize::Zeroizing;

/// Error healing callback: (error_string, operation_name).
type HealCallback = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Default base path for template files relative to the project root.
const DEFAULT_TEMPLATE_BASE_PATH: &str = "registry/templates";

/// Safely join a base path with a template reference, rejecting path traversal.
///
/// Mirrors minijinja's internal `safe_join`: any segment starting with `.`
/// or containing a backslash is rejected. This prevents `{% include "../../etc/passwd" %}`
/// and `template_ref: "../../../secrets"` from reading files outside the base.
///
/// Returns `None` if the template_ref would escape the base path.
fn safe_template_join(base: &std::path::Path, template_ref: &str) -> Option<PathBuf> {
    let mut rv = base.to_path_buf();
    for segment in template_ref.split('/') {
        if segment.starts_with('.') || segment.contains('\\') {
            return None;
        }
        rv.push(segment);
    }
    Some(rv)
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
    template_base_path: PathBuf,
    /// Optional heal callback: (error_string, operation_name).
    heal_error_cb: Option<HealCallback>,
    /// Spotlighter for transforming untrusted tool outputs (Layer 2 defense).
    /// Applied to every MCP tool result before it enters the LLM context.
    /// Source: Microsoft Research arXiv:2403.14720
    spotlighter: Spotlighter,
    /// Optional runtime policy for pre-execution checks (Layer 6 defense).
    /// When present, checked before every MCP tool invocation.
    /// Source: VeriGuard pattern + AgentGuard arXiv:2509.23864
    runtime_policy: Option<Arc<dyn hkask_regulation::RuntimePolicy>>,
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
            template_base_path: PathBuf::from(DEFAULT_TEMPLATE_BASE_PATH),
            heal_error_cb: None,
            spotlighter: Spotlighter::new(SpotlightMode::Delimit),
            runtime_policy: None,
            taint_labels: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Set the template base path for resolving template_ref values.
    /// Useful for integration tests that need to point to a test fixture directory.
    #[must_use]
    pub fn with_template_base_path(mut self, path: PathBuf) -> Self {
        self.template_base_path = path;
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
    pub fn with_runtime_policy(mut self, policy: Arc<dyn hkask_regulation::RuntimePolicy>) -> Self {
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

    /// Load and render a template from the filesystem.
    fn load_template(
        &self,
        template_ref: &str,
        context: &HashMap<String, Value>,
    ) -> Result<String> {
        let template_path =
            safe_template_join(&self.template_base_path, template_ref).ok_or_else(|| {
                TemplateError::PathTraversal(format!(
                    "template_ref '{template_ref}' escapes base path '{}'",
                    self.template_base_path.display()
                ))
            })?;
        let template_content = std::fs::read_to_string(&template_path).map_err(|e| {
            TemplateError::NotFound(NotFound {
                entity_type: "template".to_string(),
                id: format!(
                    "KnowAct template not found at {}: {}",
                    template_path.display(),
                    e
                ),
            })
        })?;
        let prompt = render_minijinja(&template_content, context, &self.template_base_path)?;
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

        let max_iterations = if manifest.convergence.max_iterations == 0 {
            1 // single-pass for one-shot manifests
        } else {
            manifest.convergence.max_iterations
        };
        let threshold = manifest.convergence.threshold;
        let field = manifest.convergence.convergence_field.clone();
        let improvement_enabled = manifest.convergence.improvement_ratio > 0.0;
        let min_iterations = manifest.convergence.min_iterations;
        let mut baseline_quality: Option<f64> = None; // captured on first pass
        let mut iteration: u32 = 0;
        let mut recursion_depth: u8 = 0;
        let matryoshka_limit: u8 = hkask_capability::SYSTEM_MAX_RECURSION;
        // Gas tracking — hard parent allocation for compute cycles
        let gas_cap = manifest.gas.cap as u64;
        let gas_cost_per_iter = manifest.gas.cost_per_iteration as u64;
        let gas_alert_threshold = manifest.gas.alert_threshold;
        let gas_hard_limit = manifest.gas.hard_limit;
        let mut gas_used: u64 = 0;
        let mut gas_alerted: bool = false;
        // rJoule tracking — hard parent allocation for inference energy
        // Cost per token is set by the inference provider/model config, not the manifest.
        let rjoule_cap = manifest.rjoule.cap as f64;
        let rjoule_alert_threshold = manifest.rjoule.alert_threshold;
        let rjoule_hard_limit = manifest.rjoule.hard_limit;
        let rjoule_enabled = rjoule_cap > 0.0;
        let mut rjoule_used: f64 = 0.0;
        let mut rjoule_alerted: bool = false;

        // Manifest-level fusion control: when manifest.fusion is Some(config),
        // all steps use this per-manifest fusion config (custom judge/panel/mode).
        // When None, follows the global default (global fusion if configured).
        // Per-step fusion: Some(false) bypasses, Some(true) forces manifest config,
        // None inherits the manifest behavior.
        let manifest_fusion_config = manifest.fusion.clone();

        context.insert(
            "_convergence".to_string(),
            serde_json::json!({
                "threshold": threshold,
                "max_iterations": max_iterations,
                "field": field,
                "status": "running",
                "iterations_completed": 0,
                "exit_reason": null,
                "improvement_target": manifest.convergence.improvement_ratio,
                "baseline_quality": null,
                "gas_cap": gas_cap,
                "gas_used": 0,
                "gas_remaining": gas_cap,
                "rjoule_cap": rjoule_cap,
                "rjoule_used": 0.0,
                "rjoule_remaining": rjoule_cap,
            }),
        );

        let mut step_idx: usize = 0;

        'cascade: loop {
            iteration += 1;
            // Update live convergence context for template awareness
            context.insert(
                "_convergence".to_string(),
                serde_json::json!({
                    "threshold": threshold,
                    "max_iterations": max_iterations,
                    "field": field,
                    "status": "running",
                    "iterations_completed": iteration,
                    "exit_reason": null,
                    "improvement_target": manifest.convergence.improvement_ratio,
                    "baseline_quality": baseline_quality,
                    "gas_cap": gas_cap,
                    "gas_used": gas_used,
                    "gas_remaining": gas_cap.saturating_sub(gas_used),
                    "rjoule_cap": rjoule_cap,
                    "rjoule_used": rjoule_used,
                    "rjoule_remaining": (rjoule_cap - rjoule_used).max(0.0),
                }),
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
                        match render_minijinja(cond, &context, &self.template_base_path) {
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
                        self.finalize_convergence_report(
                            &mut context,
                            "converged",
                            "quality_met",
                            iteration,
                            threshold,
                            &field,
                            baseline_quality,
                            manifest.convergence.improvement_ratio,
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
                        self.finalize_convergence_report(
                            &mut context,
                            "escalated",
                            "obstacle_blocked",
                            iteration,
                            threshold,
                            &field,
                            baseline_quality,
                            manifest.convergence.improvement_ratio,
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
                            self.finalize_convergence_report(
                                &mut context,
                                "maxed_out",
                                "energy_spent",
                                iteration,
                                threshold,
                                &field,
                                baseline_quality,
                                manifest.convergence.improvement_ratio,
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
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32;

                        info!(
                            target: "reg.skill.cascade.step_executed",
                            iteration = iteration,
                            loop_target = loop_target,
                            depth = recursion_depth,
                            "REG"
                        );

                        // Check convergence before looping
                        if iteration >= max_iterations {
                            self.finalize_convergence_report(
                                &mut context,
                                "maxed_out",
                                "energy_spent",
                                iteration,
                                threshold,
                                &field,
                                baseline_quality,
                                manifest.convergence.improvement_ratio,
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

                        // Check threshold convergence
                        if self.check_convergence(
                            &context,
                            &manifest.convergence.convergence_field,
                            threshold,
                            manifest.convergence.improvement_ratio,
                            &manifest.convergence.improvement_gate,
                            baseline_quality,
                            iteration,
                            min_iterations,
                        ) {
                            self.finalize_convergence_report(
                                &mut context,
                                "converged",
                                "quality_met",
                                iteration,
                                threshold,
                                &field,
                                baseline_quality,
                                manifest.convergence.improvement_ratio,
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
                                let bound =
                                    resolve_mapping_value(v, &context, &self.template_base_path);
                                context.insert(k.clone(), bound);
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
                                &mut gas_used,
                                gas_cap,
                                gas_cost_per_iter,
                                &mut rjoule_used,
                                rjoule_cap,
                                rjoule_enabled,
                                rjoule_hard_limit,
                                manifest_fusion_config.as_ref(),
                            )
                            .await?;
                        // Check gas exhaustion after select
                        if gas_hard_limit && gas_used >= gas_cap {
                            info!(
                                target: "reg.skill.budget.gas_exhausted",
                                iteration = iteration,
                                gas_used = gas_used,
                                gas_cap = gas_cap,
                                "REG"
                            );
                            self.finalize_convergence_report(
                                &mut context,
                                "maxed_out",
                                "energy_spent",
                                iteration,
                                threshold,
                                &field,
                                baseline_quality,
                                manifest.convergence.improvement_ratio,
                            );
                            break 'cascade;
                        }
                        // Gas alert threshold
                        if !gas_alerted
                            && gas_cap > 0
                            && (gas_used as f64 / gas_cap as f64) >= gas_alert_threshold
                        {
                            gas_alerted = true;
                            info!(
                                target: "reg.skill.budget.gas_alert",
                                gas_used = gas_used,
                                gas_cap = gas_cap,
                                pct = (gas_used as f64 / gas_cap as f64) * 100.0,
                                "REG"
                            );
                        }
                        // Check rJoule exhaustion after select
                        if rjoule_enabled && rjoule_hard_limit && rjoule_used >= rjoule_cap {
                            info!(
                                target: "reg.skill.budget.rjoule_exhausted",
                                iteration = iteration,
                                rjoule_used = rjoule_used,
                                rjoule_cap = rjoule_cap,
                                "REG"
                            );
                            self.finalize_convergence_report(
                                &mut context,
                                "maxed_out",
                                "energy_spent",
                                iteration,
                                threshold,
                                &field,
                                baseline_quality,
                                manifest.convergence.improvement_ratio,
                            );
                            break 'cascade;
                        }
                        // rJoule alert threshold
                        if !rjoule_alerted
                            && rjoule_cap > 0.0
                            && (rjoule_used / rjoule_cap) >= rjoule_alert_threshold
                        {
                            rjoule_alerted = true;
                            info!(
                                target: "reg.skill.budget.rjoule_alert",
                                rjoule_used = rjoule_used,
                                rjoule_cap = rjoule_cap,
                                pct = (rjoule_used / rjoule_cap) * 100.0,
                                "REG"
                            );
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

                    other => {
                        return Err(TemplateError::Manifest(format!(
                            "Unknown manifest step action: '{}'",
                            other
                        )));
                    }
                }

                step_idx += 1;
            }

            // while loop exited normally — reset step_idx for implicit loop re-entry
            step_idx = 0;

            // Check gas exhaustion at end of pass
            if gas_hard_limit && gas_used >= gas_cap {
                info!(
                    target: "reg.skill.budget.gas_exhausted",
                    iteration = iteration,
                    gas_used = gas_used,
                    gas_cap = gas_cap,
                    "REG"
                );
                self.finalize_convergence_report(
                    &mut context,
                    "maxed_out",
                    "energy_spent",
                    iteration,
                    threshold,
                    &field,
                    baseline_quality,
                    manifest.convergence.improvement_ratio,
                );
                break 'cascade;
            }

            // Capture baseline quality on first full pass
            if improvement_enabled && baseline_quality.is_none() {
                baseline_quality = context
                    .get(&field)
                    .and_then(|v| v.as_f64())
                    .or_else(|| resolve_dot_path(&field, &context).and_then(|v| v.as_f64()));
            }

            // Compute compound quality from nested skill reports
            if manifest.convergence.aggregation != "none"
                && !manifest.convergence.aggregation_sources.is_empty()
            {
                let compound = self.compute_compound_quality(
                    &context,
                    &manifest.convergence.aggregation,
                    &manifest.convergence.aggregation_sources,
                );
                context.insert(field.clone(), serde_json::json!(compound));
            }

            // ── End of pass: check convergence if no explicit loop/abort ──
            if iteration >= max_iterations {
                self.finalize_convergence_report(
                    &mut context,
                    "maxed_out",
                    "energy_spent",
                    iteration,
                    threshold,
                    &field,
                    baseline_quality,
                    manifest.convergence.improvement_ratio,
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

            if self.check_convergence(
                &context,
                &manifest.convergence.convergence_field,
                threshold,
                manifest.convergence.improvement_ratio,
                &manifest.convergence.improvement_gate,
                baseline_quality,
                iteration,
                min_iterations,
            ) {
                self.finalize_convergence_report(
                    &mut context,
                    "converged",
                    "quality_met",
                    iteration,
                    threshold,
                    &field,
                    baseline_quality,
                    manifest.convergence.improvement_ratio,
                );
                break 'cascade;
            }

            // Implicit loop: re-enter from step 0
            recursion_depth += 1;
            if recursion_depth > matryoshka_limit {
                self.finalize_convergence_report(
                    &mut context,
                    "maxed_out",
                    "energy_spent",
                    iteration,
                    threshold,
                    &field,
                    baseline_quality,
                    manifest.convergence.improvement_ratio,
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

    /// Finalize the convergence report at cascade exit.
    /// Writes a complete report with status, reason, iterations, quality at exit, threshold, field,
    /// and improvement metadata (baseline_quality, improvement_ratio, improvement_pct).
    #[allow(clippy::too_many_arguments)]
    fn finalize_convergence_report(
        &self,
        context: &mut HashMap<String, Value>,
        status: &str,
        reason: &str,
        iteration: u32,
        threshold: f64,
        field: &str,
        baseline_quality: Option<f64>,
        improvement_target: f64,
    ) {
        let quality = context
            .get(field)
            .and_then(|v| v.as_f64())
            .or_else(|| resolve_dot_path(field, context).and_then(|v| v.as_f64()));

        let gas_used_val = context
            .get("_gas")
            .and_then(|g| g.get("used"))
            .and_then(|v| v.as_u64())
            .map(|v| v as f64)
            .unwrap_or(0.0);
        let gas_cap_val = context
            .get("_gas")
            .and_then(|g| g.get("cap"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        context.insert(
            "_convergence".to_string(),
            serde_json::json!({
                "status": status,
                "reason": reason,
                "iterations_completed": iteration,
                "quality_at_exit": quality,
                "threshold": threshold,
                "field": field,
                "improvement_achieved": baseline_quality.and_then(|b| quality.map(|q| if b > 0.0 { (b - q) / b } else { 0.0 })),
                "improvement_pct": baseline_quality.and_then(|b| quality.map(|q| if b > 0.0 { ((b - q) / b) * 100.0 } else { 0.0 })),
                "improvement_target": improvement_target,
                "baseline_quality": baseline_quality,
                "gas_used": gas_used_val,
                "gas_cap": gas_cap_val,
                "gas_remaining": (gas_cap_val - gas_used_val).max(0.0),
                "gas_pct": if gas_cap_val > 0.0 { (gas_used_val / gas_cap_val) * 100.0 } else { 0.0 },
                "rjoule_used": context.get("_rjoule").and_then(|g| g.get("used")).and_then(|v| v.as_f64()).unwrap_or(0.0),
                "rjoule_cap": context.get("_rjoule").and_then(|g| g.get("cap")).and_then(|v| v.as_f64()).unwrap_or(0.0),
            }),
        );
    }

    /// Check whether the convergence threshold has been met.
    /// Looks for the configured `convergence_field` in the context (defaults to "composite").
    /// Also enforces improvement tracking: min_iterations, improvement_ratio, and improvement_gate.
    #[allow(clippy::too_many_arguments)]
    fn check_convergence(
        &self,
        context: &HashMap<String, Value>,
        field: &str,
        threshold: f64,
        improvement_ratio: f64,
        improvement_gate: &str,
        baseline_quality: Option<f64>,
        iteration: u32,
        min_iterations: u32,
    ) -> bool {
        // Enforce minimum iterations before exit is allowed
        if iteration <= min_iterations {
            return false;
        }

        // Compute current quality and threshold check
        let current = context
            .get(field)
            .and_then(|v| v.as_f64())
            .or_else(|| resolve_dot_path(field, context).and_then(|v| v.as_f64()));

        // Fallback: also check the composite score when a specific field is configured
        let current = if current.is_none() && field != "composite" {
            context.get("composite").and_then(|v| v.as_f64())
        } else {
            current
        };

        // Check convergence metadata as additional fallback
        let current = if current.is_none() {
            context.get("_convergence_score").and_then(|v| v.as_f64())
        } else {
            current
        };

        let threshold_met = current.map(|q| q <= threshold).unwrap_or(false);

        // Compute improvement from baseline as proportional ratio
        let improvement_met = if improvement_ratio > 0.0 {
            match (baseline_quality, current) {
                (Some(b), Some(c)) if b > 0.0 => ((b - c) / b) >= improvement_ratio,
                _ => false,
            }
        } else {
            false
        };

        match improvement_gate {
            "both" => threshold_met && improvement_met,
            "either" => threshold_met || improvement_met,
            _ => threshold_met, // "threshold_only"
        }
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
    #[allow(clippy::too_many_arguments)]
    async fn execute_select(
        &self,
        step: &BundleManifestStep,
        mut context: HashMap<String, Value>,
        gas_used: &mut u64,
        gas_cap: u64,
        gas_cost_per_iter: u64,
        rjoule_used: &mut f64,
        rjoule_cap: f64,
        rjoule_enabled: bool,
        _rjoule_hard_limit: bool,
        manifest_fusion_config: Option<&hkask_types::fusion::FusionConfig>,
    ) -> Result<HashMap<String, Value>> {
        // Apply input_mapping: resolve {{ }} string values (and $ref objects) from the
        // context and promote them to top-level template variables. Without this, mapped
        // names referenced in .j2 templates (e.g. {{ tasks }}) would render empty.
        if let Some(ref mapping) = step.input_mapping
            && let Value::Object(map) = mapping
        {
            for (k, v) in map {
                let bound = resolve_mapping_value(v, &context, &self.template_base_path);
                context.insert(k.clone(), bound);
            }
        }

        let prompt = self.render_step_template(step, &context)?;

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

        // Single-model or fusion path — the fusion routing above handles it.
        let result_text = {
            let timeout_dur = std::time::Duration::from_secs(step.timeout_seconds as u64);
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
                        "Step {} timed out after {}s",
                        step.ordinal, step.timeout_seconds
                    )));
                }
            };
            result.text
        };

        // rJoule tracking — cost per token comes from inference provider.
        // Token count is tracked; rJoule deduction wired when provider reports cost.
        if rjoule_enabled {
            let tokens = result_text.len() as f64 / 4.0; // rough token estimate
            let _ = tokens;
        }

        // Gas tracking — deduct one iteration of compute
        *gas_used = gas_used.saturating_add(gas_cost_per_iter);

        let parsed: Value = parse_json_response(&result_text, step.ordinal)?;
        context.insert(format!("step_{}_result", step.ordinal), parsed);

        // Inject dual-budget context for template awareness
        let gas_remaining = gas_cap.saturating_sub(*gas_used);
        let rjoule_remaining = (rjoule_cap - *rjoule_used).max(0.0);
        context.insert(
            "_gas".to_string(),
            serde_json::json!({
                "used": *gas_used,
                "cap": gas_cap,
                "remaining": gas_remaining,
                "cost_per_iteration": gas_cost_per_iter,
            }),
        );
        context.insert(
            "_rjoule".to_string(),
            serde_json::json!({
                "used": *rjoule_used,
                "cap": rjoule_cap,
                "remaining": rjoule_remaining,
                "enabled": rjoule_enabled,
            }),
        );

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
        let mcp_ref = render_inline_template(mcp_ref_raw, &context);

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
        other => Err(TemplateError::Manifest(format!(
            "Unknown compute_ref: '{}'. Supported: calibrate_from_fermi, outside_view_adjustment, bayesian_update, apply_calibration_adjustment, brier_score, brier_score_multi, brier_interpretation",
            other
        ))),
    }
}

/// Render a template step according to its renderer mode.
///
/// Dispatches based on `step.renderer`:
/// - `"minijinja"` — Load template from `step.template_ref` (a file path
///   like `curator/system_state_gather.j2`) relative to `template_base_path`,
///   then render with full Jinja2 syntax via minijinja.
/// - Inline/absent — Render `step.template_ref` or `step.renderer` as a
///   simple template string with `{{key}}` substitution.
impl ManifestExecutor {
    fn render_step_template(
        &self,
        step: &BundleManifestStep,
        context: &HashMap<String, Value>,
    ) -> Result<String> {
        let renderer = step.renderer.as_deref().unwrap_or("");

        match renderer {
            "minijinja" => {
                // template_ref is a file path relative to template_base_path.
                // Resolve {{key}} references from context before loading.
                let template_ref_raw = step.template_ref.as_deref().ok_or_else(|| {
                    TemplateError::Manifest(format!(
                        "Step {} has renderer='minijinja' but no template_ref",
                        step.ordinal
                    ))
                })?;
                let template_ref = render_inline_template(template_ref_raw, context);

                let template_path = safe_template_join(&self.template_base_path, &template_ref)
                    .ok_or_else(|| {
                        TemplateError::PathTraversal(format!(
                            "step {}: template_ref '{template_ref}' escapes base path '{}'",
                            step.ordinal,
                            self.template_base_path.display()
                        ))
                    })?;
                let template_content = match std::fs::read_to_string(&template_path) {
                    Ok(c) => c,
                    Err(e) => {
                        // Fallback: if template_ref doesn't end with .j2, try appending it.
                        // Many manifests omit the extension; KataEngine resolves without it,
                        // and ManifestExecutor should too.
                        if !template_ref.ends_with(".j2") {
                            let j2_ref = format!("{template_ref}.j2");
                            let j2_path = safe_template_join(&self.template_base_path, &j2_ref)
                                .ok_or_else(|| {
                                    TemplateError::PathTraversal(format!(
                                        "step {}: template_ref '{j2_ref}' escapes base path '{}'",
                                        step.ordinal,
                                        self.template_base_path.display()
                                    ))
                                })?;
                            if let Ok(c) = std::fs::read_to_string(&j2_path) {
                                // Success with .j2 extension
                                info!(
                                    target: "reg.spec.executor",
                                    step = step.ordinal,
                                    resolved = %j2_path.display(),
                                    "Resolved template with .j2 fallback"
                                );
                                c
                            } else {
                                let err_msg = format!(
                                    "Step {}: template file not found at {} (also tried {}): {}",
                                    step.ordinal,
                                    template_path.display(),
                                    j2_path.display(),
                                    e
                                );
                                if let Some(ref cb) = self.heal_error_cb {
                                    cb(&err_msg, &template_path.display().to_string());
                                }
                                return Err(TemplateError::NotFound(NotFound {
                                    entity_type: "template".to_string(),
                                    id: err_msg,
                                }));
                            }
                        } else {
                            let err_msg = format!(
                                "Step {}: template file not found at {}: {}",
                                step.ordinal,
                                template_path.display(),
                                e
                            );
                            if let Some(ref cb) = self.heal_error_cb {
                                cb(&err_msg, &template_path.display().to_string());
                            }
                            return Err(TemplateError::NotFound(NotFound {
                                entity_type: "template".to_string(),
                                id: err_msg,
                            }));
                        }
                    }
                };

                info!(
                    target: "reg.spec.executor",
                    step = step.ordinal,
                    template = %template_ref,
                    "Rendering minijinja template"
                );

                render_minijinja(&template_content, context, &self.template_base_path)
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

                Ok(render_inline_template(template_content, context))
            }
        }
    }
}

/// Render a template using minijinja (full Jinja2 syntax).
///
/// Supports `{% for %}`, `{{ var }}`, `| filter`, `{% if %}`, `{% include %}`
/// etc. The main template is registered under the synthetic name `"step"`;
/// `{% include "path/frag.j2" %}` references resolve relative to
/// `template_base_path` (the same root used for `template_ref` values).
fn render_minijinja(
    template: &str,
    context: &HashMap<String, Value>,
    template_base_path: &std::path::Path,
) -> Result<String> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Lenient);

    // Register custom filters
    env.add_filter(
        "truncate",
        |state: &minijinja::State, value: String, max_len: usize| -> String {
            let _ = state;
            if value.len() <= max_len {
                value
            } else {
                let mut truncated: String = value.chars().take(max_len).collect();
                truncated.push_str("...");
                truncated
            }
        },
    );

    // Loader: the synthetic "step" name resolves to the in-memory main
    // template; any other name (from `{% include %}`) resolves from disk
    // under `template_base_path`, mirroring the `template_ref` resolution
    // rules (including the `.j2` extension fallback).
    let main_template = template.to_string();
    let base = template_base_path.to_path_buf();
    env.set_loader(
        move |name: &str| -> std::result::Result<Option<String>, minijinja::Error> {
            if name == "step" {
                return Ok(Some(main_template.clone()));
            }
            // safe_join rejects any segment starting with '.' or containing '\\',
            // preventing `{% include "../../etc/passwd" %}` path traversal.
            let primary = match safe_template_join(&base, name) {
                Some(p) => p,
                None => return Ok(None),
            };
            if let Ok(content) = std::fs::read_to_string(&primary) {
                return Ok(Some(content));
            }
            if !name.ends_with(".j2") {
                let j2_name = format!("{name}.j2");
                if let Some(j2_path) = safe_template_join(&base, &j2_name)
                    && let Ok(content) = std::fs::read_to_string(&j2_path)
                {
                    return Ok(Some(content));
                }
            }
            Ok(None)
        },
    );

    // Convert HashMap<String, Value> to minijinja context via serde
    let context_value = serde_json::to_value(context)
        .map_err(|e| TemplateError::Render(format!("Failed to serialize context: {}", e)))?;
    let minijinja_context = minijinja::Value::from_serialize(&context_value);

    // Validate the main template parses, surfacing syntax errors with a
    // clear message (the loader resolves "step" lazily on first access).
    env.add_template("step", template)
        .map_err(|e| TemplateError::Render(format!("Invalid template: {}", e)))?;

    env.get_template("step")
        .and_then(|tmpl| tmpl.render(minijinja_context))
        .map_err(|e| TemplateError::Render(format!("Template render error: {}", e)))
}

/// Render an inline template using simple `{{key}}` substitution.
fn render_inline_template(template: &str, context: &HashMap<String, Value>) -> String {
    let mut result = template.to_string();
    for (key, value) in context {
        let placeholder = format!("{{{{{}}}}}", key);
        let replacement = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        result = result.replace(&placeholder, &replacement);
    }
    result
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

impl ManifestExecutor {
    /// Compute compound quality from nested inner skill convergence reports.
    fn compute_compound_quality(
        &self,
        context: &HashMap<String, Value>,
        method: &str,
        sources: &[crate::bundle::config::AggregationSource],
    ) -> f64 {
        match method {
            "all_converged" => {
                let all_ok = sources.iter().all(|src| {
                    let key = format!("step_{}_result", src.step_ordinal);
                    context
                        .get(&key)
                        .and_then(|v| v.get("_convergence"))
                        .and_then(|c| c.get("status"))
                        .and_then(|s| s.as_str())
                        .map(|s| s == "converged")
                        .unwrap_or(false)
                });
                if all_ok { 0.0 } else { 1.0 }
            }
            "min" => sources
                .iter()
                .filter_map(|src| {
                    let key = format!("step_{}_result", src.step_ordinal);
                    context
                        .get(&key)
                        .and_then(|v| v.get("_convergence"))
                        .and_then(|c| c.get("quality_at_exit"))
                        .and_then(|v| v.as_f64())
                })
                .fold(1.0_f64, f64::min),
            "weighted_avg" => {
                let mut sum = 0.0_f64;
                let mut total = 0.0_f64;
                for src in sources {
                    let key = format!("step_{}_result", src.step_ordinal);
                    if let Some(v) = context
                        .get(&key)
                        .and_then(|v| v.get("_convergence"))
                        .and_then(|c| c.get("quality_at_exit"))
                        .and_then(|v| v.as_f64())
                    {
                        sum += v * src.weight;
                        total += src.weight;
                    }
                }
                if total > 0.0 { sum / total } else { 1.0 }
            }
            _ => 0.0,
        }
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
}
