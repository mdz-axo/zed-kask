//! Step action implementations — one function per action type.
//!
//! Each function is a method on `StepMachine` that takes a `&StepNode` and
//! `&Infra` and returns `Result<Effect>`. They are small (40-80 lines each)
//! and independently testable with mock infrastructure.
//!
//! The only probabilistic action is `execute_select` (it calls
//! `InferencePort`). Everything else is deterministic.

use crate::ports::{Result, TemplateError};
use crate::step_context::ContextLookup;
use crate::step_graph::{ExitKind, StepId};
use crate::step_machine::{Infra, StepMachine};
use hkask_capability::ToolPort;
use hkask_capability::tool_taint::ToolTaint;
use hkask_types::ChatToolDefinition;
use hkask_types::ports::inference_port::InferencePort;
use hkask_types::template::LLMParameters;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// What a step action produced. The machine merges this with the node's
/// static `ControlFlow` to decide what happens next.
#[derive(Debug, Clone)]
pub enum Effect {
    Stored {
        step_id: StepId,
        value: Value,
        taint: ToolTaint,
    },
    StoredNamed {
        step_id: StepId,
        suffix: String,
        value: Value,
        taint: ToolTaint,
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
            let bound =
                crate::input_mapping::resolve_mapping_value(value, ctx.legacy_map(), renderer);
            ctx.insert_legacy(key.clone(), bound);
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
                                .legacy(field)
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
                    .render(s, self.context.legacy_map())
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
                    self.context.legacy_map(),
                    &infra.template_renderer,
                );
                self.context.insert_legacy(key.clone(), bound);
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
        let (prompt, raw_template_content) =
            render_step_template_with_raw(node, self.context.legacy_map(), infra)?;

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

        // Call inference with streaming + timeout.
        let params = infra.default_params.clone();
        let timeout_dur = std::time::Duration::from_secs(node.timeout_seconds as u64);

