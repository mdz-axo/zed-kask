//! Step action implementations — one function per action type.
//!
//! Each function is a method on `StepMachine` that takes a `&StepNode` and
//! `&Infra` and returns `Result<Effect>`. They are small (40-80 lines each)
//! and independently testable with mock infrastructure.
//!
//! The only probabilistic action is `execute_select` (it calls
//! `InferencePort`). Everything else is deterministic.

use crate::ports::{Result, TemplateError};
use crate::step_context::StepContext;
use crate::step_graph::{ExitKind, StepId};
use crate::step_machine::{CascadeOutcome, Infra, StepMachine};
use crate::template_renderer::InferenceBlock;
use futures_util::StreamExt;
use futures_util::stream;
use hkask_capability::ToolPort;
use hkask_types::ChatToolDefinition;
use hkask_types::ports::inference_port::InferencePort;
use hkask_types::template::LLMParameters;
use serde_json::Value;
use std::sync::Arc;

/// What a step action produced. The machine merges this with the node's
/// static `ControlFlow` to decide what happens next.
#[derive(Debug, Clone)]
pub enum Effect {
    Stored {
        step_id: StepId,
        value: Value,
    },
    StoredNamed {
        step_id: StepId,
        suffix: String,
        value: Value,
    },
    Jump(StepId),
    Reenter(StepId),
    Exit(crate::step_graph::ExitKind),
    NoOp,
    ConsumedGas(u32),
    ConsumedRJoule(f64),
}

/// Helper: resolve a step's `input_mapping` into the context.
pub(crate) fn apply_input_mapping(
    ctx: &mut crate::step_context::StepContext,
    mapping: &Value,
    renderer: &crate::template_renderer::TemplateRenderer,
) {
    if let Value::Object(map) = mapping {
        for (key, value) in map {
            let bound = crate::input_mapping::resolve_mapping_value(value, ctx, renderer);
            ctx.insert_protocol(key.clone(), bound);
        }
    }
}

