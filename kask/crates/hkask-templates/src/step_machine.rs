//! Step machine — the deterministic interpreter that replaces `run_cascade`.
//!
//! The machine owns three things: a program counter (`StepId`), an iteration
//! counter (`u32`), and a budget tracker. It loops: fetch step → dispatch via
//! the step's action → apply the effect → check exits → advance the PC.
//! There is no `match` arm in here — dispatch is trait-based via `StepAction`.
//!
//! Convergence is checked in exactly one place: the `Reenter` arm. Budget is
//! checked in exactly one place: after applying each effect. The matryoshka
//! guard is a property of `FlowDefAction::execute` (the only action that
//! recurses), not a `depth` parameter threaded through every call.
//!
//! This replaces the 720-line `run_cascade` that simultaneously owned step
//! dispatch, iteration counting, step-index bookkeeping, convergence checking,
//! budget checking, prev-step snapshotting, profile enforcement, feedback-span
//! emission, and matryoshka recursion — causing the five control-flow bugs
//! documented in `.rules`.

use crate::budget::BudgetTracker;
use crate::concurrency::ConcurrencyLimiter;
use crate::convergence::{ConvergenceStatus, ConvergenceTracker};
use crate::ports::Result;
use crate::step_context::StepContext;
use crate::step_graph::{ControlFlow, ENTRY, ExitKind, StepGraph, StepId};
use crate::template_renderer::TemplateRenderer;
use hkask_capability::ToolPort;
use hkask_types::ports::inference_port::InferencePort;
use hkask_types::ports::inference_types::ChatMessage;
use hkask_types::ports::memory_port::MemorySnippet;
use hkask_types::template::LLMParameters;
use std::sync::Arc;

/// Infrastructure ports and callbacks passed to each `StepAction::execute`.
/// Replaces the 10+ fields on `ManifestExecutor` that were accessed via
/// `&self` inside the 720-line `run_cascade`.
#[derive(Clone)]
pub struct Infra {
    pub inference: Arc<dyn InferencePort>,
    pub tools: Arc<dyn ToolPort>,
    pub default_params: LLMParameters,
    pub template_renderer: TemplateRenderer,
    pub terminal_check: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    pub progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    pub title: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    /// Short-term context: prior turns from the invoking thread, role-tagged.
    /// Prepended to each `execute_select` inference call so the model sees
    /// the conversation the skill was invoked from. Empty when the cascade
    /// is invoked outside a thread (CLI) or when `cascade_short_term_turns`
    /// is 0.
    pub prior_messages: Vec<ChatMessage>,
    /// Long-term memory snippets, gathered by the `CascadeContextProvider`.
    /// Injected as a system message prepended to each `execute_select`
    /// inference call. Empty when no stores are available or no chunks
    /// exceed the saliency floor.
    pub memory_snippets: Vec<MemorySnippet>,
    /// Global inference concurrency limiter — process-wide, shared across
    /// all consumers (skill cascades, corpus OCR, MCP tool calls). `None`
    /// when the limiter is not wired (tests, pre-startup); callers must
    /// skip gating when `None`. When `Some`, every cloud inference and tool
    /// call acquires a permit before issuing and calls `on_success` /
    /// `on_throttle` after completion.
    pub concurrency_limiter: Option<Arc<ConcurrencyLimiter>>,
}

