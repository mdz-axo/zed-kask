//! Manifest executor — public API and utilities.
//!
//! The heavy lifting lives in `step_machine.rs` (the deterministic interpreter)
//! and `step_actions.rs` (the per-action implementations). This module exposes
//! the `ManifestExecutor` builder + `execute_manifest` entry point that the
//! bridge (`kask_bridge::skill_executor`) calls, plus `extract_final_step_result`
//! and `normalize_model_output`.

use crate::convergence::ConvergenceTracker;
use crate::ports::Result;
use crate::step_context::StepContext;
use crate::step_graph::StepGraph;
use crate::step_machine::{CascadeOutcome, Infra, StepMachine};
use crate::template_renderer::TemplateRenderer;
use hkask_types::template::LLMParameters;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

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
    /// Short-term context: prior turns from the invoking thread. Injected
    /// into `Infra` for `execute_select` to prepend to inference calls.
    prior_messages: Vec<hkask_types::ports::inference_types::ChatMessage>,
    /// Long-term memory snippets. Injected into `Infra` for `execute_select`
    /// to prepend as a system message.
    memory_snippets: Vec<hkask_types::ports::memory_port::MemorySnippet>,
    /// Global inference concurrency limiter. `None` when not wired (tests).
    /// When `Some`, threaded into `Infra` so step actions can acquire permits
    /// before cloud inference / tool calls.
    concurrency_limiter: Option<Arc<hkask_types::concurrency::ConcurrencyLimiter>>,
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
            prior_messages: Vec::new(),
            memory_snippets: Vec::new(),
            concurrency_limiter: None,
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

    /// Wire short-term (prior thread messages) and long-term (memory
    /// snippets) context for the cascade. These are injected into `Infra`
    /// and prepended to each `execute_select` inference call so the model
    /// sees the conversation the skill was invoked from, plus salient
    /// long-term memory.
    #[must_use]
    pub fn with_cascade_context(
        mut self,
        prior_messages: Vec<hkask_types::ports::inference_types::ChatMessage>,
        memory_snippets: Vec<hkask_types::ports::memory_port::MemorySnippet>,
    ) -> Self {
        self.prior_messages = prior_messages;
        self.memory_snippets = memory_snippets;
        self
    }

    /// Wire the global inference concurrency limiter. Threaded into `Infra`
    /// so step actions (`execute_parallel`, `execute_tool_invoke`,
    /// `execute_select`) can acquire permits before issuing cloud inference
    /// or tool calls. `None` (the default) means no gating — used by tests
    /// and pre-startup paths.
    #[must_use]
    pub fn with_concurrency_limiter(
        mut self,
        limiter: Arc<hkask_types::concurrency::ConcurrencyLimiter>,
    ) -> Self {
        self.concurrency_limiter = Some(limiter);
        self
    }

    /// Execute the full manifest cascade.
    ///
    /// Builds a `StepGraph` from the manifest's steps, a `StepContext` from
    /// the initial context, and a `StepMachine` to drive the cascade. The
    /// machine's dispatch loop runs each step's action via `step_actions.rs`,
    /// checks convergence in one place (the `Reenter` arm), and checks budget
    /// in one place. Returns the typed `CascadeOutcome`; callers extract the
    /// final result via `extract_final_step_result`.
    ///
    /// Consumes `self` and `manifest` so the returned future owns both (no
    /// borrows) and is `Send + 'static`, making it safe to `tokio::spawn`.
    /// The bridge (`skill_executor.rs`) relies on this for the GPUI→tokio
    /// handoff. (The borrowed `execute_manifest` wrapper was removed — it
    /// had no production callers; tests clone the executor + manifest.)
    pub async fn execute_manifest_into(
        self,
        manifest: crate::bundle::BundleManifest,
        initial_context: HashMap<String, Value>,
    ) -> Result<CascadeOutcome> {
        // (K5) hard-enforce the capacity cap at the public entry point. The
        // advisory `tracing::warn!` in `StepGraph::new` remains for the
        // diagnostic; `check_step_cap` is the operator-facing hard gate,
        // shared with the flowdef/parallel sub-cascade paths so the gate
        // fires in all three orchestration paths.
        crate::step_graph::check_step_cap(
            manifest.steps.len(),
            &format!("manifest '{}'", manifest.id),
        )?;
        let graph = StepGraph::new(&manifest.steps, manifest.convergence.max_iterations);
        let context = StepContext::new(initial_context);
        let convergence = ConvergenceTracker::new(
            manifest.convergence.max_iterations,
            manifest.convergence.min_iterations,
            manifest.convergence.threshold,
        );

        let infra = Infra {
            inference: self.inference.clone(),
            tools: self.tools.clone(),
            default_params: self.default_params.clone(),
            template_renderer: self.template_renderer.clone(),
            terminal_check: self.terminal_check.clone(),
            progress: self.progress.clone(),
            title: self.title.clone(),
            prior_messages: self.prior_messages.clone(),
            memory_snippets: self.memory_snippets.clone(),
            concurrency_limiter: self.concurrency_limiter.clone(),
        };

        let machine = StepMachine::new(
            graph,
            context,
            convergence,
            manifest.error_handling.clone(),
            manifest.id.clone(),
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
/// context).
///
/// Applies `normalize_model_output` to strip `<thinking>` reasoning wrappers.
///
/// **Primary rule** (`last_result_step` set): the last select/render/execute
/// step's stored value is the result. Compute steps store via `StoredNamed`
/// (suffix `"compute"`) and deliberately do NOT set `last_result_step` — their
/// output is an auxiliary value (convergence signal, validation list), not the
/// skill's product. This fixes the 49-of-~58 registry manifests that ended in
/// `…select → compute(convergence signal) → loop` and returned bare numbers
/// (e.g. `"0"`) instead of the select step's report.
///
/// **Fallback** (`last_result_step` is `None`): no select/render/execute step
/// stored a primary result — surface the highest-ordinal step result. This
/// narrowly resurrects the pre-K5 max-ordinal scan for the `None` case only:
/// compute-only manifests (bench manifests, test sub-branches without a
/// trailing render step) still surface their compute output instead of
/// collapsing to `Null`. `store_named` keeps compute results in the `results`
/// map, so the scan sees them. When `last_result_step` IS set, the primary
/// rule above wins — a render step following a compute step clobbers the
/// compute result as the cascade's final output.
pub fn extract_final_step_result(outcome: &CascadeOutcome) -> Value {
    if let Some(step_id) = outcome.last_result_step {
        if let Some(result) = outcome.context.result(step_id) {
            return normalize_model_output(&result.value).into_owned();
        }
    }
    // Fallback (compute-only cascades + parallel sub-branches): no
    // select/render/execute step stored a primary result — surface the
    // highest-ordinal step result. `store_named` keeps compute results in
    // `results`, so the scan sees them.
    outcome
        .context
        .results_iter()
        .max_by_key(|(_, result)| result.ordinal)
        .map(|(_, result)| normalize_model_output(&result.value).into_owned())
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

