//! Manifest executor — public API and utilities.
//!
//! The heavy lifting lives in `step_machine.rs` (the deterministic interpreter)
//! and `step_actions.rs` (the per-action implementations). This module exposes
//! the `ManifestExecutor` builder + `execute_manifest` entry point that the
//! bridge (`kask_bridge::skill_executor`) calls, plus the utility functions
//! (`normalize_model_output`, `parse_json_response`, `extract_final_step_result`,
//! `extract_feedback_phase`) that `step_actions.rs` and the bridge consume.

use crate::budget::BudgetTracker;
use crate::convergence::ConvergenceTracker;
use crate::ports::{Result, TemplateError};
use crate::step_context::StepContext;
use crate::step_graph::{MAX_STEPS, StepGraph};
use crate::step_machine::{CascadeOutcome, Infra, StepMachine};
use crate::template_renderer::TemplateRenderer;
use hkask_types::json_extract as llm_json;
use hkask_types::template::LLMParameters;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Extract the feedback phase from a template reference for span emission.
///
/// Maps the last path segment of a template_ref to a canonical phase name
/// (Classify, Gather, Draft, Evaluate, Convergence, OperatorFeedback, Write,
/// Outcome). Used by `step_machine.rs` to emit `reg.skill.cascade.step_executed`
/// spans with the correct phase field.
pub(crate) fn extract_feedback_phase(template_ref: &str) -> Option<&'static str> {
    let last_segment = template_ref.rsplit('/').next().unwrap_or(template_ref);
    if last_segment.contains("classify") {
        Some("Classify")
    } else if last_segment.contains("gather") {
        Some("Gather")
    } else if last_segment.contains("draft")
        || last_segment.contains("generate")
        || last_segment.contains("extract")
    {
        Some("Draft")
    } else if last_segment.contains("evaluate") {
        Some("Evaluate")
    } else if last_segment.contains("convergence") || last_segment.contains("converge") {
        Some("Convergence")
    } else if last_segment.contains("operator_feedback") || last_segment.contains("feedback") {
        Some("OperatorFeedback")
    } else if last_segment.contains("write") {
        Some("Write")
    } else if last_segment.contains("outcome") {
        Some("Outcome")
    } else {
        None
    }
}

/// Manifest executor — drives the skill cascade via `StepMachine`.
///
/// Created once per session (or per manifest invocation) and wired into the
/// REPL turn loop. The executor holds references to the infrastructure
/// ports it needs:
///
/// - `InferencePort` — for `select` steps (the only probabilistic action)
/// - `ToolPort` — for `execute` steps (MCP tool invocation)
/// - `TemplateRenderer` — for `minijinja` template rendering
///
/// `execute_manifest` builds a `StepGraph` + `StepContext` + `StepMachine`
/// and runs the cascade to completion. The machine's dispatch loop is ~30
/// lines; each action is a separate method in `step_actions.rs`.
#[derive(Clone)]
pub struct ManifestExecutor {
    inference: Arc<dyn hkask_types::ports::inference_port::InferencePort>,
    tools: Arc<dyn hkask_capability::ToolPort>,
    default_params: LLMParameters,
    template_renderer: TemplateRenderer,
    terminal_check: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    title: Option<Arc<dyn Fn(&str) + Send + Sync>>,
}

impl ManifestExecutor {
    /// Create a new executor with the given infrastructure ports.
    pub fn new(
        inference: Arc<dyn hkask_types::ports::inference_port::InferencePort>,
        tools: Arc<dyn hkask_capability::ToolPort>,
        default_params: LLMParameters,
    ) -> Self {
        Self {
            inference,
            tools,
            default_params,
            template_renderer: TemplateRenderer::new(std::path::PathBuf::from(
                crate::template_renderer::DEFAULT_TEMPLATE_BASE_PATH,
            )),
            terminal_check: None,
            progress: None,
            title: None,
        }
    }

    /// Wire a callback that checks whether the `terminal` built-in tool is
    /// enabled for the current agent profile (proposer/evaluator separation).
    #[must_use]
    pub fn with_terminal_check(mut self, check: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        self.terminal_check = Some(check);
        self
    }