/// The deterministic step machine. Created per cascade run.
pub struct StepMachine {
    pub(crate) graph: StepGraph,
    pub(crate) context: StepContext,
    pub(crate) budget: BudgetTracker,
    pub(crate) convergence: ConvergenceTracker,
    /// Per-step error handling policy (on_timeout, max_retries,
    /// retry_backoff_seconds). Read by `run_pass` to decide whether a
    /// `TemplateError::Timeout` is retried or propagated.
    pub(crate) error_handling: crate::bundle::config::ErrorHandlingConfig,
    /// The manifest ID (skill name). Used by `on_failure: report` to identify
    /// the skill in curator skill-use-issue reports.
    pub(crate) manifest_id: String,
    /// Resume text from the `on_failure` config that triggered the cascade
    /// exit. Populated by `dispatch_with_retry` when an `on_failure` action
    /// (halt/escalate/report) produces `Effect::Exit(Escalated)`. Surfaced to
    /// the operator via `CascadeOutcome.resume_text` so they can distinguish
    /// "escalated by on_failure" from "escalated by the model" and see the
    /// author's resume instruction.
    pub(crate) resume_text: Option<String>,
    /// Program counter — which step we're executing.
    pub(crate) pc: StepId,
    /// Iteration counter — how many times we've re-entered the cascade.
    pub(crate) iteration: u32,
    /// The highest `StepId` that stored a result during this cascade.
    /// Used to extract the final result in O(1).
    pub(crate) last_result_step: Option<StepId>,
    /// Matryoshka recursion depth. Only incremented by `FlowDefAction`.
    pub(crate) depth: u8,
    /// Tool calls made during this cascade (execute steps only). Each entry
    /// is `{"tool": "server/tool_name", "ok": true/false}` — same shape as
    /// `LocalDelegateResult.tool_calls`. Surfaced on `CascadeOutcome` so the
    /// skill executor can run grounding checks (Phase 5: skill cascade
    /// grounding). Sub-cascade tool calls (from `execute_parallel` branches)
    /// are tracked by their own `StepMachine`, not merged here.
    pub(crate) tool_calls: Vec<serde_json::Value>,
}

/// The outcome of running a cascade to completion.
#[derive(Debug)]
pub struct CascadeOutcome {
    pub context: StepContext,
    pub iterations: u32,
    pub exit_kind: ExitKind,
    pub last_result_step: Option<StepId>,
    pub budget_snapshot: crate::budget::BudgetSnapshot,
    /// Resume instruction from the `on_failure` config that triggered the
    /// exit. `Some` only when the exit was caused by an `on_failure` action
    /// (halt/escalate/report); `None` for normal convergence, max-out, or
    /// model-initiated exits. Operators read this to understand why the
    /// cascade escalated and how to resume.
    pub resume_text: Option<String>,
    /// Tool calls made during this cascade (execute steps only). Each entry
    /// is `{"tool": "server/tool_name", "ok": true/false}` — same shape as
    /// `LocalDelegateResult.tool_calls`. Surfaced for grounding enforcement
    /// (Phase 5: skill cascade grounding). Empty when no execute steps ran
    /// or all tool calls failed before recording.
    pub tool_calls: Vec<serde_json::Value>,
}

impl StepMachine {
    /// Create a new machine for the given graph and context.
    pub fn new(
        graph: StepGraph,
        context: StepContext,
        budget: BudgetTracker,
        convergence: ConvergenceTracker,
        error_handling: crate::bundle::config::ErrorHandlingConfig,
        manifest_id: String,
    ) -> Self {
        Self {
            graph,
            context,
            budget,
            convergence,
            error_handling,
            manifest_id,
            pc: ENTRY,
            iteration: 0,
            last_result_step: None,
            depth: 0,
            resume_text: None,
            tool_calls: Vec::new(),
        }
    }

