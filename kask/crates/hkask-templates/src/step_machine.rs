//! Step machine — minimal cascade interpreter.
//!
//! Fetch step → dispatch → apply effect → check convergence → advance.
//! Controls: timeouts (per step), max_iterations (convergence), depth (recursion).

use crate::convergence::{ConvergenceStatus, ConvergenceTracker};
use crate::ports::{Result, TemplateError};
use crate::step_context::StepContext;
use crate::step_graph::{ControlFlow, ENTRY, ExitKind, StepGraph, StepId};
use crate::template_renderer::TemplateRenderer;
use hkask_capability::ToolPort;
use hkask_types::concurrency::ConcurrencyLimiter;
use hkask_types::ports::inference_port::InferencePort;
use hkask_types::ports::inference_types::ChatMessage;
use hkask_types::ports::memory_port::MemorySnippet;
use hkask_types::template::LLMParameters;
use std::sync::Arc;

#[derive(Clone)]
pub struct Infra {
    pub inference: Arc<dyn InferencePort>,
    pub tools: Arc<dyn ToolPort>,
    pub default_params: LLMParameters,
    pub template_renderer: TemplateRenderer,
    pub terminal_check: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    pub progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    pub title: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    pub prior_messages: Vec<ChatMessage>,
    pub memory_snippets: Vec<MemorySnippet>,
    pub concurrency_limiter: Option<Arc<ConcurrencyLimiter>>,
}

pub struct StepMachine {
    pub(crate) graph: StepGraph,
    pub(crate) context: StepContext,
    pub(crate) convergence: ConvergenceTracker,
    pub(crate) error_handling: crate::bundle::config::ErrorHandlingConfig,
    pub(crate) manifest_id: String,
    pub(crate) resume_text: Option<String>,
    pub(crate) pc: StepId,
    pub(crate) iteration: u32,
    pub(crate) last_result_step: Option<StepId>,
    pub(crate) depth: u8,
    pub(crate) tool_calls: Vec<serde_json::Value>,
    pub(crate) discovered_tools: Option<Vec<String>>,
}

#[derive(Debug)]
pub struct CascadeOutcome {
    pub context: StepContext,
    pub iterations: u32,
    pub exit_kind: ExitKind,
    pub last_result_step: Option<StepId>,
    pub resume_text: Option<String>,
    pub tool_calls: Vec<serde_json::Value>,
}

impl StepMachine {
    pub fn new(
        graph: StepGraph,
        context: StepContext,
        convergence: ConvergenceTracker,
        error_handling: crate::bundle::config::ErrorHandlingConfig,
        manifest_id: String,
    ) -> Self {
        Self {
            graph,
            context,
            convergence,
            error_handling,
            manifest_id,
            pc: ENTRY,
            iteration: 0,
            last_result_step: None,
            depth: 0,
            resume_text: None,
            tool_calls: Vec::new(),
            discovered_tools: None,
        }
    }

    pub async fn run(mut self, infra: Infra) -> Result<CascadeOutcome> {
        if self.depth >= hkask_capability::SYSTEM_MAX_RECURSION {
            return Err(TemplateError::Manifest(format!(
                "Matryoshka depth limit ({}) exceeded",
                hkask_capability::SYSTEM_MAX_RECURSION
            )));
        }

        let exit_kind = loop {
            self.iteration += 1;

            let node = self.graph.step(self.pc).clone();

            // Title callback.
            if let Some(ref title) = infra.title {
                let desc = if node.description.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", node.description)
                };
                let total = self.graph.len();
                if self.iteration > 1 {
                    title(&format!(
                        "Iter {} — Step {}/{}{}",
                        self.iteration, node.ordinal, total, desc
                    ));
                } else {
                    title(&format!("Step {}/{}{}", node.ordinal, total, desc));
                }
            }

            // Dispatch.
            let effect = self.dispatch_with_retry(node.clone(), infra.clone()).await?;
            let cf = self.apply_effect(&node, effect);
            match cf {
                ControlFlow::Fallthrough => {
                    self.pc = self.pc + 1;
                    if self.pc == ENTRY {
                        // Wrapped around — shouldn't happen without a loop step.
                        break ExitKind::Converged;
                    }
                }
                ControlFlow::Jump(target) => {
                    self.pc = target;
                }
                ControlFlow::Reenter(target) => {
                    // Convergence check — the one place.
                    let status = self.convergence.check(&self.context.inputs.clone().into_iter().collect::<serde_json::Map<_,_>>());
                    match status {
                        ConvergenceStatus::Converged => break ExitKind::Converged,
                        ConvergenceStatus::MaxedOut => break ExitKind::MaxedOut,
                        ConvergenceStatus::Escalated => break ExitKind::Escalated,
                        ConvergenceStatus::Continue => {}
                    }
                    self.context.snapshot_prev();
                    self.pc = target;
                }
                ControlFlow::Exit(kind) => break kind,
            }

            // End of graph without re-enter.
            if self.pc == ENTRY && self.iteration > 1 {
                break ExitKind::Converged;
            }
        };