impl StepMachine {
    /// **Choice** — evaluate a condition and jump to a target step.
    /// Pure: no inference, no tools, no side effects.
    pub(crate) fn execute_choice(&self, node: &crate::step_graph::StepNode) -> Result<Effect> {
        let mapping = match &node.input_mapping {
            Some(m) => m,
            None => {
                tracing::warn!(
                    target: "reg.skill.cascade.choice_misconfigured",
                    step = node.ordinal,
                    "Step {} (action 'choice') has no `input_mapping` — the `branches` array lives under \
                     `input_mapping.branches`. The choice will never branch.",
                    node.ordinal
                );
                return Ok(Effect::NoOp);
            }
        };

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
                        if let Some((field, op, val_str)) =
                            crate::condition::parse_choice_condition(condition)
                        {
                            let current = self
                                .context
                                .lookup(field)
                                .and_then(|v| v.as_f64())
                                .unwrap_or(1.0);
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
                        "continue" => Ok(Effect::NoOp),
                        "abort" => Ok(Effect::Exit(ExitKind::Converged)),
                        "escalate" => Ok(Effect::Exit(ExitKind::Escalated)),
                        _ => {
                            if let Some(ordinal) = action.parse::<u32>().ok() {
                                if let Some(step_id) = self.graph.find(ordinal) {
                                    Ok(Effect::Jump(step_id))
                                } else {
                                    Err(TemplateError::Manifest(format!(
                                        "Choice action '{action}' — ordinal {ordinal} not found in graph"
                                    )))
                                }
                            } else {
                                Err(TemplateError::Manifest(format!(
                                    "Choice action '{action}' is not a valid ordinal"
                                )))
                            }
                        }
                    };
                }
            }
        } else {
            tracing::warn!(
                target: "reg.skill.cascade.choice_misconfigured",
                step = node.ordinal,
                "Step {} (action 'choice') has `input_mapping` but no `branches` array.",
                node.ordinal
            );
        }

        Ok(Effect::NoOp)
    }

    /// **Loop** — re-enter the cascade from a target step. Bounded by
    /// `max_iterations` (checked by the machine in the `Reenter` arm).
    pub(crate) fn execute_loop(
        &mut self,
        node: &crate::step_graph::StepNode,
        infra: &Infra,
    ) -> Result<Effect> {
        let loop_target = node
            .input_mapping
            .as_ref()
            .and_then(|m| m.get("loop_target"))
            .and_then(|v| v.as_str())
            .and_then(|s| {
                infra
                    .template_renderer
                    .render(s, &self.context)
                    .ok()
                    .and_then(|rendered| rendered.trim().parse::<u32>().ok())
            })
            .unwrap_or(0);

        // Bind loop input_mapping (except loop_target) into context.
        if let Some(mapping) = node.input_mapping.as_deref()
            && let Value::Object(map) = mapping
        {
            for (key, value) in map {
                if key == "loop_target" {
                    continue;
                }
                let bound = crate::input_mapping::resolve_mapping_value(
                    value,
                    &self.context,
                    &infra.template_renderer,
                );
                self.context.insert_protocol(key.clone(), bound);
            }
        }

        // Read convergence signal BEFORE re-entering (the loop step's
        // convergence_signal binding is now in the legacy map).
        self.context.read_convergence_signal();

        let step_id = self
            .graph
            .find(loop_target)
            .unwrap_or(crate::step_graph::ENTRY);
        Ok(Effect::Reenter(step_id))
    }

    /// **Select** — render a template, call inference, parse JSON result.
    /// The only probabilistic action.
    pub(crate) async fn execute_select(
        &mut self,
        node: &crate::step_graph::StepNode,
        infra: &Infra,
    ) -> Result<Effect> {
        // Apply input_mapping.
        if let Some(ref mapping) = node.input_mapping {
            crate::step_actions::apply_input_mapping(
                &mut self.context,
                mapping,
                &infra.template_renderer,
            );
        }

        // Render the template.
        let (prompt, raw_template_content, inference_block) =
            render_step_template_with_raw(node, &self.context, infra)?;

        // Resolve output schema for structured tool calling.
        let output_schema = crate::output_schema::resolve_output_schema(
            node.output_schema.as_deref(),
            &raw_template_content,
        );
        let structured_tool = output_schema
            .as_ref()
            .map(|schema| crate::output_schema::build_structured_output_tool(schema.clone()));
        let tools: Option<&[ChatToolDefinition]> =
            structured_tool.as_ref().map(std::slice::from_ref);

        // Merge per-step inference parameters from the template's `[inference]`
        // block over the default params. Templates declare temperature,
        // max_tokens, and thinking_budget per step — without this, every call
        // uses the default (temperature 0.6, max_tokens 2048), which is too
        // low for complex templates that need thinking + a full JSON response.
        let mut params = infra.default_params.clone();
        if let Some(temp) = inference_block.temperature {
            params.temperature = temp;
        }
        if let Some(max_tok) = inference_block.max_tokens {
            params.max_tokens = max_tok;
        }
        match inference_block.thinking_budget.as_deref() {
            Some("full") | Some("on") => {
                params.disable_thinking = false;
            }
            Some("off") | Some("none") => {
                params.disable_thinking = true;
            }
            Some(other) => {
                // LLMParameters only has a boolean disable_thinking —
                // intermediate values ("minimal", "low", "medium", etc.)
                // can't be represented. Warn so operators can distinguish
                // "not configured" from "configured but unrecognized".
                tracing::warn!(
                    target: "hkask.templates.inference_block",
                    thinking_budget = %other,
                    "Unrecognized thinking_budget value — falling back to default disable_thinking"
                );
            }
            None => {}
        }

        let timeout_dur = effective_timeout(node.timeout_seconds);

        let (result_text, tool_calls, cost_usd, finish_reason) = call_inference_stream(
            &infra.inference,
            &prompt,
            &params,
            tools,
            timeout_dur,
            infra.progress.as_deref(),
        )
        .await?;

        // Charge rJoule (USD cost).
        if let Some(cost) = cost_usd {
            self.budget.charge_rjoule(cost);
        }

        // Charge gas (one iteration of compute).
        self.budget.charge_iteration();

        // Extract the parsed result.
        let parsed: Value = if let Some(tool_call) = tool_calls.first() {
            tracing::info!(
                target: "reg.skill.cascade.step_executed",
                step = node.ordinal,
                structured_output = true,
                "Model emitted structured tool call — extracting args"
            );
            tool_call.args.clone()
        } else {
            if output_schema.is_some() {
                // zed-kask: D25 — a truncated generation (finish_reason "length")
                // never emits the structured-output tool call. Refuse to parse the
                // partial text as JSON — surface a loud error so the regulation loop
                // / UI can act (raise max_tokens, shrink prompt, or retry) instead of
                // silently feeding truncated output to parse_json_response.
                if finish_reason.as_deref() == Some("length") {
                    tracing::warn!(
                        target: "reg.skill.cascade.step_executed",
                        step = node.ordinal,
                        "Step truncated at max_tokens before emitting structured-output tool call"
                    );
                    return Err(TemplateError::Manifest(format!(
                        "Step {} truncated at max_tokens before emitting the structured-output \\n                         tool call — increase max_tokens or reduce the prompt; refusing to \
                         parse partial output",
                        node.ordinal
                    )));
                }
                tracing::warn!(
                    target: "reg.skill.cascade.step_executed",
                    step = node.ordinal,
                    "Model did not call structured-output tool — falling back to text parsing"
                );
            }
            crate::executor::parse_json_response(&result_text, node.ordinal)?
        };

        // Inject budget context for template awareness.
        self.budget.inject_into_context(&mut self.context);

        Ok(Effect::Stored {
            step_id: node.id,
            value: parsed,
        })
    }

    /// **Populate** — render a template with the accumulated context.
    pub(crate) async fn execute_populate(
        &mut self,
        node: &crate::step_graph::StepNode,
        infra: &Infra,
    ) -> Result<Effect> {
        // Apply input_mapping.
        if let Some(ref mapping) = node.input_mapping {
            crate::step_actions::apply_input_mapping(
                &mut self.context,
                mapping,
                &infra.template_renderer,
            );
        }

        let populated = render_step_template(node, &self.context, infra)?;

        Ok(Effect::StoredNamed {
            step_id: node.id,
            suffix: "populated".to_string(),
            value: Value::String(populated),
        })
    }

    /// **Compute** — invoke a deterministic compute primitive.
    pub(crate) async fn execute_compute(
        &mut self,
        node: &crate::step_graph::StepNode,
        infra: &Infra,
    ) -> Result<Effect> {
        let compute_ref = node.compute_ref.as_deref().ok_or_else(|| {
            TemplateError::Manifest(format!("Compute step {} has no compute_ref", node.ordinal))
        })?;

        let input: Value = node
            .input_mapping
            .as_deref()
            .map(|mapping| {
                if let Value::Object(map) = mapping {
                    let mut out = serde_json::Map::new();
                    for (key, value) in map {
                        let bound = crate::input_mapping::resolve_mapping_value(
                            value,
                            &self.context,
                            &infra.template_renderer,
                        );
                        out.insert(key.clone(), bound);
                    }
                    Value::Object(out)
                } else {
                    mapping.clone()
                }
            })
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

        let result = crate::compute::dispatch_compute(compute_ref, &input)?;

        tracing::info!(
            target: "reg.skill.cascade.compute",
            ordinal = node.ordinal,
            compute_ref = compute_ref,
            "REG"
        );

        Ok(Effect::Stored {
            step_id: node.id,
            value: result,
        })
    }

    /// **Render** — render a template without inference.
    pub(crate) async fn execute_render(
        &mut self,
        node: &crate::step_graph::StepNode,
        infra: &Infra,
    ) -> Result<Effect> {
        let rendered = render_step_template(node, &self.context, infra)?;
        Ok(Effect::Stored {
            step_id: node.id,
            value: Value::String(rendered),
        })
    }

    /// **Execute** — invoke an MCP tool with parameters bound from context.
    pub(crate) async fn execute_tool_invoke(
        &mut self,
        node: &crate::step_graph::StepNode,
        infra: &Infra,
    ) -> Result<Effect> {
        let mcp_ref_raw = node.mcp.as_deref().ok_or_else(|| {
            TemplateError::Manifest(format!(
                "Execute step {} has no mcp reference",
                node.ordinal
            ))
        })?;

        // Resolve ${variable} references in the MCP reference.
        let mcp_ref =
            crate::template_renderer::TemplateRenderer::render_inline(mcp_ref_raw, &self.context);

        // Resolve the tool input.
        let input: Value = node
            .input_mapping
            .as_ref()
            .map(|mapping| {
                crate::input_mapping::resolve_mapping_value(
                    mapping,
                    &self.context,
                    &infra.template_renderer,
                )
            })
            .unwrap_or_else(|| {
                Value::Object(
                    self.context
                        .entries()
                        .map(|(k, v)| (k, v.clone()))
                        .collect(),
                )
            });

        // Invoke the tool with a timeout. Without this, a hung MCP tool call
        // blocks the cascade forever — the tokio task has no external watchdog.
        let timeout_dur = effective_timeout(node.timeout_seconds);
        let result =
            match tokio::time::timeout(timeout_dur, invoke_tool(&infra.tools, &mcp_ref, input))
                .await
            {
                Ok(inner) => inner?,
                Err(_elapsed) => {
                    return Err(TemplateError::Manifest(format!(
                        "Tool step {} timed out after {}s",
                        node.ordinal,
                        timeout_dur.as_secs()
                    )));
                }
            };

        Ok(Effect::Stored {
            step_id: node.id,
            value: result,
        })
    }

    /// **FlowDef** — recursively execute a sub-manifest as a nested cascade.
    pub(crate) async fn execute_flowdef(
        &mut self,
        node: &crate::step_graph::StepNode,
        infra: &Infra,
    ) -> Result<Effect> {
        let template_ref = node.template_ref.as_deref().ok_or_else(|| {
            TemplateError::Manifest(format!(
                "Step {} has action='flowdef' but no template_ref",
                node.ordinal
            ))
        })?;

        // Resolve {{key}} references from context.
        let template_ref =
            crate::template_renderer::TemplateRenderer::render_inline(template_ref, &self.context);

        // Load the sub-manifest YAML.
        let manifest_yaml = if let Ok(content) = infra
            .template_renderer
            .load_from_disk(&template_ref, node.ordinal)
        {
            content
        } else if let Some(content) = crate::template_yaml_file(&template_ref) {
            content.to_string()
        } else if let Some(content) = crate::template_file(&template_ref) {
            content.to_string()
        } else {
            return Err(TemplateError::NotFound(hkask_types::NotFound {
                entity_type: "flowdef sub-manifest".to_string(),
                id: format!(
                    "Step {}: sub-manifest '{}' not found",
                    node.ordinal, template_ref
                ),
            }));
        };

        // Parse the sub-manifest.
        let mut sub_manifest = crate::manifest_loader::load_manifest_from_yaml(&manifest_yaml)
            .map_err(|e| {
                TemplateError::Manifest(format!(
                    "Step {}: failed to parse sub-manifest '{}': {}",
                    node.ordinal, template_ref, e
                ))
            })?;

        // Cap the sub-cascade's budget to the parent's remaining budget.
        let sub_gas_cap = (sub_manifest.gas.cap as u64).min(self.budget.remaining_gas());
        let sub_rjoule_cap_f64 =
            (sub_manifest.rjoule.cap as f64).min(self.budget.remaining_rjoule().max(0.0));
        let sub_rjoule_cap = if sub_rjoule_cap_f64.is_finite() {
            sub_rjoule_cap_f64
        } else {
            tracing::warn!(
                target: "hkask.templates",
                "sub_rjoule_cap is not finite — clamping to 0."
            );
            0.0
        };
        sub_manifest.gas.cap = sub_gas_cap as u32;
        sub_manifest.rjoule.cap = sub_rjoule_cap as u32;

        // Apply input_mapping.
        if let Some(ref mapping) = node.input_mapping {
            crate::step_actions::apply_input_mapping(
                &mut self.context,
                mapping,
                &infra.template_renderer,
            );
        }

        // Build the sub-graph and sub-machine.
        let sub_graph = crate::step_graph::StepGraph::new(
            &sub_manifest.steps,
            sub_manifest.convergence.max_iterations,
        );
        let sub_budget = crate::budget::BudgetTracker::new(&sub_manifest.gas, &sub_manifest.rjoule);
        let sub_convergence =
            crate::convergence::ConvergenceTracker::new(&sub_manifest.convergence);

        // Snapshot the parent's keys (so we merge back only parent-key updates
        // from the sub-cascade, dropping sub-only keys). Computed before the
        // sub-cascade; `merge_back_sub_cascade` keeps the parent intact (the
        // sub-cascade ran on a clone).
        let parent_step_ids: std::collections::HashSet<StepId> =
            self.context.results_iter().map(|(id, _)| *id).collect();
        let parent_protocol_keys: Vec<String> =
            self.context.protocol_map().keys().cloned().collect();
        let parent_named_keys: Vec<String> = self.context.named_map().keys().cloned().collect();

        // Run the sub-cascade.
        let mut sub_machine =
            StepMachine::new(sub_graph, self.context.clone(), sub_budget, sub_convergence);
        sub_machine.depth = self.depth + 1;

        let sub_outcome = Box::pin(sub_machine.run(infra.clone())).await?;

        // Extract the sub-cascade's final result.
        let result_value = sub_outcome
            .last_result_step
            .and_then(|step_id| sub_outcome.context.result(step_id))
            .map(|r| crate::executor::normalize_model_output(&r.value).into_owned())
            .unwrap_or(Value::Null);

        // Merge the sub-cascade's updates back into the parent — keep only the
        // parent's original keys (step ids, protocol keys, named keys); drop
        // sub-only keys.
        self.context.merge_back_sub_cascade(
            &sub_outcome.context,
            &parent_step_ids,
            &parent_protocol_keys,
            &parent_named_keys,
        );

        // Deduct the sub-cascade's actual gas/rJoule consumption.
        self.budget.consume_child(
            sub_outcome.budget_snapshot.gas_used,
            sub_outcome.budget_snapshot.rjoule_used,
        );

        Ok(Effect::Stored {
            step_id: node.id,
            value: result_value,
        })
    }

    /// **Parallel** — run a list of sub-cascades concurrently (K2). Branches
    /// live under `input_mapping.branches`; `concurrency_cap` bounds in-flight
    /// branches; `join` is `"list"` (first cut). Each branch: its own
    /// `ConvergenceTracker` + a `BudgetTracker` that shares the parent's gas
    /// `Arc<AtomicU64>` (enforced during the wave) + owns its rJoule (joined
    /// after via `charge_rjoule`). Results join by `branch_id` — deterministic,
    /// not completion order.
    pub(crate) async fn execute_parallel(
        &mut self,
        node: &crate::step_graph::StepNode,
        infra: &Infra,
    ) -> Result<Effect> {
        let step_ordinal = node.ordinal;
        let mapping = node.input_mapping.as_deref().cloned().ok_or_else(|| {
            TemplateError::Manifest(format!(
                "Step {step_ordinal} (action 'parallel') has no input_mapping — the branch \
                 list lives under input_mapping.branches.",
            ))
        })?;
        // Clone the branches array into an owned vec so the branch futures
        // don't borrow from the local `mapping`. Without this, the
        // `buffer_unordered` stream holds `&mapping`, creating a
        // self-referential future that is not `'static` — `tokio::spawn`
        // (used by the bridge) rejects it with "Send is not general enough".
        let branches: Vec<Value> = mapping
            .get("branches")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                TemplateError::Manifest(format!(
                    "Step {step_ordinal} (action 'parallel') has no `branches` array in \
                     input_mapping.",
                ))
            })?
            .clone();
        let concurrency_cap = mapping
            .get("concurrency_cap")
            .and_then(|v| v.as_u64())
            .unwrap_or(branches.len() as u64)
            .max(1) as usize;
        let _join_mode = mapping
            .get("join")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        // Drop `mapping` — everything below is owned, no borrows from locals.
        drop(mapping);

        // Shared gas (enforced during the wave); per-branch rJoule (settled after).
        let shared_gas = self.budget.gas_atomic();
        let gas_cap = self.budget.gas_cap();
        let rjoule_remaining = self.budget.remaining_rjoule();
        let context_template = self.context.clone();

        let branch_futs = branches.into_iter().enumerate().map(|(branch_id, spec)| {
            let shared_gas = Arc::clone(&shared_gas);
            // `run` now owns the `Infra` (so its future is `Send + 'static` and
            // tokio-spawnable); clone `infra` + `context_template` per branch so
            // each `async move` owns its own.
            let infra = infra.clone();
            let context_template = context_template.clone();
            let template_ref = spec
                .get("template_ref")
                .and_then(|v| v.as_str())
                .map(String::from);
            async move {
                let template_ref = template_ref.ok_or_else(|| {
                    TemplateError::Manifest(format!(
                        "Step {} (action 'parallel') branch {} has no \
                         template_ref.",
                        step_ordinal, branch_id,
                    ))
                })?;
                let template_ref = crate::template_renderer::TemplateRenderer::render_inline(
                    &template_ref,
                    &context_template,
                );
                let manifest_yaml = if let Ok(content) = infra
                    .template_renderer
                    .load_from_disk(&template_ref, step_ordinal)
                {
                    content
                } else if let Some(content) = crate::template_yaml_file(&template_ref) {
                    content.to_string()
                } else if let Some(content) = crate::template_file(&template_ref) {
                    content.to_string()
                } else {
                    return Err(TemplateError::NotFound(hkask_types::NotFound {
                        entity_type: "parallel sub-manifest".to_string(),
                        id: format!(
                            "Step {} parallel branch {}: sub-manifest '{}' \
                             not found",
                            step_ordinal, branch_id, template_ref,
                        ),
                    }));
                };
                let sub_manifest = crate::manifest_loader::load_manifest_from_yaml(&manifest_yaml)
                    .map_err(|e| {
                        TemplateError::Manifest(format!(
                            "Step {} parallel branch {}: failed to parse \
                             sub-manifest '{}': {}",
                            step_ordinal, branch_id, template_ref, e,
                        ))
                    })?;
                let sub_budget = crate::budget::BudgetTracker::from_remaining_shared(
                    Arc::clone(&shared_gas),
                    gas_cap,
                    rjoule_remaining,
                );
                let sub_convergence =
                    crate::convergence::ConvergenceTracker::new(&sub_manifest.convergence);
                let sub_graph = crate::step_graph::StepGraph::new(
                    &sub_manifest.steps,
                    sub_manifest.convergence.max_iterations,
                );
                let sub_machine = StepMachine::new(
                    sub_graph,
                    context_template.clone(),
                    sub_budget,
                    sub_convergence,
                );
                let outcome = sub_machine.run(infra).await?;
                Ok::<(usize, CascadeOutcome), TemplateError>((branch_id, outcome))
            }
        });

        // Bounded concurrency: poll up to `concurrency_cap` branch futures at
        // once. `buffer_unordered` yields in completion order; we sort by
        // `branch_id` below for a deterministic join.
        let outcomes: Vec<(usize, CascadeOutcome)> = stream::iter(branch_futs)
            .buffer_unordered(concurrency_cap)
            .collect::<Vec<Result<(usize, CascadeOutcome)>>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        let mut ordered = outcomes;
        ordered.sort_by_key(|(id, _)| *id);

        // Deterministic join: results in branch_id order. `list` mode (first
        // cut) → Value::Array.
        let branch_results: Vec<Value> = ordered
            .iter()
            .map(|(_, o)| crate::executor::extract_final_step_result(o))
            .collect();
        let joined = Value::Array(branch_results);

        // After the wave: parent rJoule = sum of branch rJoule. Gas already in
        // the shared `Arc<AtomicU64>` (branches charged it during the wave).
        let sum_rjoule: f64 = ordered
            .iter()
            .map(|(_, o)| o.budget_snapshot.rjoule_used)
            .sum();
        self.budget.charge_rjoule(sum_rjoule);

        Ok(Effect::Stored {
            step_id: node.id,
            value: joined,
        })
    }
}