    /// Run the cascade to completion.
    pub async fn run(mut self, infra: Infra) -> Result<CascadeOutcome> {
        // Matryoshka guard — only FlowDefAction recurses, but the guard is
        // checked here so it's in one place, not threaded through every call.
        if self.depth > hkask_capability::SYSTEM_MAX_RECURSION {
            return Err(crate::ports::TemplateError::Manifest(format!(
                "Matryoshka depth limit ({}) exceeded",
                hkask_capability::SYSTEM_MAX_RECURSION
            )));
        }

        // (K1) inputs are read directly via `StepContext::lookup`/`Serialize` —
        // no merge into a parallel map. Inject initial convergence context
        // (status: running, iteration 0).
        let snap = self.budget.snapshot();
        self.convergence.inject_running(
            &mut self.context,
            0,
            snap.gas_used,
            snap.gas_cap,
            snap.rjoule_used,
            snap.rjoule_cap,
        );

        let exit_kind = loop {
            // Start of a new iteration.
            self.iteration += 1;
            let snap = self.budget.snapshot();
            self.convergence.inject_running(
                &mut self.context,
                self.iteration,
                snap.gas_used,
                snap.gas_cap,
                snap.rjoule_used,
                snap.rjoule_cap,
            );

            // Emit step label to the title callback and a step-boundary
            // marker to the thinking trace.
            let total = self.graph.len();
            let node = self.graph.step(self.pc);
            let desc = if node.description.is_empty() {
                String::new()
            } else {
                format!(" — {}", node.description)
            };
            if let Some(ref title) = infra.title {
                if self.iteration > 1 {
                    title(&format!(
                        "Iteration {}, step {}/{}: {}{}",
                        self.iteration,
                        self.pc + 1,
                        total,
                        node.action,
                        desc
                    ));
                } else {
                    title(&format!(
                        "Step {}/{}: {}{}",
                        self.pc + 1,
                        total,
                        node.action,
                        desc
                    ));
                }
            }
            // Also emit a step-boundary marker to the thinking trace so
            // the user can see which step is running even when the model
            // doesn't produce reasoning deltas (e.g. non-thinking models).
            if let Some(ref progress) = infra.progress {
                let marker = if self.iteration > 1 {
                    format!(
                        "\n\n---\n**Iteration {}, Step {}/{}: {}{}**\n\n",
                        self.iteration,
                        self.pc + 1,
                        total,
                        node.action,
                        desc
                    )
                } else {
                    format!(
                        "\n\n---\n**Step {}/{}: {}{}**\n\n",
                        self.pc + 1,
                        total,
                        node.action,
                        desc
                    )
                };
                progress(&marker);
            }

            // Execute steps until we hit a Reenter, Exit, or the end of the graph.
            // Clone `infra` by value into `run_pass` so the resulting future owns
            // its `Infra` (no `&Infra` borrow crossing `.await`). rustc's HRTB
            // `Send` check rejects futures that hold `&Infra` across awaits when
            // the outer future is `tokio::spawn`ed; owning the value sidesteps it.
            // `run` keeps its own `infra` for the title/progress callbacks above.
            match self.run_pass(infra.clone()).await? {
                PassResult::Reenter(target) => {
                    // Convergence check — exactly one place, not four.
                    self.context.read_convergence_signal();
                    self.convergence.push_cycle_from_context(&self.context);

                    let max_iterations = self.convergence.max_iterations();
                    if self.iteration >= max_iterations {
                        self.finalize(ExitKind::MaxedOut, "energy_spent");
                        break ExitKind::MaxedOut;
                    }

                    if self.convergence.check_met(&self.context, self.iteration) {
                        self.finalize(ExitKind::Converged, "quality_met");
                        break ExitKind::Converged;
                    }

                    // Budget check — exactly one place.
                    if self.budget.check_exhausted(self.iteration).is_some() {
                        self.finalize(ExitKind::MaxedOut, "energy_spent");
                        break ExitKind::MaxedOut;
                    }

                    // Snapshot prev results for Self-Refine loops.
                    self.context.snapshot_prev();
                    self.pc = target;
                }
                PassResult::Exit(kind) => {
                    let reason = match kind {
                        ExitKind::Converged => "quality_met",
                        ExitKind::MaxedOut => "energy_spent",
                        ExitKind::Escalated => "escalated",
                    };
                    self.finalize(kind, reason);
                    break kind;
                }
            }
        };

        let budget_snapshot = self.budget.snapshot();
        Ok(CascadeOutcome {
            context: self.context,
            iterations: self.iteration,
            exit_kind,
            last_result_step: self.last_result_step,
            budget_snapshot,
            resume_text: self.resume_text.take(),
            tool_calls: std::mem::take(&mut self.tool_calls),
        })
    }