    /// Wire a progress callback for real-time cascade feedback (thinking traces).
    #[must_use]
    pub fn with_progress(mut self, progress: Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Wire a title callback for step-label updates.
    #[must_use]
    pub fn with_title(mut self, title: Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        self.title = Some(title);
        self
    }

    /// Set the template base path for resolving template_ref values.
    #[must_use]
    pub fn with_template_base_path(mut self, path: std::path::PathBuf) -> Self {
        self.template_renderer = TemplateRenderer::new(path);
        self
    }

    /// Execute the full manifest cascade.
    ///
    /// Builds a `StepGraph` from the manifest's steps, a `StepContext` from
    /// the initial context, and a `StepMachine` to drive the cascade. The
    /// machine's dispatch loop runs each step's action via `step_actions.rs`,
    /// checks convergence in one place (the `Reenter` arm), and checks budget
    /// in one place. Returns the final context map with convergence metadata
    /// under `_convergence`.
    /// Execute the full manifest cascade (borrowed interface — delegates to
    /// `execute_manifest_into` via clone). For callers that hold a borrowed
    /// executor/manifest and await directly (tests). Callers that need a
    /// `'static + Send` future for `tokio::spawn` must use `execute_manifest_into`
    /// (owned `self` + `manifest`) so the future owns both and has no borrows.
    pub async fn execute_manifest(
        &self,
        manifest: &crate::bundle::BundleManifest,
        initial_context: HashMap<String, Value>,
    ) -> Result<CascadeOutcome> {
        self.clone()
            .execute_manifest_into(manifest.clone(), initial_context)
            .await
    }

    /// Owned-args variant — consumes `self` and `manifest` so the returned
    /// future owns both (no borrows) and is `Send + 'static`, making it safe to
    /// `tokio::spawn`. The bridge (`skill_executor.rs`) uses this for the
    /// GPUI→tokio handoff; the borrowed `execute_manifest` above delegates here.
    pub async fn execute_manifest_into(
        self,
        manifest: crate::bundle::BundleManifest,
        initial_context: HashMap<String, Value>,
    ) -> Result<CascadeOutcome> {
        // (K5) hard-enforce the capacity cap at the public entry point. The
        // advisory `tracing::warn!` in `StepGraph::new` remains for the flowdef
        // sub-cascade path; this is the operator-facing hard gate.
        if manifest.steps.len() > MAX_STEPS {
            return Err(TemplateError::Manifest(format!(
                "Manifest '{}' has {} steps — exceeds the capacity cap of {}. Remediation: split the manifest, or raise the cap in `step_graph::MAX_STEPS`.",
                manifest.id,
                manifest.steps.len(),
                MAX_STEPS,
            )));
        }
        let graph = StepGraph::new(&manifest.steps, manifest.convergence.max_iterations);
        let context = StepContext::new(initial_context);
        let budget = BudgetTracker::new(&manifest.gas, &manifest.rjoule);
        let convergence = ConvergenceTracker::new(&manifest.convergence);

        let infra = Infra {
            inference: self.inference.clone(),
            tools: self.tools.clone(),
            default_params: self.default_params.clone(),
            template_renderer: self.template_renderer.clone(),
            terminal_check: self.terminal_check.clone(),
            progress: self.progress.clone(),
            title: self.title.clone(),
        };

        let machine = StepMachine::new(
            graph,
            context,
            budget,
            convergence,
            manifest.error_handling.clone(),
        );
        let outcome = machine.run(infra).await?;

        // (K5) return the typed outcome directly — callers extract the final
        // result via `extract_final_step_result(&outcome)` (last_result_step),
        // not by scanning a string-keyed map.
        Ok(outcome)
    }
}

/// Deterministically extract the cascade's final result from the typed
/// outcome. (K5) replaced the `step_N_result` ordinal-keyed HashMap scan with
/// the machine-tracked `last_result_step` — deterministic by construction (no
/// randomized HashMap order), and correct for `populate`/`render`-final
/// manifests (their stored value is the result, not a fallback to the whole
/// context). Returns `Value::Null` when no step stored a result.
///
/// Applies `normalize_model_output` to strip `<thinking>` reasoning wrappers.
pub fn extract_final_step_result(outcome: &CascadeOutcome) -> Value {
    outcome
        .last_result_step
        .and_then(|step_id| outcome.context.result(step_id))
        .map(|r| normalize_model_output(&r.value).into_owned())
        .unwrap_or(Value::Null)
}

/// Strip `<thinking>` reasoning wrappers from model output.
///
/// Reasoning models (Qwen3, GLM-5.2, DeepSeek-R1) emit chain-of-thought
/// inside `<thinking>...</thinking>` tags before the final answer. Without
/// stripping, the tags pollute downstream step inputs and break JSON parsing.
/// (Wang 2026, arXiv:2603.02615v1, Appendix A.4).
///
/// Returns `Cow::Borrowed` when no stripping is needed (the common path),
/// `Cow::Owned` when tags were removed.
pub(crate) fn normalize_model_output(value: &Value) -> std::borrow::Cow<'_, Value> {
    let Value::String(s) = value else {
        return std::borrow::Cow::Borrowed(value);
    };
    if !s.contains("<thinking") && !s.contains("</thinking>") {
        return std::borrow::Cow::Borrowed(value);
    }
    let mut cleaned = s.to_string();
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
    cleaned = cleaned.replace("</thinking>", "");
    std::borrow::Cow::Owned(Value::String(cleaned))
}