// ── Helper functions ──────────────────────────────────────────────────────

/// Render a step's template and return the rendered string.
fn render_step_template(
    node: &crate::step_graph::StepNode,
    ctx: &StepContext,
    infra: &Infra,
) -> Result<String> {
    let (rendered, _, _) = render_step_template_with_raw(node, ctx, infra)?;
    Ok(rendered)
}

/// Render a step's template and return the rendered prompt, the raw template
/// content (for output-schema extraction), and the parsed `[inference]` config
/// block (for LLM parameter overrides).
fn render_step_template_with_raw(
    node: &crate::step_graph::StepNode,
    ctx: &StepContext,
    infra: &Infra,
) -> Result<(String, String, crate::template_renderer::InferenceBlock)> {
    use crate::template_renderer::{parse_and_strip_inference_block, strip_front_matter};

    let renderer = node.renderer.as_deref().unwrap_or("");

    match renderer {
        "minijinja" => {
            let template_ref_raw = node.template_ref.as_deref().ok_or_else(|| {
                TemplateError::Manifest(format!(
                    "Step {} has renderer='minijinja' but no template_ref",
                    node.ordinal
                ))
            })?;
            let template_ref =
                crate::template_renderer::TemplateRenderer::render_inline(template_ref_raw, ctx);

            let template_content = infra.template_renderer.load(&template_ref, node.ordinal)?;

            tracing::info!(
                target: "reg.spec.executor",
                step = node.ordinal,
                template = %template_ref,
                "Rendering minijinja template"
            );

            // Parse the `[inference]` block from the template body (after front
            // matter stripping) to extract per-step LLM parameters.
            let after_front_matter = strip_front_matter(&template_content);
            let (_stripped_body, inference_block) =
                parse_and_strip_inference_block(after_front_matter);

            let prompt = infra.template_renderer.render(&template_content, ctx)?;
            Ok((prompt, template_content, inference_block))
        }
        _ => {
            let template_content = node
                .template_ref
                .as_deref()
                .or(node.renderer.as_deref())
                .ok_or_else(|| {
                    TemplateError::Manifest(format!(
                        "Step {} has no template_ref or renderer",
                        node.ordinal
                    ))
                })?;

            let rendered =
                crate::template_renderer::TemplateRenderer::render_inline(template_content, ctx);
            Ok((
                rendered,
                template_content.to_string(),
                InferenceBlock::default(),
            ))
        }
    }
}