    /// Run one pass through the graph — from the current PC until we hit a
    /// `Reenter`, `Exit`, or run out of steps.
    async fn run_pass(&mut self, infra: Infra) -> Result<PassResult> {
        loop {
            // Clone the node to avoid holding an immutable borrow of `self.graph`
            // across the mutable `dispatch_action` call. After K4 the heavy
            // fields are `Arc`-backed, so this clone is a shallow refcount bump,
            // not a deep String/Value copy — the original justification (restructure
            // dispatch to take borrowed fields) is no longer worth the surface.
            let node = self.graph.step(self.pc).clone();

            // Evaluate step condition — skip if false.
            if let Some(ref cond) = node.condition {
                if !self.evaluate_condition(cond)? {
                    // Condition false — skip to next step.
                    match node.on_complete {
                        ControlFlow::Fallthrough => {
                            self.pc += 1;
                            continue;
                        }
                        ControlFlow::Reenter(target) => return Ok(PassResult::Reenter(target)),
                        ControlFlow::Exit(kind) => return Ok(PassResult::Exit(kind)),
                        ControlFlow::Jump(target) => {
                            self.pc = target;
                            continue;
                        }
                    }
                }
            }

            // Profile enforcement (proposer/evaluator separation).
            if let Some(ref profile_name) = node.profile {
                if self.is_terminal_available(infra.clone()).await {
                    return Err(crate::ports::TemplateError::Manifest(format!(
                        "Step {} declares profile '{}' but the `terminal` tool is available. \
                         This violates proposer/evaluator separation.",
                        node.ordinal, profile_name
                    )));
                }
            }

            // Dispatch the step's action, with retry on timeout if the
            // manifest's `error_handling` policy opts in. Only timeouts are
            // retried — other errors (validation, not-found, render) propagate
            // immediately because retrying a deterministic failure is wasteful.
            // The retry uses the same timeout (the manifest's per-step
            // `timeout_seconds`); a longer timeout on retry would require a
            // separate field, which the schema doesn't have today.
            let effect = self
                .dispatch_with_retry(node.clone(), infra.clone())
                .await?;

            // Determine control flow: merge the effect with the node's static flow.
            // Done BEFORE apply_effect because apply_effect takes effect by value.
            let flow = self.merge_control_flow(&node, &effect);

            // Apply the effect to the context and budget.
            self.apply_effect(effect, &node)?;

            // Emit feedback span for select steps.
            if node.action.as_ref() == "select"
                && let Some(ref template_ref) = node.template_ref
                && let Some(phase) = crate::executor::extract_feedback_phase(template_ref)
            {
                tracing::info!(
                    target: "reg.skill.cascade.step_executed",
                    iteration = self.iteration,
                    step = node.ordinal,
                    phase = phase,
                    "REG"
                );
            }

            match flow {
                ControlFlow::Fallthrough => {
                    self.pc += 1;
                }
                ControlFlow::Jump(target) => {
                    self.pc = target;
                }
                ControlFlow::Reenter(target) => {
                    return Ok(PassResult::Reenter(target));
                }
                ControlFlow::Exit(kind) => {
                    return Ok(PassResult::Exit(kind));
                }
            }
        }
    }

    /// Dispatch a step's action to the appropriate handler. This is the only
    /// place that matches on `node.action` — each arm is a small function call,
    /// not a 100-line block.
    async fn dispatch_action(
        &mut self,
        node: crate::step_graph::StepNode,
        infra: Infra,
    ) -> Result<crate::step_actions::Effect> {
        // Extract the action into an owned `String` before matching. The match
        // would otherwise hold an immutable borrow of `node` (via
        // `node.action.as_ref()`) across the arms, preventing the `node` move
        // into the async arms below.
        let action = node.action.to_string();
        match action.as_str() {
            "abort" => Ok(crate::step_actions::Effect::Exit(ExitKind::Converged)),
            "escalate" => {
                let reason = node.description.clone();
                tracing::info!(
                    target: "reg.skill.convergence.escalated",
                    iteration = self.iteration,
                    reason = %reason,
                    "REG"
                );
                Ok(crate::step_actions::Effect::Exit(ExitKind::Escalated))
            }
            // Sync arms: borrow `node`/`infra` — no await, no move needed.
            "choice" => self.execute_choice(&node),
            "loop" => self.execute_loop(&node, &infra),
            // Async arms: move `node`/`infra` by value so each future owns them
            // and is `Send + 'static` under `tokio::spawn`.
            "select" => self.execute_select(node, infra).await,
            "populate" => self.execute_populate(node, infra).await,
            "compute" => self.execute_compute(node, infra).await,
            "render" => self.execute_render(node, infra).await,
            "execute" | "feedback" | "validate" | "retrieve" => {
                self.execute_tool_invoke(node, infra).await
            }
            "flowdef" => self.execute_flowdef(node, infra).await,
            "parallel" => self.execute_parallel(node, infra).await,
            "gate" => self.execute_gate(node, infra).await,
            other => Err(crate::ports::TemplateError::Manifest(format!(
                "Unknown manifest step action: '{other}'"
            ))),
        }
    }