/// Parse a JSON response from an inference call.
///
/// Attempts direct `serde_json::from_str`, then falls back to
/// `llm_json::extract_json_from_response` for brace-balanced extraction
/// (handles markdown code fences and injected JSON in reasoning preamble).
pub(crate) fn parse_json_response(text: &str, step_ordinal: u32) -> Result<Value> {
    if let Ok(v) = serde_json::from_str(text) {
        return Ok(v);
    }
    let extracted = llm_json::extract_json_from_response(text);
    serde_json::from_str(&extracted).map_err(|e| {
        TemplateError::Manifest(format!(
            "Step {}: Failed to parse JSON response: {}",
            step_ordinal, e
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::BudgetSnapshot;
    use crate::step_context::StepContext;
    use crate::step_graph::{ExitKind, StepId};
    use crate::step_machine::CascadeOutcome;
    use hkask_capability::{ToolFuture, ToolInfo};
    use hkask_types::InferenceError;
    use std::future::Future;
    use std::pin::Pin;

    /// Build a minimal `CascadeOutcome` for `extract_final_step_result` tests:
    /// the typed context + the machine-tracked `last_result_step` (what the
    /// machine sets in `apply_effect`). A zeroed `BudgetSnapshot` — irrelevant
    /// to final-result extraction.
    fn outcome_with_last(context: StepContext, last: Option<StepId>) -> CascadeOutcome {
        CascadeOutcome {
            context,
            iterations: 1,
            exit_kind: ExitKind::Converged,
            last_result_step: last,
            budget_snapshot: BudgetSnapshot {
                gas_used: 0,
                gas_cap: 0,
                gas_remaining: 0,
                gas_cost_per_iteration: 0,
                rjoule_used: 0.0,
                rjoule_cap: 0.0,
                rjoule_remaining: 0.0,
                rjoule_enabled: false,
            },
        }
    }

    /// Stub `InferencePort` — returns an error if called.
    struct StubInference;
    impl hkask_types::ports::inference_port::InferencePort for StubInference {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = std::result::Result<hkask_types::InferenceResult, InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async { Err(InferenceError::Generation("stub".into())) })
        }
    }

    /// Stub `ToolPort` whose `discover_tools` returns a configurable list.
    struct StubToolPort {
        discover: Vec<String>,
    }
    impl hkask_capability::ToolPort for StubToolPort {
        fn invoke<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
            _args: Value,
            _agent: hkask_types::WebID,
        ) -> ToolFuture<'a, std::result::Result<Value, hkask_capability::ToolPortError>> {
            Box::pin(async {
                Err(hkask_capability::ToolPortError::InvocationFailed(
                    "stub".into(),
                ))
            })
        }
        fn discover_tools<'a>(&'a self) -> ToolFuture<'a, Vec<String>> {
            let discover = self.discover.clone();
            Box::pin(async move { discover })
        }
        fn get_tool_info<'a>(&'a self, _tool_name: &'a str) -> ToolFuture<'a, Option<ToolInfo>> {
            Box::pin(async { None })
        }
    }

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

    #[tokio::test]
    async fn profile_gate_fires_when_terminal_check_says_enabled() {
        let executor = make_executor(vec![]).with_terminal_check(Arc::new(|| true));
        let manifest = crate::manifest_loader::load_manifest_from_yaml(GATE_MANIFEST)
            .expect("parse gate manifest");
        let result = executor.execute_manifest(&manifest, HashMap::new()).await;
        assert!(
            result.is_err(),
            "profile gate must fire when terminal is enabled"
        );
    }

    #[tokio::test]
    async fn profile_gate_passes_when_terminal_check_says_disabled() {
        let executor = make_executor(vec![]).with_terminal_check(Arc::new(|| false));
        let manifest = crate::manifest_loader::load_manifest_from_yaml(GATE_MANIFEST)
            .expect("parse gate manifest");
        let result = executor.execute_manifest(&manifest, HashMap::new()).await;
        assert!(
            result.is_ok(),
            "profile gate must pass when terminal is disabled"
        );
    }

    #[tokio::test]
    async fn profile_gate_fallback_uses_discover_tools_when_unwired() {
        let executor = make_executor(vec!["terminal".to_string()]);
        let manifest = crate::manifest_loader::load_manifest_from_yaml(GATE_MANIFEST)
            .expect("parse gate manifest");
        let result = executor.execute_manifest(&manifest, HashMap::new()).await;
        assert!(
            result.is_err(),
            "profile gate must fire when discover_tools returns terminal"
        );
    }

    // ── normalize_model_output: <thinking> tag stripping ──

    #[test]
    fn normalize_model_output_passes_through_non_string() {
        let value = serde_json::json!({"answer": 42});
        let out = normalize_model_output(&value);
        assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
        assert_eq!(*out, value);
    }

    #[test]
    fn normalize_model_output_borrows_clean_string() {
        let value = serde_json::json!("Answer: 5");
        let out = normalize_model_output(&value);
        assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
        assert_eq!(*out, value);
    }

    #[test]
    fn normalize_model_output_strips_paired_thinking_tags() {
        let value = serde_json::json!("<thinking>let me reason</thinking>Answer: 5");
        let out = normalize_model_output(&value);
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
        let value = serde_json::json!("Answer: 5</thinking>");
        let out = normalize_model_output(&value);
        assert_eq!(*out, serde_json::json!("Answer: 5"));
    }

    #[test]
    fn normalize_model_output_leaves_unclosed_opening_tag_untouched() {
        let value = serde_json::json!("<thinking without close Answer: 5");
        let out = normalize_model_output(&value);
        assert_eq!(*out, value);
    }

    #[test]
    fn extract_final_step_result_returns_last_result_step_value() {
        let mut ctx = StepContext::new(HashMap::new());
        ctx.store_result(0, 1, serde_json::json!("first"));
        ctx.store_result(2, 3, serde_json::json!("third"));
        ctx.store_result(1, 2, serde_json::json!("second"));
        // last_result_step = step_id 2 (ordinal 3), the last step to store in a
        // linear manifest. (K5) the ordinal-keyed HashMap scan is retired; the
        // machine-tracked last_result_step is deterministic by construction.
        let outcome = outcome_with_last(ctx, Some(2));
        assert_eq!(
            extract_final_step_result(&outcome),
            serde_json::json!("third")
        );
    }

    #[test]
    fn extract_final_step_result_strips_thinking_tags_from_result() {
        let mut ctx = StepContext::new(HashMap::new());
        ctx.store_result(
            0,
            1,
            Value::String(r#"<thinking>reasoning</thinking>{"answer": 5}"#.into()),
        );
        let outcome = outcome_with_last(ctx, Some(0));
        // `normalize_model_output` strips the `<thinking>` wrapper but leaves the
        // remainder a string (it does not parse it as JSON).
        assert_eq!(
            extract_final_step_result(&outcome),
            Value::String(r#"{"answer": 5}"#.into())
        );
    }

    #[test]
    fn extract_final_step_result_ignores_protocol_and_named_keys() {
        let mut ctx = StepContext::new(HashMap::new());
        ctx.store_result(0, 1, serde_json::json!("result"));
        ctx.insert_protocol("task".into(), serde_json::json!("user request"));
        ctx.store_named(1, 2, "populated", serde_json::json!("populated"));
        // last_result_step points at step_id 0 (ordinal 1), NOT step 2's named
        // result. extract returns last_result_step's value only.
        let outcome = outcome_with_last(ctx, Some(0));
        assert_eq!(
            extract_final_step_result(&outcome),
            serde_json::json!("result")
        );
    }

    #[test]
    fn extract_final_step_result_falls_back_to_null_when_no_step_results() {
        let ctx = StepContext::new(HashMap::new());
        let outcome = outcome_with_last(ctx, None);
        assert_eq!(extract_final_step_result(&outcome), Value::Null);
    }

    #[test]
    fn extract_feedback_phase_resolves_known_refs() {
        assert_eq!(
            extract_feedback_phase("sankey-flow/sankey-classify"),
            Some("Classify")
        );
        assert_eq!(
            extract_feedback_phase("bug-hunt/bug-hunt-gather"),
            Some("Gather")
        );
        assert_eq!(extract_feedback_phase("skill/draft-plan"), Some("Draft"));
        assert_eq!(
            extract_feedback_phase("skill/evaluate-result"),
            Some("Evaluate")
        );
        assert_eq!(
            extract_feedback_phase("skill/convergence-check"),
            Some("Convergence")
        );
        assert_eq!(
            extract_feedback_phase("skill/operator_feedback"),
            Some("OperatorFeedback")
        );
        assert_eq!(extract_feedback_phase("skill/write-report"), Some("Write"));
        assert_eq!(
            extract_feedback_phase("skill/outcome-track"),
            Some("Outcome")
        );
        assert_eq!(extract_feedback_phase("skill/unknown-phase"), None);
    }
}