/// Convert a step's `timeout_seconds` into a `Duration`, treating 0 as
/// "no explicit timeout" and substituting a 300s fallback. This is defense
/// in depth: the serde default on `BundleManifestStep::timeout_seconds` is
/// 120s, but manifests loaded through other paths (inline YAML, programmatic
/// construction) could still produce 0, which causes
/// `tokio::time::timeout(Duration::ZERO, ...)` to fire immediately without
/// polling the future.
fn effective_timeout(timeout_seconds: u32) -> std::time::Duration {
    if timeout_seconds == 0 {
        tracing::warn!(
            target: "hkask.templates.effective_timeout",
            "timeout_seconds is 0 — substituting 300s fallback to avoid zero-timeout"
        );
        std::time::Duration::from_secs(300)
    } else {
        std::time::Duration::from_secs(timeout_seconds as u64)
    }
}

/// Call inference with streaming, timeout, and reasoning-delta forwarding.
/// Returns (text, tool_calls, cost_usd).
///
/// The streaming path does not surface `cost_usd` (the stream chunks carry
/// token deltas, not the provider's observed cost). When cost tracking is
/// needed, the non-streaming `InferencePort::generate()` path populates it
/// from the full response. For now, the streaming path returns `None` for
/// cost — the budget tracker treats `None` as free (not charged).
async fn call_inference_stream(
    inference: &Arc<dyn InferencePort>,
    prompt: &str,
    params: &LLMParameters,
    tools: Option<&[ChatToolDefinition]>,
    timeout: std::time::Duration,
    progress: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> Result<(
    String,
    Vec<hkask_types::StructuredToolCall>,
    Option<f64>,
    Option<String>,
)> {
    use futures_util::StreamExt;

    // Defense in depth: if a caller passes Duration::ZERO (e.g. from a
    // manifest step with timeout_seconds: 0 loaded through a path that
    // bypasses the serde default), tokio::time::timeout fires immediately
    // without polling the inference future. Substitute a 300s fallback.
    let timeout = if timeout == std::time::Duration::ZERO {
        tracing::warn!(
            target: "hkask.templates.call_inference_stream",
            "timeout is Duration::ZERO — substituting 300s fallback"
        );
        std::time::Duration::from_secs(300)
    } else {
        timeout
    };

    let stream = inference.generate_stream(prompt, params, tools);

    let (full_text, tool_calls, finish_reason) = match tokio::time::timeout(timeout, async {
        let mut full_text = String::new();
        let mut final_chunk: Option<hkask_types::InferenceStreamChunk> = None;
        let mut stream = stream;
        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    if !chunk.reasoning_delta.is_empty() {
                        if let Some(progress) = progress {
                            progress(&chunk.reasoning_delta);
                        }
                    }
                    if !chunk.text_delta.is_empty() {
                        if let Some(progress) = progress {
                            progress(&chunk.text_delta);
                        }
                        full_text.push_str(&chunk.text_delta);
                    }
                    final_chunk = Some(chunk);
                }
                Err(e) => return Err(TemplateError::Inference(e)),
            }
        }
        let chunk = final_chunk.unwrap_or_else(|| hkask_types::InferenceStreamChunk {
            text_delta: String::new(),
            reasoning_delta: String::new(),
            model: String::new(),
            finish_reason: Some("stop".to_string()),
            usage: None,
            tool_calls: Vec::new(),
            cost_usd: None,
        });
        Ok::<_, TemplateError>((full_text, chunk.tool_calls, chunk.finish_reason))
    })
    .await
    {
        Ok(Ok((text, tool_calls, finish_reason))) => (text, tool_calls, finish_reason),
        Ok(Err(e)) => return Err(e),
        Err(_elapsed) => {
            return Err(TemplateError::Manifest(format!(
                "Step timed out after {}s",
                timeout.as_secs()
            )));
        }
    };

    // The streaming path does not surface cost_usd. Return None — the budget
    // tracker treats None as free (not charged). When cost tracking is needed,
    // use the non-streaming generate() path.
    Ok((full_text, tool_calls, None, finish_reason))
}