    /// Dispatch a step's action, retrying on `TemplateError::Timeout` if the
    /// manifest's `error_handling` policy opts in (`on_timeout == "retry"` and
    /// `max_retries > 0`). Non-timeout errors propagate immediately — retrying
    /// a validation or not-found error is wasteful. The retry uses the same
    /// per-step timeout; a longer retry timeout would need a separate schema
    /// field. Backoff is `retry_backoff_seconds` (default 1s).
    ///
    /// This is the enforcement point for `ErrorHandlingConfig.on_timeout` /
    /// `max_retries` / `retry_backoff_seconds` — previously parsed but never
    /// read (an advertised invariant with no enforcement point).
    ///
    /// Per-step `on_failure` config: when a step (any action, not just gates)
    /// fails after retries are exhausted, the `on_failure` config is checked.
    /// `action: halt` or `action: escalate` produces `Effect::Exit(Escalated)`
    /// with the `resume` text. This is the enforcement point for per-step
    /// `on_failure` — previously only gates checked it (an advertised
    /// invariant with no enforcement point for execute/select/compute steps).
    async fn dispatch_with_retry(
        &mut self,
        node: crate::step_graph::StepNode,
        infra: Infra,
    ) -> Result<crate::step_actions::Effect> {
        let max_retries = if self.error_handling.on_timeout == "retry"
            || self.error_handling.on_parse_failure == "retry"
        {
            self.error_handling.max_retries
        } else {
            0
        };

        let mut attempt: u32 = 0;
        loop {
            match self.dispatch_action(node.clone(), infra.clone()).await {
                Ok(effect) => return Ok(effect),
                Err(crate::ports::TemplateError::Timeout {
                    step_ordinal,
                    elapsed_seconds,
                }) if attempt < max_retries && self.error_handling.on_timeout == "retry" => {
                    attempt += 1;
                    tracing::warn!(
                        target: "reg.skill.cascade.timeout_retry",
                        step = step_ordinal,
                        attempt,
                        max_retries,
                        elapsed_seconds,
                        backoff_seconds = self.error_handling.retry_backoff_seconds,
                        failure_mode = "timeout",
                        "Step {} timed out after {}s — retrying (attempt {}/{}) after {}s backoff",
                        step_ordinal,
                        elapsed_seconds,
                        attempt,
                        max_retries,
                        self.error_handling.retry_backoff_seconds,
                    );
                    if self.error_handling.retry_backoff_seconds > 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(
                            self.error_handling.retry_backoff_seconds as u64,
                        ))
                        .await;
                    }
                    continue;
                }
                Err(crate::ports::TemplateError::ParseFailure {
                    step_ordinal,
                    detail: _,
                }) if attempt < max_retries && self.error_handling.on_parse_failure == "retry" => {
                    attempt += 1;
                    tracing::warn!(
                        target: "reg.skill.cascade.parse_failure_retry",
                        step = step_ordinal,
                        attempt,
                        max_retries,
                        backoff_seconds = self.error_handling.retry_backoff_seconds,
                        failure_mode = "parse_failure",
                        "Step {} parse failure — retrying (attempt {}/{}) after {}s backoff",
                        step_ordinal,
                        attempt,
                        max_retries,
                        self.error_handling.retry_backoff_seconds,
                    );
                    if self.error_handling.retry_backoff_seconds > 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(
                            self.error_handling.retry_backoff_seconds as u64,
                        ))
                        .await;
                    }
                    continue;
                }
                Err(e) => {
                    // Per-step on_failure: check for halt/escalate/report after
                    // retries are exhausted. This is the enforcement point
                    // for OnFailureConfig on execute/select/compute steps —
                    // previously only gates checked it.
                    if let Some(ref on_failure) = node.on_failure {
                        match on_failure.action.as_str() {
                            "report" => {
                                // Co-evolution Phase 2, Loop 2: report the
                                // failure to the curator before escalating.
                                // The report is best-effort — if the curator
                                // MCP server is down, the escalation still
                                // proceeds (the resume text is logged).
                                let tool_name = node.mcp.as_deref().unwrap_or("").to_string();
                                let report_input = serde_json::json!({
                                    "skill_name": self.manifest_id,
                                    "tool_name": tool_name,
                                    "step_ordinal": node.ordinal,
                                    "error": format!("{e}"),
                                    "tool_input": null,
                                    "failure_type": null,
                                });
                                // Best-effort: log if the report fails.
                                // Clone `infra.tools` into a standalone local
                                // before the await — rustc's HRTB `Send` check
                                // rejects a `&infra.tools` borrow held across an
                                // `.await` under `tokio::spawn`.
                                let tools = infra.tools.clone();
                                if let Err(report_err) = crate::step_actions::invoke_tool(
                                    tools,
                                    "curator_report_skill_use_issue".to_string(),
                                    report_input,
                                )
                                .await
                                {
                                    tracing::warn!(
                                        target: "reg.skill.cascade.skill_use_issue_report_failed",
                                        step = node.ordinal,
                                        error = %report_err,
                                        "Failed to report skill-use issue to curator — escalation proceeds without report",
                                    );
                                }
                                tracing::warn!(
                                    target: "reg.skill.cascade.step_failed",
                                    step = node.ordinal,
                                    action = %on_failure.action,
                                    error = %e,
                                    resume = %on_failure.resume,
                                    "Step {} failed — on_failure report+escalate",
                                    node.ordinal
                                );
                                self.resume_text = Some(on_failure.resume.clone());
                                return Ok(crate::step_actions::Effect::Exit(
                                    crate::step_graph::ExitKind::Escalated,
                                ));
                            }
                            "halt" | "escalate" => {
                                tracing::warn!(
                                    target: "reg.skill.cascade.step_failed",
                                    step = node.ordinal,
                                    action = %on_failure.action,
                                    error = %e,
                                    resume = %on_failure.resume,
                                    "Step {} failed — on_failure config halts the cascade",
                                    node.ordinal
                                );
                                self.resume_text = Some(on_failure.resume.clone());
                                return Ok(crate::step_actions::Effect::Exit(
                                    crate::step_graph::ExitKind::Escalated,
                                ));
                            }
                            _ => {}
                        }
                    }
                    return Err(e);
                }
            }
        }
    }

    /// Apply an effect to the context and budget.
    fn apply_effect(
        &mut self,
        effect: crate::step_actions::Effect,
        node: &crate::step_graph::StepNode,
    ) -> Result<()> {
        match effect {
            crate::step_actions::Effect::Stored { step_id, value } => {
                self.context.store_result(step_id, node.ordinal, value);
                self.last_result_step = Some(step_id);
            }
            crate::step_actions::Effect::StoredNamed {
                step_id,
                suffix,
                value,
            } => {
                self.context
                    .store_named(step_id, node.ordinal, &suffix, value);
                self.last_result_step = Some(step_id);
            }
            crate::step_actions::Effect::ConsumedGas(amount) => {
                // Gas is charged per iteration, not per step — the budget
                // tracker's `charge_iteration` handles this. This effect is
                // for per-step gas (e.g. flowdef sub-cascade consumption).
                let _ = amount; // handled by charge_iteration in the pass loop
            }
            crate::step_actions::Effect::ConsumedRJoule(cost) => {
                self.budget.charge_rjoule(cost);
            }
            crate::step_actions::Effect::NoOp
            | crate::step_actions::Effect::Jump(_)
            | crate::step_actions::Effect::Reenter(_)
            | crate::step_actions::Effect::Exit(_) => {
                // Control-flow effects — handled by merge_control_flow.
            }
        }
        Ok(())
    }

    /// Merge the effect's dynamic control flow with the node's static flow.
    /// The effect wins if it specifies a jump/re-enter/exit; otherwise the
    /// node's static flow is used.
    fn merge_control_flow(
        &self,
        node: &crate::step_graph::StepNode,
        effect: &crate::step_actions::Effect,
    ) -> ControlFlow {
        match effect {
            crate::step_actions::Effect::Jump(target) => ControlFlow::Jump(*target),
            crate::step_actions::Effect::Reenter(target) => ControlFlow::Reenter(*target),
            crate::step_actions::Effect::Exit(kind) => ControlFlow::Exit(*kind),
            _ => {
                // Check branching map — if the step has a `branching` map and
                // the result has a routing field, jump to the target.
                if let Some(ref branching) = node.branching {
                    let field_name = node.branching_field.as_deref().unwrap_or("routing");
                    let result_key = format!("step_{}_result", node.ordinal);
                    if let Some(routing) = self
                        .context
                        .lookup(&result_key)
                        .and_then(|v| v.get(field_name))
                        .and_then(|v| v.as_str())
                    {
                        if let Some(&target_ordinal) = branching.get(routing) {
                            if let Some(target_id) = self.graph.find(target_ordinal) {
                                return ControlFlow::Jump(target_id);
                            }
                        }
                    }
                }
                node.on_complete
            }
        }
    }

    /// Evaluate a step condition. Renders Jinja expressions first, then
    /// evaluates the truthy/comparison expression.
    fn evaluate_condition(&self, cond: &str) -> Result<bool> {
        // (K1) the old `__renderer__` probe was dead (the key was never set, so
        // both arms produced `cond.to_string()`); dropped. `cond` is evaluated
        // directly against the typed context via `ContextLookup`.
        Ok(crate::condition::evaluate_step_condition(
            cond,
            &self.context,
        ))
    }

    /// Check whether the `terminal` tool is available (for profile enforcement).
    async fn is_terminal_available(&self, infra: Infra) -> bool {
        match &infra.terminal_check {
            Some(check) => check(),
            None => {
                // Clone `infra.tools` into a standalone local before the await —
                // `discover_tools` returns a future borrowing `&self`, and
                // rustc's HRTB `Send` check rejects a `&infra.tools` borrow
                // held across `.await` under `tokio::spawn`.
                let tools = infra.tools.clone();
                let available = tools.discover_tools().await;
                available.iter().any(|t| t == "terminal")
            }
        }
    }

    /// Finalize the convergence report at cascade exit. Called once, not 11 times.
    fn finalize(&mut self, kind: ExitKind, reason: &str) {
        let status = match kind {
            ExitKind::Converged => ConvergenceStatus::Converged,
            ExitKind::MaxedOut => ConvergenceStatus::MaxedOut,
            ExitKind::Escalated => ConvergenceStatus::Escalated,
        };
        let snap = self.budget.snapshot();
        self.convergence.finalize_report(
            &mut self.context,
            status,
            reason,
            self.iteration,
            snap.gas_used,
            snap.gas_cap,
            snap.rjoule_used,
            snap.rjoule_cap,
        );
    }
}