        Ok(CascadeOutcome {
            context: self.context,
            iterations: self.iteration,
            exit_kind,
            last_result_step: self.last_result_step,
            resume_text: self.resume_text.take(),
            tool_calls: std::mem::take(&mut self.tool_calls),
        })
    }

    async fn dispatch_with_retry(
        &mut self,
        node: crate::step_graph::StepNode,
        infra: Infra,
    ) -> Result<crate::step_actions::Effect> {
        let max_retries = if true {
            self.error_handling.max_retries
        } else {
            0
        };

        let mut attempt = 0;
        loop {
            match self.dispatch_action(node.clone(), infra.clone()).await {
                Ok(effect) => return Ok(effect),
                Err(TemplateError::Timeout {
                    step_ordinal,
                    elapsed_seconds,
                }) if attempt < max_retries && true => {
                    attempt += 1;
                    if self.error_handling.retry_backoff_seconds > 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(
                            self.error_handling.retry_backoff_seconds as u64,
                        ))
                        .await;
                    }
                    continue;
                }
                Err(e) => {
                    if let Some(effect) = self.handle_step_failure(&node, &infra, &e).await {
                        return Ok(effect);
                    }
                    return Err(e);
                }
            }
        }
    }

    async fn dispatch_action(
        &mut self,
        node: crate::step_graph::StepNode,
        infra: Infra,
    ) -> Result<crate::step_actions::Effect> {
        use crate::step_actions::Effect;
        let action: &str = &node.action;
        match action {
            "abort" => Ok(Effect::Exit(ExitKind::Converged)),
            "escalate" => Ok(Effect::Exit(ExitKind::Escalated)),
            "loop" => self.execute_loop(&node, &infra),
            "select" => self.execute_select(node, infra).await,
            "compute" => self.execute_compute(node, infra).await,
            "render" => self.execute_render(node, infra).await,
            "execute" | "feedback" | "validate" | "retrieve" => {
                self.execute_tool_invoke(node, infra).await
            }
            "flowdef" => self.execute_flowdef(node, infra).await,
            "parallel" => self.execute_parallel(node, infra).await,
            other => Err(TemplateError::Manifest(format!(
                "Unknown manifest step action: '{other}'"
            ))),
        }
    }

    async fn handle_step_failure(
        &mut self,
        node: &crate::step_graph::StepNode,
        _infra: &Infra,
        error: &TemplateError,
    ) -> Option<crate::step_actions::Effect> {
        let on_failure = node.on_failure.as_ref()?;
        tracing::warn!(
            target: "reg.skill.cascade.step_failed",
            step = node.ordinal,
            error = %error,
            resume = %on_failure.resume,
            "Step failed — escalating"
        );
        self.resume_text = Some(on_failure.resume.clone());
        Some(crate::step_actions::Effect::Exit(ExitKind::Escalated))
    }

    fn apply_effect(
        &mut self,
        node: &crate::step_graph::StepNode,
        effect: crate::step_actions::Effect,
    ) -> ControlFlow {
        use crate::step_actions::Effect;
        match effect {
            Effect::Stored { step_id, value } => {
                self.context.store_result(step_id, node.ordinal, value);
                self.last_result_step = Some(step_id);
                ControlFlow::Fallthrough
            }
            Effect::StoredNamed {
                step_id,
                suffix,
                value,
            } => {
                self.context
                    .store_named(step_id, node.ordinal, &suffix, value);
                ControlFlow::Fallthrough
            }
            Effect::NoOp => ControlFlow::Fallthrough,
            Effect::Jump(target) => ControlFlow::Jump(target),
            Effect::Reenter(target) => ControlFlow::Reenter(target),
            Effect::Exit(kind) => ControlFlow::Exit(kind),
        }
    }
}

enum PassResult {
    Reenter(StepId),
    Exit(ExitKind),
}