/// Resolve a tool's server and dispatch the call.
///
/// A FIDES taint gate (`DefaultPolicy::check` on a `Source`→`Sink` flow) used to
/// run here. It was removed rather than repaired: both of its inputs were
/// constants — every `ToolInfo` was labelled `Pure` at its only construction
/// site, and the untrusted-input flag read taint markers the context write side
/// had stopped emitting — so the block could never fire. Restoring the gate
/// means first giving tools real taint labels and propagating taint on write.
async fn invoke_tool(tools: &Arc<dyn ToolPort>, tool_name: &str, input: Value) -> Result<Value> {
    let tool_info = tools.get_tool_info(tool_name).await.ok_or_else(|| {
        TemplateError::NotFound(hkask_types::NotFound {
            entity_type: "tool".to_string(),
            id: tool_name.to_string(),
        })
    })?;

    // Accounting identity for the call meter — not a credential. The cascade's
    // authority comes from which tools the manifest may name, not from this.
    let executor_webid = hkask_types::WebID::from_persona(b"manifest-executor");

    tools
        .invoke(&tool_info.server_id, tool_name, input, executor_webid)
        .await
        .map_err(|error| TemplateError::Mcp(Box::new(error)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_timeout_substitutes_fallback_for_zero() {
        // A zero timeout_seconds must not produce Duration::ZERO — that
        // causes tokio::time::timeout to fire immediately without polling
        // the inference future, silently breaking every select/execute step.
        let result = effective_timeout(0);
        assert_eq!(result, std::time::Duration::from_secs(300));
    }

    #[test]
    fn effective_timeout_passes_through_nonzero() {
        let result = effective_timeout(120);
        assert_eq!(result, std::time::Duration::from_secs(120));

        let result = effective_timeout(1);
        assert_eq!(result, std::time::Duration::from_secs(1));
    }

    #[tokio::test]
    async fn call_inference_stream_threads_finish_reason_length() {
        // zed-kask: D25 — `call_inference_stream` must return the chunk's
        // finish_reason so `execute_select` can detect truncation
        // (finish_reason "length") and refuse to parse partial output as JSON.
        use hkask_types::{InferenceError, InferenceResult, InferenceUsage};
        use std::future::Future;
        use std::pin::Pin;

        struct TruncationStream {
            finish_reason: String,
            text: String,
        }
        impl InferencePort for TruncationStream {
            fn generate(
                &self,
                _prompt: &str,
                _parameters: &LLMParameters,
                _tools: Option<&[ChatToolDefinition]>,
            ) -> Pin<
                Box<
                    dyn Future<Output = std::result::Result<InferenceResult, InferenceError>>
                        + Send
                        + '_,
                >,
            > {
                // Return a truncated result: finish_reason "length", no tool
                // calls, partial text. The default `generate_stream` wraps this
                // into a single `InferenceStreamChunk` via `From<InferenceResult>`
                // which carries `finish_reason` through as `Some(...)`.
                let result = InferenceResult {
                    text: self.text.clone(),
                    model: "test".into(),
                    usage: InferenceUsage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                    finish_reason: self.finish_reason.clone(),
                    token_probabilities: None,
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                };
                Box::pin(async move { Ok(result) })
            }
        }

        let inference = Arc::new(TruncationStream {
            finish_reason: "length".into(),
            text: "{\"partial\":".into(),
        }) as Arc<dyn InferencePort>;
        let (text, tool_calls, _cost, finish_reason) = call_inference_stream(
            &inference,
            "prompt",
            &LLMParameters::default(),
            None,
            std::time::Duration::from_secs(30),
            None,
        )
        .await
        .expect("stream should complete");

        assert_eq!(text, "{\"partial\":");
        assert!(tool_calls.is_empty());
        assert_eq!(
            finish_reason.as_deref(),
            Some("length"),
            "finish_reason must be threaded out for execute_select truncation detection"
        );
    }
}