/// What happened when running a pass through the graph.
enum PassResult {
    /// The pass hit a `Reenter` — the machine should check convergence and
    /// budget, then re-enter from `target`.
    Reenter(StepId),
    /// The pass hit an `Exit` — the cascade is done.
    Exit(ExitKind),
}

/// Classify a `TemplateError` into a `failure_mode` string for tracing.
/// This enables operators to filter and aggregate skill failures by mode
/// (e.g. `failure_mode=timeout`, `failure_mode=parse_failure`,
/// `failure_mode=tool_not_found`) in log analysis tools.
#[allow(dead_code)] // in-process work — not yet wired
fn classify_failure_mode(error: &crate::ports::TemplateError) -> &'static str {
    match error {
        crate::ports::TemplateError::Timeout { .. } => "timeout",
        crate::ports::TemplateError::ParseFailure { .. } => "parse_failure",
        crate::ports::TemplateError::NotFound(_) => "tool_not_found",
        crate::ports::TemplateError::Manifest(_) => "manifest_error",
        crate::ports::TemplateError::Mcp(_) => "mcp_error",
        crate::ports::TemplateError::Render(_) => "render_error",
        crate::ports::TemplateError::Inference(_) => "inference_error",
        crate::ports::TemplateError::Database(_) => "database_error",
        crate::ports::TemplateError::Validation(_) => "validation_error",
        crate::ports::TemplateError::PathTraversal(_) => "path_traversal",
        crate::ports::TemplateError::SandboxViolation(_) => "sandbox_violation",
        crate::ports::TemplateError::SkillLoad { .. } => "skill_load_error",
        crate::ports::TemplateError::Frontmatter { .. } => "frontmatter_error",
    }
}