        let (result_text, tool_calls, cost_usd) = call_inference_stream(
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
                tracing::warn!(
                    target: "reg.skill.cascade.step_executed",
                    step = node.ordinal,
                    "Model did not call structured-output tool — falling back to text parsing"
                );
            }
            crate::executor::parse_json_response(&result_text, node.ordinal)?
        };

        // Inject budget context for template awareness.
        self.budget
            .inject_into_context(self.context.legacy_map_mut());

        Ok(Effect::Stored {
            step_id: node.id,
            value: parsed,
            taint: ToolTaint::Pure, // LLM output is not tainted (it's generated, not external)
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

        let populated = render_step_template(node, self.context.legacy_map(), infra)?;

        Ok(Effect::StoredNamed {
            step_id: node.id,
            suffix: "populated".to_string(),
            value: Value::String(populated),
            taint: ToolTaint::Pure,
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
                            self.context.legacy_map(),
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
            taint: ToolTaint::Pure,
        })
    }

    /// **Render** — render a template without inference.
    pub(crate) async fn execute_render(
        &mut self,
        node: &crate::step_graph::StepNode,
        infra: &Infra,
    ) -> Result<Effect> {
        let rendered = render_step_template(node, self.context.legacy_map(), infra)?;
        Ok(Effect::Stored {
            step_id: node.id,
            value: Value::String(rendered),
            taint: ToolTaint::Pure,
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
        let mcp_ref = crate::template_renderer::TemplateRenderer::render_inline(
            mcp_ref_raw,
            self.context.legacy_map(),
        );

        // Check for untrusted input (FIDES taint check).
        let has_untrusted_input = node
            .input_mapping
            .as_ref()
            .is_some_and(|mapping| check_untrusted_input(mapping, self.context.legacy_map()));

        // Resolve the tool input.
        let input: Value = node
            .input_mapping
            .as_ref()
            .map(|mapping| {
                crate::input_mapping::resolve_mapping_value(
                    mapping,
                    self.context.legacy_map(),
                    &infra.template_renderer,
                )
            })
            .unwrap_or_else(|| {
                Value::Object(
                    self.context
                        .legacy_map()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                )
            });

        // Invoke the tool.
        let (result, tool_taint) = invoke_tool(
            &infra.tools,
            &infra.runtime_policy,
            &mcp_ref,
            input,
            self.context.legacy_map().len() as u64,
            has_untrusted_input,
        )
        .await?;

        Ok(Effect::Stored {
            step_id: node.id,
            value: result,
            taint: tool_taint,
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
        let template_ref = crate::template_renderer::TemplateRenderer::render_inline(
            template_ref,
            self.context.legacy_map(),
        );

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

        // Snapshot the parent's context keys.
        let parent_keys: std::collections::HashSet<String> =
            self.context.legacy_map().keys().cloned().collect();

        // Run the sub-cascade.
        let mut sub_machine =
            StepMachine::new(sub_graph, self.context.clone(), sub_budget, sub_convergence);
        sub_machine.depth = self.depth + 1;

        let sub_outcome = Box::pin(sub_machine.run(infra)).await?;

        // Extract the sub-cascade's final result.
        let result_value = sub_outcome
            .last_result_step
            .and_then(|step_id| sub_outcome.context.result(step_id))
            .map(|r| crate::executor::normalize_model_output(&r.value).into_owned())
            .unwrap_or(Value::Null);

        // Reconstruct the parent context — keep only the parent's original keys.
        let mut parent_context = crate::step_context::StepContext::new(self.context.inputs.clone());
        for (key, value) in sub_outcome.context.legacy_map() {
            if parent_keys.contains(key) {
                parent_context.insert_legacy(key.clone(), value.clone());
            }
        }
        // Copy back the typed results for parent keys.
        for (step_id, result) in sub_outcome.context.results_iter() {
            if parent_keys.contains(&format!("step_{}_result", result.ordinal)) {
                parent_context.store_result(
                    *step_id,
                    result.ordinal,
                    result.value.as_ref().clone(),
                    result.taint,
                );
            }
        }

        // Replace our context with the reconstructed parent context.
        self.context = parent_context;

        // Deduct the sub-cascade's actual gas/rJoule consumption.
        self.budget.consume_child(
            sub_outcome.budget_snapshot.gas_used,
            sub_outcome.budget_snapshot.rjoule_used,
        );

        Ok(Effect::Stored {
            step_id: node.id,
            value: result_value,
            taint: ToolTaint::Pure,
        })
    }
}

// ── Helper functions ──────────────────────────────────────────────────────

/// Render a step's template and return the rendered string.
fn render_step_template(
    node: &crate::step_graph::StepNode,
    context: &HashMap<String, Value>,
    infra: &Infra,
) -> Result<String> {
    let (rendered, _) = render_step_template_with_raw(node, context, infra)?;
    Ok(rendered)
}

/// Render a step's template and return both the rendered prompt and the raw
/// template content (for output-schema extraction).
fn render_step_template_with_raw(
    node: &crate::step_graph::StepNode,
    context: &HashMap<String, Value>,
    infra: &Infra,
) -> Result<(String, String)> {
    let renderer = node.renderer.as_deref().unwrap_or("");

    match renderer {
        "minijinja" => {
            let template_ref_raw = node.template_ref.as_deref().ok_or_else(|| {
                TemplateError::Manifest(format!(
                    "Step {} has renderer='minijinja' but no template_ref",
                    node.ordinal
                ))
            })?;
            let template_ref = crate::template_renderer::TemplateRenderer::render_inline(
                template_ref_raw,
                context,
            );

            let template_content = infra.template_renderer.load(&template_ref, node.ordinal)?;

            tracing::info!(
                target: "reg.spec.executor",
                step = node.ordinal,
                template = %template_ref,
                "Rendering minijinja template"
            );

            let prompt = infra.template_renderer.render(&template_content, context)?;
            Ok((prompt, template_content))
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

            let rendered = crate::template_renderer::TemplateRenderer::render_inline(
                template_content,
                context,
            );
            Ok((rendered, template_content.to_string()))
        }
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
) -> Result<(String, Vec<hkask_types::StructuredToolCall>, Option<f64>)> {
    use futures_util::StreamExt;

    let stream = inference.generate_stream(prompt, params, tools);

    let (full_text, tool_calls) = match tokio::time::timeout(timeout, async {
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
        Ok::<_, TemplateError>((full_text, chunk.tool_calls))
    })
    .await
    {
        Ok(Ok((text, tool_calls))) => (text, tool_calls),
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
    Ok((full_text, tool_calls, None))
}

/// Check whether a JSON value references any tainted (Source) context entries.
/// Replaces the old `check_untrusted_input` — but reads from the legacy map
/// since taint markers are stored there.
fn check_untrusted_input<C: ContextLookup>(value: &Value, context: &C) -> bool {
    // Walk the value for $ref and {{ }} references, check if any referenced
    // key has a taint marker in the legacy map.
    let mut keys = Vec::new();
    collect_referenced_keys(value, &mut keys);
    keys.iter().any(|key| {
        let marker = format!("__taint__{key}");
        context
            .get(&marker)
            .and_then(|v| v.as_u64())
            .is_some_and(|t| t == 1) // 1 = Source
    })
}

/// Collect all context keys referenced in a mapping value.
fn collect_referenced_keys(value: &Value, keys: &mut Vec<String>) {
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
            for value in map.values() {
                collect_referenced_keys(value, keys);
            }
        }
        Value::Array(arr) => {
            for value in arr {
                collect_referenced_keys(value, keys);
            }
        }
        Value::String(s) => {
            let mut remaining = s.as_str();
            while let Some(open) = remaining.find("{{") {
                let after_open = &remaining[open + 2..];
                let Some(close) = after_open.find("}}") else {
                    break;
                };
                let expr = after_open[..close].trim();
                let token = expr
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .find(|t| {
                        !t.is_empty()
                            && (t.starts_with(|c: char| c.is_alphabetic()) || t.starts_with('_'))
                            && !matches!(*t, "if" | "for" | "endif" | "endfor" | "else" | "elif")
                    });
                if let Some(tok) = token
                    && (tok.starts_with("step_") || tok == "task" || tok == "prev_step")
                {
                    keys.push(tok.to_string());
                }
                remaining = &after_open[close + 2..];
            }
        }
        _ => {}
    }
}

/// Invoke a tool with runtime policy checks. Replaces the old `invoke_tool`.
async fn invoke_tool(
    tools: &Arc<dyn ToolPort>,
    runtime_policy: &Option<Arc<hkask_regulation::DefaultPolicy>>,
    tool_name: &str,
    input: Value,
    action_number: u64,
    has_untrusted_input: bool,
) -> Result<(Value, ToolTaint)> {
    let tool_info = tools.get_tool_info(tool_name).await.ok_or_else(|| {
        TemplateError::NotFound(hkask_types::NotFound {
            entity_type: "tool".to_string(),
            id: tool_name.to_string(),
        })
    })?;

    if let Some(policy) = runtime_policy {
        use hkask_regulation::PolicyVerdict;

        match policy.check(
            tool_name,
            tool_info.taint,
            has_untrusted_input,
            action_number,
        ) {
            PolicyVerdict::Block(reason) => {
                tracing::warn!(
                    target: "reg.runtime.policy",
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
                    target: "reg.runtime.policy",
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
                tracing::info!(
                    target: "reg.runtime.policy",
                    tool = tool_name,
                    verdict = "log",
                    %message,
                    "REG"
                );
            }
            PolicyVerdict::Allow => {}
        }
    }

    let executor_webid = hkask_types::WebID::from_persona(b"manifest-executor");
    let token = hkask_capability::DelegationToken::new(
        hkask_capability::DelegationResource::Tool,
        tool_name.to_string(),
        hkask_capability::DelegationAction::Execute,
        executor_webid,
        executor_webid,
    );

    let result = tools
        .invoke(&tool_info.server_id, tool_name, input, &token)
        .await
        .map_err(|error| match error {
            hkask_capability::ToolPortError::CapabilityDenied(message) => {
                TemplateError::CapabilityDenied(message)
            }
            other => TemplateError::Mcp(Box::new(other)),
        })?;

    Ok((result, tool_info.taint))
}
