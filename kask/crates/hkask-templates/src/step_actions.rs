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
use crate::step_graph::StepId;
use crate::step_machine::{CascadeOutcome, Infra, StepMachine, parse_json_response};
use crate::template_renderer::InferenceBlock;
use futures_util::StreamExt;
use futures_util::future::FutureExt;
use futures_util::stream;
use hkask_capability::ToolPort;
use hkask_types::ChatToolDefinition;
use hkask_types::ports::inference_port::InferencePort;
use hkask_types::ports::inference_types::ChatMessage;
use hkask_types::ports::memory_port::MemorySnippet;
use hkask_types::template::LLMParameters;
use serde_json::Value;
use std::sync::Arc;

/// Fallback timeout (in seconds) when a step's `timeout_seconds` is 0 or
/// `Duration::ZERO`. Prevents `tokio::time::timeout(Duration::ZERO, ...)` from
/// firing immediately without polling the inference future.
const INFERENCE_TIMEOUT_FALLBACK_SECS: u64 = 300;

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

fn cap_string_value(value: &Value) -> Value {
    match value {
        Value::String(s) if s.len() > 64 * 1024 => {
            Value::String(s.chars().take(64 * 1024).collect())
        }
        _ => value.clone(),
    }
}

fn tool_call_summary(mcp_ref: &str, result: std::result::Result<&Value, ()>) -> Value {
    match result {
        Ok(value) => {
            serde_json::json!({"tool": mcp_ref, "ok": true, "result": cap_string_value(value)})
        }
        Err(_) => serde_json::json!({"tool": mcp_ref, "ok": false}),
    }
}

fn load_sub_manifest_yaml(
    renderer: &crate::template_renderer::TemplateRenderer,
    template_ref: &str,
    step_ordinal: u32,
) -> Result<String> {
    if let Ok(content) = renderer.load_from_disk(template_ref, step_ordinal) {
        return Ok(content);
    }
    if let Some(content) = crate::template_yaml_file(template_ref) {
        return Ok(content.to_string());
    }
    if let Some(content) = crate::template_file(template_ref) {
        return Ok(content.to_string());
    }
    Err(TemplateError::NotFound(hkask_types::NotFound {
        entity_type: "sub-manifest".to_string(),
        id: format!("Step {step_ordinal}: sub-manifest '{template_ref}' not found"),
    }))
}

impl StepMachine {
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
        node: crate::step_graph::StepNode,
        infra: Infra,
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
            render_step_template_with_raw(&node, &self.context, &infra)?;

        // Resolve output schema for structured tool calling.
        let output_schema = crate::output_schema::resolve_output_schema(
            node.output_schema.as_deref(),
            &raw_template_content,
        );
        let structured_tool = output_schema
            .as_ref()
            .map(|schema| crate::output_schema::build_structured_output_tool(schema.clone()));
        let tools: Option<Vec<ChatToolDefinition>> = structured_tool.map(|tool| vec![tool]);

        // Merge per-step inference parameters from the template's `[inference]`
        // block over the default params. Templates declare temperature,
        // thinking_budget, work_effort, and verbosity per step — without this,
        // every call uses the default (temperature 0.6), which is too
        // low for complex templates that need thinking + a full JSON response.
        // work_effort is a fallback for thinking_budget (high/medium → ON,
        // low/minimal → OFF); thinking_budget takes precedence when both are set.
        // verbosity injects a system-prompt instruction controlling output length.
        let mut params = infra.default_params.clone();
        if let Some(temp) = inference_block.temperature {
            params.temperature = temp;
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
            None => {
                // thinking_budget not set — check work_effort as fallback.
                // work_effort maps: "high"/"medium" → thinking ON,
                // "low"/"minimal" → thinking OFF.
                match inference_block.work_effort.as_deref() {
                    Some("high") | Some("medium") => {
                        params.disable_thinking = false;
                    }
                    Some("low") | Some("minimal") => {
                        params.disable_thinking = true;
                    }
                    Some(other) => {
                        tracing::warn!(
                            target: "hkask.templates.inference_block",
                            work_effort = %other,
                            "Unrecognized work_effort value — falling back to default disable_thinking"
                        );
                    }
                    None => {}
                }
            }
        }

        let timeout_dur = effective_timeout(node.timeout_seconds);

        // Build the message array: [memory_system?, ...prior_messages, system=template, user=trigger]
        // This gives the provider the real conversation as proper role-tagged
        // messages — the same shape `agent_executor.rs` uses for swarm agents.
        // Without this, each template step is an isolated single-prompt call
        // with no conversational context, confusing the model (the original
        // bug this fixes).
        let messages = build_cascade_messages(
            &infra.prior_messages,
            &infra.memory_snippets,
            &prompt,
            inference_block.verbosity.as_deref(),
        );

        // Gate the inference call with the global concurrency limiter. The
        // permit is held across the cloud round-trip and released on drop
        // (including on timeout — `tokio::time::timeout` drops the inner
        // future, which drops the permit). `None` (tests, pre-startup) skips
        // gating. This is the primary inference path — without it, N concurrent
        // cascades each issuing `select` steps run unbounded against the
        // provider, defeating the process-wide ceiling.
        let _permit = if let Some(ref limiter) = infra.concurrency_limiter {
            Some(limiter.acquire().await)
        } else {
            None
        };

        // Clone the `Arc`-backed fields into standalone owned locals before the
        // await. rustc's higher-ranked `Send` check rejects futures that hold a
        // borrow of a struct field (`&infra.inference`, `&infra.progress`)
        // across an `.await` when the outer future is `tokio::spawn`ed and the
        // async fn has a `&mut self` lifetime parameter — even though the
        // pointees are `Send + Sync`. Borrowing a standalone local sidesteps it.
        let inference = infra.inference.clone();
        let progress = infra.progress.clone();
        // Read the thread's model from the cascade context. The caller
        // (SkillTool / send_skill_invocation) injects "thread_model" as a
        // provider-prefixed string. When present, the inference call routes
        // through this model instead of the InferencePort's startup-pinned
        // default.
        let model_override = self
            .context
            .inputs
            .get("thread_model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let inference_result = call_inference_stream_with_messages(
            inference,
            messages,
            params,
            model_override.as_deref(),
            tools,
            timeout_dur,
            node.ordinal,
            progress,
        )
        .await;

        // Ramp the limiter based on the call outcome before propagating the
        // error. `on_success` adds `step` permits (capped at `max`);
        // `on_throttle` backs off one `step` (floored at `step`). Only
        // 429/503-class errors back off — deterministic errors (parse, timeout)
        // don't shrink the pool for unrelated callers. A timeout is neither a
        // success (don't ramp up) nor a throttle (don't back off) — the
        // limiter stays at its current size.
        if let Some(ref limiter) = infra.concurrency_limiter {
            match &inference_result {
                Ok(_) => limiter.on_success(),
                Err(e) if e.is_throttle() => limiter.on_throttle(),
                _ => {}
            }
        }

        let (result_text, tool_calls, cost_usd, finish_reason) = inference_result?;

        // Charge rJoule (USD cost).
        if let Some(cost) = cost_usd {
            self.budget.charge_rjoule(cost);
        }

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
                // / UI can act (raise the token budget, shrink prompt, or retry) instead of
                // silently feeding truncated output to parse_json_response.
                if finish_reason.as_deref() == Some("length") {
                    tracing::warn!(
                        target: "reg.skill.cascade.step_executed",
                        step = node.ordinal,
                        failure_mode = "truncated",
                        "Step truncated at token limit before emitting structured-output tool call"
                    );
                    return Err(TemplateError::ParseFailure {
                        step_ordinal: node.ordinal,
                        detail: "truncated at token limit before emitting the structured-output \
                             tool call — increase the token budget or reduce the prompt; refusing to \
                             parse partial output"
                            .to_string(),
                    });
                }
                tracing::warn!(
                    target: "reg.skill.cascade.step_executed",
                    step = node.ordinal,
                    failure_mode = "no_structured_output",
                    "Model did not call structured-output tool — falling back to text parsing"
                );
            }
            // A2: empty-output guard. The model returned no text and no tool
            // call. Without this guard, `parse_json_response("")` produces a
            // cryptic "EOF while parsing a value at line 1 column 0" that gives
            // the operator no signal. Surface the finish_reason and likely
            // causes so the regulation loop / UI can act (raise the token budget,
            // enable thinking_budget, retry, or convert the step to a
            // deterministic `render` action).
            if result_text.trim().is_empty() {
                tracing::warn!(
                    target: "reg.skill.cascade.step_executed",
                    step = node.ordinal,
                    finish_reason = ?finish_reason,
                    failure_mode = "empty_output",
                    "Step returned empty output with no structured tool call."
                );
                return Err(TemplateError::ParseFailure {
                    step_ordinal: node.ordinal,
                    detail: format!(
                        "returned empty output (finish_reason: {:?}). Likely causes: token budget too low, model spent its budget on reasoning, or the provider returned no completion. Remediation: raise the token budget, enable thinking_budget, retry, or convert the manifest step from 'select' to 'render' action.",
                        finish_reason
                    ),
                });
            }
            parse_json_response(&result_text, node.ordinal)?
        };

        // Inject budget context for template awareness.
        self.budget.inject_into_context(&mut self.context);

        Ok(Effect::Stored {
            step_id: node.id,
            value: parsed,
        })
    }

    /// **Compute** — invoke a deterministic compute primitive.
    pub(crate) async fn execute_compute(
        &mut self,
        node: crate::step_graph::StepNode,
        infra: Infra,
    ) -> Result<Effect> {
        let compute_ref = node.compute_ref.as_deref().ok_or_else(|| {
            TemplateError::Manifest(format!("Compute step {} has no compute_ref", node.ordinal))
        })?;
        // Clone to an owned `String` so the `&str` borrow of `node.compute_ref`
        // doesn't live in the async fn's future type (rustc's HRTB `Send` check
        // rejects `&str` from a struct field under `tokio::spawn`).
        let compute_ref = compute_ref.to_string();

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

        let result = crate::compute::dispatch_compute(&compute_ref, &input)?;

        tracing::info!(
            target: "reg.skill.cascade.compute",
            ordinal = node.ordinal,
            compute_ref = %compute_ref,
            "REG"
        );

        // Stored as a NAMED result (suffix "compute"), not the primary
        // `Stored` effect: compute outputs are auxiliary values (convergence
        // signals feeding a loop step, pre-flight validation lists), not the
        // skill's product. `Stored` sets `last_result_step`, which made the
        // convergence signal the cascade's final result — 49 of ~58 registry
        // manifests end in `…compute → loop`, so their skills returned bare
        // numbers ("0") instead of the last select step's report. Named
        // storage keeps the value reachable as `step_{ordinal}_result` (via
        // the typed results map) AND `step_{ordinal}_compute`, while
        // `last_result_step` stays on the last select/render/execute step —
        // the same rule the populate/render named path already follows.
        Ok(Effect::StoredNamed {
            step_id: node.id,
            suffix: "compute".to_string(),
            value: result,
        })
    }

    /// **Render** — render a template without inference.
    pub(crate) async fn execute_render(
        &mut self,
        node: crate::step_graph::StepNode,
        infra: Infra,
    ) -> Result<Effect> {
        let rendered = render_step_template(&node, &self.context, &infra)?;
        Ok(Effect::Stored {
            step_id: node.id,
            value: Value::String(rendered),
        })
    }

    /// **Execute** — invoke an MCP tool with parameters bound from context.
    pub(crate) async fn execute_tool_invoke(
        &mut self,
        node: crate::step_graph::StepNode,
        infra: Infra,
    ) -> Result<Effect> {
        // Batch path: when `mcp_batch` is present, invoke all tools concurrently,
        // each gated by the global concurrency limiter. Results are collected
        // into a `Value::Object` keyed by `entry.key` (defaulting to the tool
        // name). Mutually exclusive with the single `mcp` path.
        // Clone `mcp_batch` out of `node` so `node` can be moved by value into
        // `execute_tool_batch` — `ref batch` would hold an immutable borrow of
        // `node` across the move.
        if let Some(batch) = node.mcp_batch.clone() {
            return self.execute_tool_batch(node, (*batch).clone(), infra).await;
        }

        let mcp_ref_raw = node.mcp.as_deref().ok_or_else(|| {
            TemplateError::Manifest(format!(
                "Execute step {} has no mcp reference",
                node.ordinal
            ))
        })?;
        // Clone to an owned `String` so the `&str` borrow of `node.mcp` doesn't
        // live in the async fn's future type (HRTB `Send` check under
        // `tokio::spawn`).
        let mcp_ref_raw = mcp_ref_raw.to_string();

        // Resolve ${variable} references in the MCP reference.
        let mcp_ref =
            crate::template_renderer::TemplateRenderer::render_inline(&mcp_ref_raw, &self.context);

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

        // Gate the tool call with the global concurrency limiter (same as
        // `execute_select` and `execute_tool_batch`). MCP tools may call cloud
        // backends, so they draw from the same process-wide pool. The permit is
        // held across the call and released on drop (including on timeout).
        let _permit = if let Some(ref limiter) = infra.concurrency_limiter {
            Some(limiter.acquire().await)
        } else {
            None
        };

        // Invoke the tool with a timeout. Without this, a hung MCP tool call
        // blocks the cascade forever — the tokio task has no external watchdog.
        // Clone `infra.tools` into a standalone owned local before the await so
        // the borrow is of a local, not of `infra.tools` (rustc's HRTB `Send`
        // check rejects field borrows held across `.await` under `tokio::spawn`).
        let timeout_dur = effective_timeout(node.timeout_seconds);
        let tools = infra.tools.clone();
        let mcp_ref_for_tracking = mcp_ref.clone();
        let tool_result =
            match tokio::time::timeout(timeout_dur, invoke_tool(tools, mcp_ref, input)).await {
                Ok(inner) => inner,
                Err(_elapsed) => {
                    // Record the timed-out tool call before propagating.
                    // Without this, grounding cannot distinguish "tool timed
                    // out" from "tool never called" — a timed-out call that
                    // supplied no data is an Unsourced field, not an absent
                    // one (paper: absence ≠ verdict). The `ok` flag is false
                    // so `failed_tools` will include it, giving the operator
                    // the `tool_failed` remediation signal (retry vs. wire up).
                    self.tool_calls.push(serde_json::json!({
                        "tool": mcp_ref_for_tracking,
                        "ok": false,
                    }));
                    return Err(TemplateError::Timeout {
                        step_ordinal: node.ordinal,
                        elapsed_seconds: timeout_dur.as_secs(),
                    });
                }
            };

        // Ramp the limiter based on the call outcome before propagating.
        if let Some(ref limiter) = infra.concurrency_limiter {
            match &tool_result {
                Ok(_) => limiter.on_success(),
                Err(e) if e.is_throttle() => limiter.on_throttle(),
                _ => {}
            }
        }

        // Record the tool call for grounding enforcement (Phase 5). The
        // summary shape matches `LocalDelegateResult.tool_calls`:
        // `{"tool": "server/tool_name", "ok": true/false, "result": <value>}`.
        // The `result` field carries the tool's return value so grounding
        // can value-match (Truth rung) — without it, "sourced" only means
        // "the tool ran," not "the value came from the tool." Recorded
        // before the `?` so both success and failure paths are captured.
        // On failure, `result` is absent (a failed call supplied no data).
        let summary =
            tool_call_summary(&mcp_ref_for_tracking, tool_result.as_ref().map_err(|_| ()));
        self.tool_calls.push(summary);

        let result = tool_result?;

        Ok(Effect::Stored {
            step_id: node.id,
            value: result,
        })
    }

    /// **Tool batch** — invoke multiple MCP tools concurrently, each gated by
    /// the global concurrency limiter. Results are collected into a
    /// `Value::Object` keyed by `entry.key` (defaulting to the tool name).
    /// All tools share one `tokio::time::timeout` (the step's
    /// `timeout_seconds`) — a batch is one logical step, not N independent
    /// steps with individual timeouts.
    ///
    /// Error semantics: if any tool fails, the step fails (the first error
    /// propagates). A partial-success mode (collect errors per-key) is a
    /// future extension.
    ///
    /// Join mode (read from `input_mapping.join`):
    ///   - `list` (default, backward-compat): Promise.all — first tool `Err`
    ///     aborts the batch. Sibling tool results are dropped.
    ///   - `allSettled`: Promise.allSettled (ECMA-262 §27.2.4.2) — collect
    ///     every tool outcome, store Ok results under `results` with an
    ///     `errors` sidecar. No tool outcome is silently dropped.
    pub(crate) async fn execute_tool_batch(
        &mut self,
        node: crate::step_graph::StepNode,
        batch: Vec<crate::bundle::manifest::McpBatchEntry>,
        infra: Infra,
    ) -> Result<Effect> {
        use futures_util::future::join_all;

        let timeout_dur = effective_timeout(node.timeout_seconds);
        let limiter = infra.concurrency_limiter.clone();
        let tools = Arc::clone(&infra.tools);

        // Build the per-tool futures. Each acquires a permit from the global
        // limiter before invoking, then calls `on_success` / `on_throttle`
        // after. The permit is held for the call's lifetime.
        let tool_futs = batch.iter().map(|entry| {
            let mcp_ref_raw = entry.mcp.clone();
            let input_mapping = entry.input_mapping.clone();
            let key = entry.key.clone();
            let limiter = limiter.clone();
            let tools = Arc::clone(&tools);
            let context = self.context.clone();
            let template_renderer = infra.template_renderer.clone();

            let tool_future = async move {
                // Wrap the tool call in `catch_unwind` INSIDE the async block so
                // the `Box<dyn Any + Send>` panic payload is consumed here, not
                // held in the outer future's type. rustc's HRTB `Send` check
                // rejects `Box<dyn Any + Send>` held across the `tokio::spawn`
                // boundary (the `Any` trait requires `'static`, so
                // `Box<dyn Any + Send + 'a>` for non-static `'a` is not Send).
                let inner = std::panic::AssertUnwindSafe(async {
                    // Resolve ${variable} references in the MCP reference.
                    let mcp_ref = crate::template_renderer::TemplateRenderer::render_inline(
                        &mcp_ref_raw,
                        &context,
                    );

                    // Resolve the tool input.
                    let input: Value = input_mapping
                        .as_ref()
                        .map(|mapping| {
                            crate::input_mapping::resolve_mapping_value(
                                mapping,
                                &context,
                                &template_renderer,
                            )
                        })
                        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

                    // Acquire a permit before issuing the call. When no limiter
                    // is wired (tests), skip gating.
                    let _permit = if let Some(ref limiter) = limiter {
                        Some(limiter.acquire().await)
                    } else {
                        None
                    };

                    let result = invoke_tool(tools, mcp_ref, input).await;

                    // Ramp the limiter based on the call outcome.
                    if let Some(ref limiter) = limiter {
                        match &result {
                            Ok(_) => limiter.on_success(),
                            Err(e) if e.is_throttle() => limiter.on_throttle(),
                            _ => {} // deterministic errors don't back off
                        }
                    }

                    result
                })
                .catch_unwind()
                .await;

                match inner {
                    Ok(result) => {
                        // Derive the result key: explicit `key` if provided, else the
                        // tool name (last segment of the mcp ref after `/` or `.`).
                        let result_key = key.unwrap_or_else(|| {
                            mcp_ref_raw
                                .rsplit(['/', '.'])
                                .next()
                                .unwrap_or(&mcp_ref_raw)
                                .to_string()
                        });
                        result.map(|value| (result_key, value))
                    }
                    Err(panic_payload) => {
                        let panic_msg = panic_payload
                            .downcast_ref::<String>()
                            .map(String::as_str)
                            .or_else(|| panic_payload.downcast_ref::<&str>().copied())
                            .unwrap_or("<non-string panic payload>");
                        tracing::error!(
                            target: "reg.skill.cascade.tool_batch_joined",
                            step_ordinal = node.ordinal,
                            panic_message = panic_msg,
                            "tool batch entry panicked — converted to typed error"
                        );
                        Err(TemplateError::Manifest(format!(
                            "Step {} (action 'mcp_batch') tool panicked: {panic_msg}",
                            node.ordinal,
                        )))
                    }
                }
            };
            tool_future
        });

        // Run all tool futures concurrently under one shared timeout.
        let batch_started = std::time::Instant::now();
        let results = match tokio::time::timeout(timeout_dur, join_all(tool_futs)).await {
            Ok(joined) => joined,
            Err(_elapsed) => {
                tracing::warn!(
                    target: "reg.skill.cascade.tool_batch_joined",
                    step_ordinal = node.ordinal,
                    tool_count = batch.len(),
                    ok_count = 0,
                    err_count = 0,
                    elapsed_ms = batch_started.elapsed().as_millis(),
                    "tool batch timed out"
                );
                // Record every batch entry as a failed tool call before
                // propagating. On timeout, `join_all` did not return, so the
                // recording loop below is unreachable — without this, a
                // timed-out batch leaves zero tool_calls entries and grounding
                // cannot distinguish "all tools timed out" from "no tools
                // called" (paper: absence ≠ verdict). Each entry is recorded
                // with `ok: false` so `failed_tools` includes it.
                //
                // Render the mcp ref through the template engine so the
                // recorded tool name matches what `execute_tool_invoke`
                // records (the resolved ref, not the raw template string).
                // Grounding enforcement matches tool names against contract
                // sources — an unrendered template would never match.
                for entry in batch.iter() {
                    let rendered = crate::template_renderer::TemplateRenderer::render_inline(
                        &entry.mcp,
                        &self.context,
                    );
                    self.tool_calls.push(serde_json::json!({
                        "tool": rendered,
                        "ok": false,
                    }));
                }
                return Err(TemplateError::Timeout {
                    step_ordinal: node.ordinal,
                    elapsed_seconds: timeout_dur.as_secs(),
                });
            }
        };
        let elapsed_ms = batch_started.elapsed().as_millis();

        // Record tool calls for grounding enforcement (Phase 5). Each batch
        // entry is recorded with its rendered MCP ref and success/failure
        // status. The results vec is in the same order as the batch entries
        // (join_all preserves order). Rendering the mcp ref through the
        // template engine ensures the recorded tool name matches the
        // resolved reference that `execute_tool_invoke` would record —
        // grounding enforcement matches tool names against contract sources,
        // so an unrendered template string would never match.
        for (entry, result) in batch.iter().zip(results.iter()) {
            let rendered = crate::template_renderer::TemplateRenderer::render_inline(
                &entry.mcp,
                &self.context,
            );
            let summary =
                tool_call_summary(&rendered, result.as_ref().map(|(_, v)| v).map_err(|_| ()));
            self.tool_calls.push(summary);
        }

        let tool_count = results.len();
        let (oks, errs): (Vec<_>, Vec<_>) = results.into_iter().partition(Result::is_ok);

        // Observability at the consolidation boundary.
        tracing::info!(
            target: "reg.skill.cascade.tool_batch_joined",
            step_ordinal = node.ordinal,
            tool_count,
            ok_count = oks.len(),
            err_count = errs.len(),
            elapsed_ms = elapsed_ms,
            "REG tool batch joined"
        );

        // allSettled semantics: collect every tool outcome. Successful results
        // go at the top level, failures go into an `errors` sidecar. If every
        // tool failed, propagate the first error.
        if oks.is_empty() {
            if errs.is_empty() {
                return Err(TemplateError::Manifest(format!(
                    "Step {} (action 'mcp_batch') has an empty batch — no tools to execute",
                    node.ordinal
                )));
            }
            let first_err = errs.into_iter().next().unwrap().unwrap_err();
            tracing::warn!(
                target: "reg.skill.cascade.tool_batch_joined",
                step_ordinal = node.ordinal,
                error_code = first_err.code(),
                "all tools in batch failed — propagating first error"
            );
            return Err(first_err);
        }

        let mut map = serde_json::Map::new();
        for result in oks {
            let (key, value) = result.unwrap();
            map.insert(key, value);
        }
        if errs.is_empty() {
            return Ok(Effect::Stored {
                step_id: node.id,
                value: Value::Object(map),
            });
        }

        let err_summaries: Vec<Value> = errs
            .iter()
            .map(|e| {
                let err = e.as_ref().unwrap_err();
                Value::Object(serde_json::Map::from_iter([
                    ("code".to_string(), Value::String(err.code().to_string())),
                    ("message".to_string(), Value::String(err.to_string())),
                ]))
            })
            .collect();
        tracing::warn!(
            target: "reg.skill.cascade.tool_batch_joined",
            step_ordinal = node.ordinal,
            ok_count = map.len(),
            err_count = err_summaries.len(),
            "tool batch completed with partial failures — successful results preserved"
        );
        // Flat layout: merge successful results at the top level and attach
        // an `errors` sidecar. This keeps downstream `step_N_result.<key>`
        // mappings working without a `.results.` prefix, regardless of
        // whether all tools succeeded or some failed.
        map.insert("errors".to_string(), Value::Array(err_summaries));
        Ok(Effect::Stored {
            step_id: node.id,
            value: Value::Object(map),
        })
    }

    /// **FlowDef** — recursively execute a sub-manifest as a nested cascade.
    pub(crate) async fn execute_flowdef(
        &mut self,
        node: crate::step_graph::StepNode,
        infra: Infra,
    ) -> Result<Effect> {
        let template_ref = node.template_ref.as_deref().ok_or_else(|| {
            TemplateError::Manifest(format!(
                "Step {} has action='flowdef' but no template_ref",
                node.ordinal
            ))
        })?;
        // Clone to an owned `String` so the `&str` borrow of `node.template_ref`
        // doesn't live in the async fn's future type (HRTB `Send` check under
        // `tokio::spawn`).
        let template_ref = template_ref.to_string();

        // Resolve {{key}} references from context.
        let template_ref =
            crate::template_renderer::TemplateRenderer::render_inline(&template_ref, &self.context);

        let manifest_yaml =
            load_sub_manifest_yaml(&infra.template_renderer, &template_ref, node.ordinal)?;

        // Parse the sub-manifest.
        let mut sub_manifest = crate::manifest_loader::load_manifest_from_yaml(&manifest_yaml)
            .map_err(|e| {
                TemplateError::Manifest(format!(
                    "Step {}: failed to parse sub-manifest '{}': {}",
                    node.ordinal, template_ref, e
                ))
            })?;

        // Hard-enforce the step capacity cap on the sub-cascade. Previously
        // this path got only the advisory `tracing::warn!` from
        // `StepGraph::new` — an open loop where a sub-cascade could exceed
        // the cap and run to completion. The gate now fires in all three
        // orchestration paths (executor, flowdef, parallel).
        crate::step_graph::check_step_cap(
            sub_manifest.steps.len(),
            &format!(
                "Step {} flowdef sub-manifest '{}'",
                node.ordinal, template_ref
            ),
        )?;

        // Cap the sub-cascade's budget to the parent's remaining budget.
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
        let sub_budget = crate::budget::BudgetTracker::new(&sub_manifest.rjoule);
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
        let mut sub_machine = StepMachine::new(
            sub_graph,
            self.context.clone(),
            sub_budget,
            sub_convergence,
            sub_manifest.error_handling.clone(),
            format!("{}::flowdef", self.manifest_id),
        );
        sub_machine.depth = self.depth + 1;

        let sub_outcome = Box::pin(sub_machine.run(infra.clone())).await?;

        // Extract the sub-cascade's final result via the canonical selector
        // (same as `execute_parallel`): `last_result_step` → normalize.
        let result_value = crate::executor::extract_final_step_result(&sub_outcome);

        // Merge the sub-cascade's updates back into the parent — keep only the
        // parent's original keys (step ids, protocol keys, named keys); drop
        // sub-only keys.
        self.context.merge_back_sub_cascade(
            &sub_outcome.context,
            &parent_step_ids,
            &parent_protocol_keys,
            &parent_named_keys,
        );

        // Deduct the sub-cascade's actual rJoule consumption.
        self.budget
            .consume_child(sub_outcome.budget_snapshot.rjoule_used);

        Ok(Effect::Stored {
            step_id: node.id,
            value: result_value,
        })
    }

    /// **Parallel** — run a list of sub-cascades concurrently (K2). Branches
    /// live under `input_mapping.branches`; `concurrency_cap` bounds in-flight
    /// branches; `join` is `"list"` (first cut). Each branch: its own
    /// `ConvergenceTracker` + a `BudgetTracker` that owns its rJoule (joined
    /// after via `charge_rjoule`). Results join by `branch_id` — deterministic,
    /// not completion order.
    pub(crate) async fn execute_parallel(
        &mut self,
        node: crate::step_graph::StepNode,
        infra: Infra,
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
        // `join` selects the consolidation discipline:
        //   - `list` (default, backward-compat): Promise.all semantics — the
        //     first branch `Err` aborts the wave. Sibling outcomes are dropped.
        //   - `allSettled`: Promise.allSettled semantics (ECMA-262 §27.2.4.2) —
        //     collect every branch outcome, store Ok results with an `errors`
        //     sidecar. No sibling outcome is silently dropped. See the
        //     consolidation block below for the full rationale.
        let join_mode = mapping
            .get("join")
            .and_then(|v| v.as_str())
            .unwrap_or("list")
            .to_string();
        // Drop `mapping` — everything below is owned, no borrows from locals.
        drop(mapping);

        // Per-branch rJoule (settled after the wave).
        let rjoule_remaining = self.budget.remaining_rjoule();
        let context_template = self.context.clone();
        let parent_manifest_id = self.manifest_id.clone();
        // The global concurrency limiter gates how many branches run
        // concurrently. `None` (tests, pre-startup) means unbounded —
        // `buffer_unordered(branches.len())` preserves the prior behavior.
        let concurrency_limiter = infra.concurrency_limiter.clone();
        let buffer_bound = concurrency_limiter
            .as_ref()
            .map(|l| l.max() as usize)
            .unwrap_or(branches.len());
        let parent_depth = self.depth;

        let branch_futs = branches.into_iter().enumerate().map(|(branch_id, spec)| {
            // `run` now owns the `Infra` (so its future is `Send + 'static` and
            // tokio-spawnable); clone `infra` + `context_template` per branch so
            // each `async move` owns its own.
            let infra = infra.clone();
            let context_template = context_template.clone();
            let branch_manifest_id = parent_manifest_id.clone();
            let template_ref = spec
                .get("template_ref")
                .and_then(|v| v.as_str())
                .map(String::from);
            // Clone `branch_id` for the panic handler — the `async move` block
            // below moves `branch_id` into the branch future, so the
            // `catch_unwind` map closure needs its own copy to name the branch
            // in the panic error message.
            let branch_id_for_panic = branch_id;
            let branch_future = async move {
                // Wrap the branch sub-cascade in `catch_unwind` INSIDE the
                // async block so the `Box<dyn Any + Send>` panic payload is
                // consumed here, not held in the outer future's type (HRTB
                // `Send` check under `tokio::spawn`).
                let inner = std::panic::AssertUnwindSafe(async {
                    // No branch-level permit: the global limiter gates the inner
                    // `execute_select` / `execute_tool_invoke` calls inside each
                    // branch's sub-cascade. Acquiring a branch permit here would
                    // double-count with the inner call's permit and deadlock (both
                    // draw from the same semaphore). The `buffer_bound` below caps
                    // how many branches are polled at once; the limiter caps how
                    // many inference calls those branches make concurrently.
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
                    let manifest_yaml = load_sub_manifest_yaml(
                        &infra.template_renderer,
                        &template_ref,
                        step_ordinal,
                    )?;
                    let sub_manifest = crate::manifest_loader::load_manifest_from_yaml(
                        &manifest_yaml,
                    )
                    .map_err(|e| {
                        TemplateError::Manifest(format!(
                            "Step {} parallel branch {}: failed to parse \
                                 sub-manifest '{}': {}",
                            step_ordinal, branch_id, template_ref, e,
                        ))
                    })?;
                    // Hard-enforce the step capacity cap on each parallel
                    // branch's sub-cascade. Previously this path got only the
                    // advisory `tracing::warn!` from `StepGraph::new`.
                    crate::step_graph::check_step_cap(
                        sub_manifest.steps.len(),
                        &format!(
                            "Step {} parallel branch {} sub-manifest '{}'",
                            step_ordinal, branch_id, template_ref
                        ),
                    )?;
                    let sub_budget = crate::budget::BudgetTracker::from_remaining(rjoule_remaining);
                    let sub_convergence =
                        crate::convergence::ConvergenceTracker::new(&sub_manifest.convergence);
                    let sub_graph = crate::step_graph::StepGraph::new(
                        &sub_manifest.steps,
                        sub_manifest.convergence.max_iterations,
                    );
                    let sub_manifest_id = branch_manifest_id.clone();
                    let mut sub_machine = StepMachine::new(
                        sub_graph,
                        context_template.clone(),
                        sub_budget,
                        sub_convergence,
                        sub_manifest.error_handling.clone(),
                        format!("{}::parallel", sub_manifest_id),
                    );
                    sub_machine.depth = parent_depth + 1;
                    let outcome = sub_machine.run(infra).await?;
                    // No branch-level limiter ramp: the inner `execute_select` /
                    // `execute_tool_invoke` calls already call `on_success` /
                    // `on_throttle` on the shared limiter. Ramp here would
                    // double-count (one ramp per branch + one per inner call).
                    Ok::<(usize, CascadeOutcome), TemplateError>((branch_id, outcome))
                })
                .catch_unwind()
                .await;
                match inner {
                    Ok(result) => result,
                    Err(panic_payload) => {
                        let panic_msg = panic_payload
                            .downcast_ref::<String>()
                            .map(String::as_str)
                            .or_else(|| panic_payload.downcast_ref::<&str>().copied())
                            .unwrap_or("<non-string panic payload>");
                        tracing::error!(
                            target: "reg.skill.cascade.parallel_joined",
                            step_ordinal = step_ordinal,
                            branch_id = branch_id_for_panic,
                            panic_message = panic_msg,
                            "parallel branch panicked — converted to typed error"
                        );
                        Err(TemplateError::Manifest(format!(
                            "Step {} (action 'parallel') branch {} panicked: {panic_msg}",
                            step_ordinal, branch_id_for_panic,
                        )))
                    }
                }
            };
            branch_future
        });

        // Bounded concurrency: poll up to `buffer_bound` branch futures at
        // once. The `buffer_bound` is the upper limit on how many branches
        // are polled concurrently; the global limiter (via the inner
        // `execute_select` / `execute_tool_invoke` permits) gates the actual
        // inference / tool-call concurrency. `buffer_unordered` yields in
        // completion order; we sort by `branch_id` below for a deterministic
        // join.
        //
        // Throttle handling: a branch that returns `Err` with a throttle-class
        // error has already called `on_throttle` via the inner `execute_select`
        // / `execute_tool_invoke` ramp logic. No outer wrapper is needed — the
        // inner call is the first to see the error and backs off the limiter
        // before the `?` propagates it out of the branch.
        //
        // Join mode (the `join` field parsed above):
        //   - `list` (default, backward-compat): Promise.all semantics — the
        //     first branch `Err` aborts the wave and propagates. Sibling
        //     outcomes are dropped. This is the historical contract.
        //   - `allSettled`: Promise.allSettled semantics (ECMA-262 §27.2.4.2)
        //     — collect every branch outcome (Ok and Err), store the Ok
        //     results with an `errors` sidecar, and emit a `reg.*` span. No
        //     sibling outcome is silently dropped. The wave already runs every
        //     branch to completion under `buffer_unordered` (it polls all
        //     before `.collect` returns), so `allSettled` costs nothing extra —
        //     `list` was paying for it and throwing the results away.
        let wave_started = std::time::Instant::now();
        // Per-wave timeout: mirrors `execute_tool_batch`. Without this, a
        // branch whose sub-cascade hangs (e.g., an inference call with no
        // per-step timeout) blocks the wave until the outer cascade times out.
        // `node.timeout_seconds` is the step-level budget for the whole wave.
        // On timeout, in-flight branches are dropped; their inner permits
        // release via RAII (`OwnedSemaphorePermit` Drop). Partial results are
        // lost (the wave did not complete) — surfaced as a typed `Timeout`.
        let wave_timeout = effective_timeout(node.timeout_seconds);
        let settled: Vec<std::result::Result<(usize, CascadeOutcome), TemplateError>> =
            match tokio::time::timeout(
                wave_timeout,
                stream::iter(branch_futs)
                    .buffer_unordered(buffer_bound)
                    .collect::<Vec<_>>(),
            )
            .await
            {
                Ok(joined) => joined,
                Err(_elapsed) => {
                    tracing::warn!(
                        target: "reg.skill.cascade.parallel_joined",
                        step_ordinal = node.ordinal,
                        branch_count = 0,
                        ok_count = 0,
                        err_count = 0,
                        elapsed_ms = wave_started.elapsed().as_millis(),
                        join_mode = join_mode,
                        "parallel wave timed out"
                    );
                    return Err(TemplateError::Timeout {
                        step_ordinal: node.ordinal,
                        elapsed_seconds: wave_timeout.as_secs(),
                    });
                }
            };
        let elapsed_ms = wave_started.elapsed().as_millis();

        let branch_count = settled.len();
        let (oks, errs): (Vec<_>, Vec<_>) = settled.into_iter().partition(Result::is_ok);

        // Observability at the consolidation boundary — closes the regulation
        // loop's sense input. Fires on both success and error paths so the
        // regulation layer can see per-wave branch_count / ok_count / err_count
        // and the wave duration. Without this span, a branch error that drops
        // sibling outcomes is invisible to `reg.*` consumers.
        tracing::info!(
            target: "reg.skill.cascade.parallel_joined",
            step_ordinal = node.ordinal,
            branch_count,
            ok_count = oks.len(),
            err_count = errs.len(),
            elapsed_ms = elapsed_ms,
            join_mode = join_mode,
            "REG parallel wave joined"
        );

        // `list` mode (default): Promise.all semantics. The first Err aborts
        // the step. rJoule from completed branches is settled before
        // propagating so the parent's budget doesn't underreport.
        if join_mode == "list" {
            if let Some(first_err) = errs.into_iter().next() {
                // Settle rJoule from completed branches even on the error path.
                let sum_rjoule: f64 = oks
                    .iter()
                    .filter_map(|r| r.as_ref().ok())
                    .map(|(_, o)| o.budget_snapshot.rjoule_used)
                    .sum();
                self.budget.charge_rjoule(sum_rjoule);
                return Err(first_err.unwrap_err());
            }
            let mut ordered: Vec<(usize, CascadeOutcome)> =
                oks.into_iter().map(|r| r.unwrap()).collect();
            ordered.sort_by_key(|(id, _)| *id);
            let branch_results: Vec<Value> = ordered
                .iter()
                .map(|(_, o)| crate::executor::extract_final_step_result(o))
                .collect();
            let sum_rjoule: f64 = ordered
                .iter()
                .map(|(_, o)| o.budget_snapshot.rjoule_used)
                .sum();
            self.budget.charge_rjoule(sum_rjoule);
            return Ok(Effect::Stored {
                step_id: node.id,
                value: Value::Array(branch_results),
            });
        }

        // `allSettled` mode: Promise.allSettled semantics. No sibling outcome
        // is silently dropped. If every branch failed, propagate the first
        // error (no partial result is meaningful). Otherwise store the partial
        // result + an `errors` sidecar so downstream steps and the operator
        // can see what survived.
        if oks.is_empty() {
            if errs.is_empty() {
                return Err(TemplateError::Manifest(format!(
                    "Step {} (action 'parallel') has an empty branches array — no branches to execute",
                    node.ordinal
                )));
            }
            let first_err = errs.into_iter().next().unwrap().unwrap_err();
            tracing::warn!(
                target: "reg.skill.cascade.parallel_joined",
                step_ordinal = node.ordinal,
                error_code = first_err.code(),
                "all parallel branches failed — propagating first error"
            );
            return Err(first_err);
        }

        let mut ordered: Vec<(usize, CascadeOutcome)> =
            oks.into_iter().map(|r| r.unwrap()).collect();
        ordered.sort_by_key(|(id, _)| *id);

        // Settle rJoule from completed branches (settled post-wave).
        let sum_rjoule: f64 = ordered
            .iter()
            .map(|(_, o)| o.budget_snapshot.rjoule_used)
            .sum();
        self.budget.charge_rjoule(sum_rjoule);

        // Build the result. If any branch errored, attach an `errors` sidecar
        // and emit a warn so the deviation is visible. The successful results
        // remain in `results` (branch_id order).
        if errs.is_empty() {
            let branch_results: Vec<Value> = ordered
                .iter()
                .map(|(_, o)| crate::executor::extract_final_step_result(o))
                .collect();
            return Ok(Effect::Stored {
                step_id: node.id,
                value: Value::Array(branch_results),
            });
        }

        let branch_results: Vec<Value> = ordered
            .iter()
            .map(|(_, o)| crate::executor::extract_final_step_result(o))
            .collect();
        let err_summaries: Vec<Value> = errs
            .iter()
            .map(|e| {
                let err = e.as_ref().unwrap_err();
                Value::Object(serde_json::Map::from_iter([
                    ("code".to_string(), Value::String(err.code().to_string())),
                    ("message".to_string(), Value::String(err.to_string())),
                ]))
            })
            .collect();
        tracing::warn!(
            target: "reg.skill.cascade.parallel_joined",
            step_ordinal = node.ordinal,
            ok_count = ordered.len(),
            err_count = err_summaries.len(),
            "parallel wave completed with partial failures — successful results preserved"
        );
        let mut map = serde_json::Map::new();
        map.insert("results".to_string(), Value::Array(branch_results));
        map.insert("errors".to_string(), Value::Array(err_summaries));
        Ok(Effect::Stored {
            step_id: node.id,
            value: Value::Object(map),
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
        std::time::Duration::from_secs(INFERENCE_TIMEOUT_FALLBACK_SECS)
    } else {
        std::time::Duration::from_secs(timeout_seconds as u64)
    }
}

/// Build the message array for a cascade step's inference call.
///
/// The message array is:
/// `[system: memory_context?, ...prior_messages, system: rendered_template, user: trigger]`
///
/// - `memory_context` (system): long-term memory snippets, formatted as a
///   single system message. Omitted when empty.
/// - `prior_messages`: short-term thread context (user/assistant turns from
///   the invoking thread). Empty when the cascade is invoked outside a thread.
/// - `rendered_template` (system): the actual Jinja2-rendered step prompt.
///   Sent as `system` to give it the semantic weight providers reserve for
///   system-level directives (stronger instruction adherence).
/// - `trigger` (user): "Execute the instructions above." — some providers
///   require at least one user message to produce output.
///
/// This matches the shape `agent_executor.rs` uses for swarm agents and the
/// shape `LanguageModelInferencePort::generate` already produces internally —
/// just with the prior-messages and memory arrays actually populated instead
/// of empty.
fn build_cascade_messages(
    prior_messages: &[ChatMessage],
    memory_snippets: &[MemorySnippet],
    rendered_template: &str,
    verbosity: Option<&str>,
) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(3 + prior_messages.len() + 1);

    // Verbosity instruction as a system message (if non-standard).
    // This is a soft lever — it instructs the model on output length without
    // imposing a hard token limit. Injected before the template so the model
    // processes it as a global constraint on its output style.
    if let Some(level) = verbosity {
        if let Some(instruction) = verbosity_instruction(level) {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: instruction.to_string(),
            });
        }
    }
    if !memory_snippets.is_empty() {
        let memory_text = format_cascade_memory_context(memory_snippets);
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: memory_text,
        });
    }

    // Short-term thread context.
    messages.extend(prior_messages.iter().cloned());

    // The rendered template as a system prompt.
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: rendered_template.to_string(),
    });

    // Trigger user message.
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: "Execute the instructions above.".to_string(),
    });

    messages
}

/// Map a verbosity level to a system-prompt instruction controlling output length.
/// Returns `None` for "standard" (no instruction — the default).
/// This is a soft lever: it instructs the model on conciseness without imposing
/// a hard token limit. The template's own instructions still take precedence
/// for fields that require specific content.
fn verbosity_instruction(level: &str) -> Option<&'static str> {
    match level {
        "terse" => Some(
            "Be extremely concise. Minimize unnecessary words and explanations. Prefer short sentences over paragraphs.",
        ),
        "concise" => Some("Be concise and direct. Avoid unnecessary elaboration."),
        "standard" => None,
        "detailed" => {
            Some("Be thorough and detailed in your analysis. Provide complete explanations.")
        }
        "verbose" => Some(
            "Be comprehensive and exhaustive in your analysis. Leave no relevant detail unexplored.",
        ),
        _ => {
            tracing::warn!(
                target: "hkask.templates.inference_block",
                verbosity = %level,
                "Unrecognized verbosity value — no instruction injected"
            );
            None
        }
    }
}

/// Format long-term memory snippets as a system message.
///
/// Uses a simple bulleted format with source tags — distinct from the
/// `format_recall_context` helper in the bridge (which uses data-boundary
/// markers for the chat path). The cascade path does not need the data
/// boundary because the memory is prepended as a separate system message,
/// not interleaved with user content.
fn format_cascade_memory_context(snippets: &[MemorySnippet]) -> String {
    let mut text = String::from(
        "Relevant long-term memory (from prior sessions and consolidated experiences):\n",
    );
    for (i, snippet) in snippets.iter().enumerate() {
        text.push_str(&format!(
            "\n{}. [{}] {}\n",
            i + 1,
            snippet.source,
            snippet.text
        ));
    }
    text
}

/// Call inference with streaming, timeout, and reasoning-delta forwarding,
/// using the message-array API (`generate_stream_with_messages`). Takes a
/// `&[ChatMessage]` so the provider sees the full conversation (memory +
/// prior turns + template) as proper role-tagged messages.
async fn call_inference_stream_with_messages(
    inference: Arc<dyn InferencePort + 'static>,
    messages: Vec<ChatMessage>,
    params: LLMParameters,
    model_override: Option<&str>,
    tools: Option<Vec<ChatToolDefinition>>,
    timeout: std::time::Duration,
    step_ordinal: u32,
    progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
) -> Result<(
    String,
    Vec<hkask_types::StructuredToolCall>,
    Option<f64>,
    Option<String>,
)> {
    // Defense in depth: if a caller passes Duration::ZERO, substitute fallback.
    let timeout = if timeout == std::time::Duration::ZERO {
        tracing::warn!(
            target: "hkask.templates.call_inference_stream",
            "timeout is Duration::ZERO — substituting {INFERENCE_TIMEOUT_FALLBACK_SECS}s fallback"
        );
        std::time::Duration::from_secs(INFERENCE_TIMEOUT_FALLBACK_SECS)
    } else {
        timeout
    };

    let stream = inference.generate_stream_with_messages(
        &messages,
        &params,
        model_override,
        tools.as_deref(),
    );

    let (full_text, tool_calls, cost_usd, finish_reason) =
        match tokio::time::timeout(timeout, async move {
            let mut full_text = String::new();
            let mut accumulated_tool_calls = Vec::new();
            let mut accumulated_cost_usd: Option<f64> = None;
            let mut accumulated_finish_reason: Option<String> = None;
            let mut stream = stream;
            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if !chunk.reasoning_delta.is_empty() {
                            if let Some(progress) = progress.as_ref() {
                                progress(&chunk.reasoning_delta);
                            }
                        }
                        if !chunk.text_delta.is_empty() {
                            full_text.push_str(&chunk.text_delta);
                        }
                        if !chunk.tool_calls.is_empty() {
                            accumulated_tool_calls.extend(chunk.tool_calls);
                        }
                        if chunk.cost_usd.is_some() {
                            accumulated_cost_usd = chunk.cost_usd;
                        }
                        if chunk.finish_reason.is_some() {
                            accumulated_finish_reason = chunk.finish_reason;
                        }
                    }
                    Err(e) => return Err(TemplateError::Inference(e)),
                }
            }
            Ok::<_, TemplateError>((
                full_text,
                accumulated_tool_calls,
                accumulated_cost_usd,
                accumulated_finish_reason,
            ))
        })
        .await
        {
            Ok(Ok((text, tool_calls, cost_usd, finish_reason))) => {
                (text, tool_calls, cost_usd, finish_reason)
            }
            Ok(Err(e)) => return Err(e),
            Err(_elapsed) => {
                return Err(TemplateError::Timeout {
                    step_ordinal,
                    elapsed_seconds: timeout.as_secs(),
                });
            }
        };

    Ok((full_text, tool_calls, cost_usd, finish_reason))
}

/// Resolve a tool's server and dispatch the call.
///
/// A FIDES taint gate (`DefaultPolicy::check` on a `Source`→`Sink` flow) used to
/// run here. It was removed rather than repaired: both of its inputs were
/// constants — every `ToolInfo` was labelled `Pure` at its only construction
/// site, and the untrusted-input flag read taint markers the context write side
/// had stopped emitting — so the block could never fire. Restoring the gate
/// means first giving tools real taint labels and propagating taint on write.
pub(crate) async fn invoke_tool(
    tools: Arc<dyn ToolPort + 'static>,
    tool_name: String,
    input: Value,
) -> Result<Value> {
    let tool_info = tools.get_tool_info(&tool_name).await.ok_or_else(|| {
        TemplateError::NotFound(hkask_types::NotFound {
            entity_type: "tool".to_string(),
            id: tool_name.to_string(),
        })
    })?;

    // Accounting identity for the call meter — not a credential. The cascade's
    // authority comes from which tools the manifest may name, not from this.
    let executor_webid = hkask_types::WebID::from_persona(b"manifest-executor");

    tools
        .invoke(&tool_info.server_id, &tool_name, input, executor_webid)
        .await
        .map_err(|error| TemplateError::Mcp(Box::new(error)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;

    #[test]
    fn effective_timeout_substitutes_fallback_for_zero() {
        // A zero timeout_seconds must not produce Duration::ZERO — that
        // causes tokio::time::timeout to fire immediately without polling
        // the inference future, silently breaking every select/execute step.
        let result = effective_timeout(0);
        assert_eq!(
            result,
            std::time::Duration::from_secs(INFERENCE_TIMEOUT_FALLBACK_SECS)
        );
    }

    #[test]
    fn effective_timeout_passes_through_nonzero() {
        let result = effective_timeout(120);
        assert_eq!(result, std::time::Duration::from_secs(120));

        let result = effective_timeout(1);
        assert_eq!(result, std::time::Duration::from_secs(1));
    }

    #[tokio::test]
    async fn call_inference_stream_with_messages_threads_finish_reason_length() {
        // zed-kask: D25 — `call_inference_stream_with_messages` must return the chunk's
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
        let (text, tool_calls, _cost, finish_reason) = call_inference_stream_with_messages(
            inference,
            vec![ChatMessage {
                role: "user".into(),
                content: "prompt".into(),
            }],
            LLMParameters::default(),
            None,
            None,
            std::time::Duration::from_secs(30),
            1,
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

    // zed-kask: D25 — pinning test for the execute_select truncation refusal.
    // The stream-level test above (call_inference_stream_with_messages_threads_finish_reason_length)
    // only asserts that finish_reason is threaded out of call_inference_stream_with_messages.
    // This test exercises the full execute_select path: when finish_reason is
    // "length" AND output_schema is set AND no structured tool call was emitted,
    // execute_select must return Err containing "truncated at token limit" — not
    // silently parse the partial text as JSON. Without this test, a refactor
    // could revert the refusal guard (step_actions.rs:345) and re-introduce the
    // silent-truncated-output bug the D25 comment describes.
    #[tokio::test]
    async fn execute_select_refuses_truncated_structured_output() {
        use crate::executor::ManifestExecutor;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        // A stub InferencePort that returns a truncated result: finish_reason
        // "length", no tool calls, partial JSON text. This is the exact shape
        // that triggers the D25 refusal guard in execute_select.
        struct TruncationInference;
        impl InferencePort for TruncationInference {
            fn generate(
                &self,
                _prompt: &str,
                _parameters: &LLMParameters,
                _tools: Option<&[ChatToolDefinition]>,
            ) -> Pin<
                Box<
                    dyn Future<
                            Output = std::result::Result<
                                hkask_types::InferenceResult,
                                hkask_types::InferenceError,
                            >,
                        > + Send
                        + '_,
                >,
            > {
                let result = hkask_types::InferenceResult {
                    text: "{\"partial\":".to_string(),
                    model: "test".into(),
                    usage: hkask_types::InferenceUsage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                    finish_reason: "length".into(),
                    token_probabilities: None,
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                };
                Box::pin(async move { Ok(result) })
            }
        }

        let inference = Arc::new(TruncationInference) as Arc<dyn InferencePort>;
        let executor =
            ManifestExecutor::new(inference, Arc::new(NoopToolPort), LLMParameters::default());

        // A 1-step select manifest with output_schema set. The output_schema
        // is what activates the D25 refusal guard — without it, a truncated
        // generation falls through to parse_json_response on the partial text.
        let manifest_yaml = r#"
manifest:
  id: test-truncation-refusal
  category: skill
steps:
  - ordinal: 1
    action: select
    description: "Structured output step"
    template_ref: "Return a JSON object with a result key"
    output_schema:
      type: object
      properties:
        result:
          type: string
      required: [result]
convergence:
  max_iterations: 1
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let result = executor
            .clone()
            .execute_manifest_into(manifest.clone(), std::collections::HashMap::new())
            .await;

        let err = result.expect_err(
            "execute_select must refuse a truncated structured-output generation, \
             not silently parse partial text as JSON",
        );
        let msg = err.to_string();
        assert!(
            msg.contains("truncated at token limit"),
            "error must mention truncation; got: {msg}"
        );
    }

    /// A2: empty-output guard. When the model returns no text and no tool
    /// call, `execute_select` must surface an actionable error naming the
    /// finish_reason and likely causes — not the cryptic
    /// "EOF while parsing a value at line 1 column 0" from
    /// `parse_json_response("")`. This test pins the stream-level shape
    /// (empty text, no tool calls, finish_reason threaded) that the guard in
    /// `execute_select` checks. The guard itself is exercised by the
    /// prompt-enhance cascade integration tests.
    #[tokio::test]
    async fn execute_select_empty_output_guard_pins_stream_shape() {
        use hkask_types::{InferenceError, InferenceResult, InferenceUsage};
        use std::future::Future;
        use std::pin::Pin;

        struct EmptyOutput;
        impl InferencePort for EmptyOutput {
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
                let result = InferenceResult {
                    text: String::new(),
                    model: "test".into(),
                    usage: InferenceUsage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                    finish_reason: "stop".into(),
                    token_probabilities: None,
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                };
                Box::pin(async move { Ok(result) })
            }
        }

        let inference = Arc::new(EmptyOutput) as Arc<dyn InferencePort>;
        let (text, tool_calls, _cost, finish_reason) = call_inference_stream_with_messages(
            inference,
            vec![ChatMessage {
                role: "user".into(),
                content: "prompt".into(),
            }],
            LLMParameters::default(),
            None,
            None,
            std::time::Duration::from_secs(30),
            1,
            None,
        )
        .await
        .expect("stream should complete");

        assert!(text.is_empty(), "empty-output case must produce empty text");
        assert!(
            tool_calls.is_empty(),
            "empty-output case must produce no tool calls"
        );
        assert_eq!(
            finish_reason.as_deref(),
            Some("stop"),
            "finish_reason threading is required for guard diagnostics"
        );
    }

    // ── Concurrency limiter gating tests (B1 fix) ──────────────────────

    /// A stub `InferencePort` that records peak concurrent `generate` calls.
    /// Used to verify the global concurrency limiter gates `execute_select`
    /// across concurrent cascades. Mirrors the OCR pipeline's
    /// `ConcurrentExecutor.peak` pattern.
    struct RecordingInference {
        active: std::sync::atomic::AtomicUsize,
        peak: std::sync::atomic::AtomicUsize,
    }

    impl RecordingInference {
        fn new() -> Self {
            Self {
                active: std::sync::atomic::AtomicUsize::new(0),
                peak: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn peak(&self) -> usize {
            self.peak.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl InferencePort for RecordingInference {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = std::result::Result<
                            hkask_types::InferenceResult,
                            hkask_types::InferenceError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            let active = &self.active;
            let peak = &self.peak;
            Box::pin(async move {
                let count = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                peak.fetch_max(count, std::sync::atomic::Ordering::SeqCst);
                // Sleep briefly to let other tasks acquire permits and overlap.
                // `yield_now` alone is not enough — the first task may complete
                // before the second acquires its permit.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                Ok(hkask_types::InferenceResult {
                    text: "{}".to_string(),
                    model: "test".into(),
                    usage: hkask_types::InferenceUsage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                    finish_reason: "stop".into(),
                    token_probabilities: None,
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    /// Minimal `ToolPort` stub for the limiter tests. Returns empty results;
    /// the select-step tests don't invoke tools.
    struct NoopToolPort;

    /// `ToolPort` stub that succeeds for every tool call. Used by tool-call
    /// tracking tests to verify that `tool_calls` records `ok: true` on the
    /// success path. Returns a static JSON object — the tests don't inspect
    /// the value, only the `tool_calls` summary.
    struct SuccessToolPort;

    impl hkask_capability::ToolPort for SuccessToolPort {
        fn invoke<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
            _args: serde_json::Value,
            _agent: hkask_types::WebID,
        ) -> hkask_capability::ToolFuture<
            'a,
            std::result::Result<serde_json::Value, hkask_capability::ToolPortError>,
        > {
            Box::pin(async { Ok(serde_json::json!({"result": "ok"})) })
        }
        fn discover_tools<'a>(&'a self) -> hkask_capability::ToolFuture<'a, Vec<String>> {
            Box::pin(async { vec!["test_tool".to_string()] })
        }
        fn get_tool_info<'a>(
            &'a self,
            tool_name: &'a str,
        ) -> hkask_capability::ToolFuture<'a, Option<hkask_capability::ToolInfo>> {
            Box::pin(async {
                Some(hkask_capability::ToolInfo {
                    name: tool_name.to_string(),
                    description: "test tool".to_string(),
                    input_schema: serde_json::json!({}),
                    server_id: "test_server".to_string(),
                })
            })
        }
    }

    /// `ToolPort` stub that succeeds for tools whose name contains "good"
    /// and fails for all others. Used by the tool-call consistency proptest
    /// to generate random success/failure patterns across a batch.
    struct MaskedToolPort;

    impl hkask_capability::ToolPort for MaskedToolPort {
        fn invoke<'a>(
            &'a self,
            _server: &'a str,
            tool: &'a str,
            _args: serde_json::Value,
            _agent: hkask_types::WebID,
        ) -> hkask_capability::ToolFuture<
            'a,
            std::result::Result<serde_json::Value, hkask_capability::ToolPortError>,
        > {
            let ok = tool.contains("good");
            Box::pin(async move {
                if ok {
                    Ok(serde_json::json!({"result": "ok"}))
                } else {
                    Err(hkask_capability::ToolPortError::NotFound(
                        hkask_types::NotFound {
                            entity_type: "tool".to_string(),
                            id: tool.to_string(),
                        },
                    ))
                }
            })
        }
        fn discover_tools<'a>(&'a self) -> hkask_capability::ToolFuture<'a, Vec<String>> {
            Box::pin(async { Vec::new() })
        }
        fn get_tool_info<'a>(
            &'a self,
            tool_name: &'a str,
        ) -> hkask_capability::ToolFuture<'a, Option<hkask_capability::ToolInfo>> {
            Box::pin(async move {
                Some(hkask_capability::ToolInfo {
                    name: tool_name.to_string(),
                    description: "masked tool".to_string(),
                    input_schema: serde_json::json!({}),
                    server_id: "test".to_string(),
                })
            })
        }
    }

    impl hkask_capability::ToolPort for NoopToolPort {
        fn invoke<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
            _args: serde_json::Value,
            _agent: hkask_types::WebID,
        ) -> hkask_capability::ToolFuture<
            'a,
            std::result::Result<serde_json::Value, hkask_capability::ToolPortError>,
        > {
            Box::pin(async {
                Err(hkask_capability::ToolPortError::NotFound(
                    hkask_types::NotFound {
                        entity_type: "tool".to_string(),
                        id: "noop".to_string(),
                    },
                ))
            })
        }
        fn discover_tools<'a>(&'a self) -> hkask_capability::ToolFuture<'a, Vec<String>> {
            Box::pin(async { Vec::new() })
        }
        fn get_tool_info<'a>(
            &'a self,
            _tool_name: &'a str,
        ) -> hkask_capability::ToolFuture<'a, Option<hkask_capability::ToolInfo>> {
            Box::pin(async { None })
        }
    }

    /// B1 regression guard: two concurrent cascades each with one `select`
    /// step, sharing a limiter with `max_concurrency: 1`, must serialize their
    /// inference calls (peak == 1). Without the `execute_select` gating fix,
    /// both cascades issue inference concurrently (peak == 2), defeating the
    /// process-wide ceiling.
    #[tokio::test]
    async fn execute_select_limiter_serializes_concurrent_cascades() {
        use crate::executor::ManifestExecutor;
        use hkask_types::concurrency::ConcurrencyLimiter;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        let inference = Arc::new(RecordingInference::new());
        let limiter = Arc::new(ConcurrencyLimiter::new(1, 1));

        // A minimal 1-step select manifest.
        let manifest_yaml = r#"
manifest:
  id: test-select
  category: skill
steps:
  - ordinal: 1
    action: select
    description: "Single select step"
    template_ref: "Return a JSON object with a result key"
convergence:
  max_iterations: 1
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let make_executor = || {
            ManifestExecutor::new(
                inference.clone(),
                Arc::new(NoopToolPort),
                LLMParameters::default(),
            )
            .with_concurrency_limiter(limiter.clone())
        };

        // Run two cascades concurrently.
        let e1 = make_executor();
        let e2 = make_executor();
        let (r1, r2) = tokio::join! {
            e1.execute_manifest_into(manifest.clone(), std::collections::HashMap::new()),
            e2.execute_manifest_into(manifest.clone(), std::collections::HashMap::new()),
        };
        r1.expect("cascade 1 succeeds");
        r2.expect("cascade 2 succeeds");

        assert_eq!(
            inference.peak(),
            1,
            "max_concurrency: 1 must serialize the two cascades' select calls"
        );
    }

    /// B1 regression guard: with `max_concurrency: 2`, the two concurrent
    /// cascades' `select` calls overlap (peak == 2). This confirms the
    /// limiter allows concurrency up to the ceiling, not just serializes.
    #[tokio::test]
    async fn execute_select_limiter_allows_concurrency_up_to_max() {
        use crate::executor::ManifestExecutor;
        use hkask_types::concurrency::ConcurrencyLimiter;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        let inference = Arc::new(RecordingInference::new());
        let limiter = Arc::new(ConcurrencyLimiter::new(2, 2));

        let manifest_yaml = r#"
manifest:
  id: test-select-2
  category: skill
steps:
  - ordinal: 1
    action: select
    description: "Single select step"
    template_ref: "Return a JSON object with a result key"
convergence:
  max_iterations: 1
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let make_executor = || {
            ManifestExecutor::new(
                inference.clone(),
                Arc::new(NoopToolPort),
                LLMParameters::default(),
            )
            .with_concurrency_limiter(limiter.clone())
        };

        let e1 = make_executor();
        let e2 = make_executor();
        let (r1, r2) = tokio::join! {
            e1.execute_manifest_into(manifest.clone(), std::collections::HashMap::new()),
            e2.execute_manifest_into(manifest.clone(), std::collections::HashMap::new()),
        };
        r1.expect("cascade 1 succeeds");
        r2.expect("cascade 2 succeeds");

        assert_eq!(
            inference.peak(),
            2,
            "max_concurrency: 2 must allow both cascades' select calls to overlap"
        );
    }

    // ── Empty batch/branches with allSettled guards ───────────────────

    /// An empty `mcp_batch` with `join: allSettled` must return a
    /// `TemplateError::Manifest` ("empty batch — no tools to execute"), not
    /// panic on `.unwrap()` when partitioning an empty results vec. Before the
    /// guard, `oks.is_empty() && errs.is_empty()` fell through to
    /// `errs.into_iter().next().unwrap()`, which panicked on `None`.
    #[tokio::test]
    async fn empty_mcp_batch_allsettled_returns_error_not_panic() {
        use crate::executor::ManifestExecutor;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        let inference = Arc::new(RecordingInference::new()) as Arc<dyn InferencePort>;
        let executor =
            ManifestExecutor::new(inference, Arc::new(NoopToolPort), LLMParameters::default());

        let manifest_yaml = r#"
manifest:
  id: test-empty-batch-allsettled
  category: skill
steps:
  - ordinal: 1
    action: execute
    description: "Empty mcp_batch with allSettled"
    mcp_batch: []
    input_mapping:
      join: allSettled
convergence:
  max_iterations: 1
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let result = executor
            .clone()
            .execute_manifest_into(manifest.clone(), std::collections::HashMap::new())
            .await;

        let err = result
            .expect_err("empty mcp_batch with allSettled must return Err, not panic on .unwrap()");
        let msg = err.to_string();
        assert!(
            msg.contains("empty batch"),
            "error must mention empty batch; got: {msg}"
        );
    }

    /// An empty `branches` array with `join: allSettled` must return a
    /// `TemplateError::Manifest` ("empty branches array — no branches to
    /// execute"), not panic on `.unwrap()` when partitioning an empty results
    /// vec. Same guard pattern as `execute_tool_batch`.
    #[tokio::test]
    async fn empty_branches_allsettled_returns_error_not_panic() {
        use crate::executor::ManifestExecutor;
        use hkask_types::concurrency::ConcurrencyLimiter;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        // A concurrency limiter is required so buffer_bound is at least 1.
        // Without it, buffer_bound falls back to branches.len() (0), and
        // buffer_unordered(0) never polls the inner stream, hanging instead
        // of discovering the stream is exhausted.
        let inference = Arc::new(RecordingInference::new()) as Arc<dyn InferencePort>;
        let limiter = Arc::new(ConcurrencyLimiter::new(1, 1));
        let executor =
            ManifestExecutor::new(inference, Arc::new(NoopToolPort), LLMParameters::default())
                .with_concurrency_limiter(limiter);

        let manifest_yaml = r#"
manifest:
  id: test-empty-branches-allsettled
  category: skill
steps:
  - ordinal: 1
    action: parallel
    description: "Empty branches with allSettled"
    input_mapping:
      branches: []
      join: allSettled
convergence:
  max_iterations: 1
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let result = executor
            .clone()
            .execute_manifest_into(manifest.clone(), std::collections::HashMap::new())
            .await;

        let err = result
            .expect_err("empty branches with allSettled must return Err, not panic on .unwrap()");
        let msg = err.to_string();
        assert!(
            msg.contains("empty branches array"),
            "error must mention empty branches; got: {msg}"
        );
    }

    // ── ParseFailure typed-variant retry ──────────────────────────────

    /// A stub `InferencePort` that always returns a truncated result
    /// (`finish_reason: "length"`, partial JSON, no tool calls). Each `generate`
    /// call increments the counter so the retry test can verify the typed
    /// `TemplateError::ParseFailure` variant is retried by `dispatch_with_retry`
    /// when `on_error: "retry"`.
    struct TruncationCountingInference {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl TruncationCountingInference {
        fn new() -> Self {
            Self {
                calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl InferencePort for TruncationCountingInference {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = std::result::Result<
                            hkask_types::InferenceResult,
                            hkask_types::InferenceError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            let calls = &self.calls;
            Box::pin(async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(hkask_types::InferenceResult {
                    text: "{\"partial\":".to_string(),
                    model: "test".into(),
                    usage: hkask_types::InferenceUsage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                    finish_reason: "length".into(),
                    token_probabilities: None,
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    /// Verifies that the typed `TemplateError::ParseFailure` variant is retried
    /// by `dispatch_with_retry` when `on_error: "retry"`. The
    /// `is_parse_failure` string matching was replaced with a typed variant
    /// match — this test pins that the typed match arm fires and retries.
    ///
    /// With `max_retries: 2` and `retry_backoff_seconds: 0`, the inference
    /// `generate` is called 3 times (1 initial + 2 retries) before the error
    /// propagates.
    #[tokio::test]
    async fn parse_failure_retries_with_typed_variant() {
        use crate::executor::ManifestExecutor;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        let inference = Arc::new(TruncationCountingInference::new());
        let executor = ManifestExecutor::new(
            inference.clone(),
            Arc::new(NoopToolPort),
            LLMParameters::default(),
        );

        // output_schema activates the D25 truncation refusal guard, which
        // returns TemplateError::ParseFailure (the typed variant).
        let manifest_yaml = r#"
manifest:
  id: test-parse-failure-retry
  category: skill
steps:
  - ordinal: 1
    action: select
    description: "Structured output step that always truncates"
    template_ref: "Return a JSON object with a result key"
    output_schema:
      type: object
      properties:
        result:
          type: string
      required: [result]
convergence:
  max_iterations: 1
error_handling:
  on_error: retry
  max_retries: 2
  retry_backoff_seconds: 0
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let result = executor
            .clone()
            .execute_manifest_into(manifest.clone(), std::collections::HashMap::new())
            .await;

        let err =
            result.expect_err("truncated output must produce an error after retries are exhausted");
        let msg = err.to_string();
        assert!(
            msg.contains("truncated at token limit"),
            "error must mention truncation; got: {msg}"
        );

        // 1 initial attempt + 2 retries = 3 generate calls. If the typed
        // ParseFailure variant match arm were broken, only 1 call would
        // happen (error propagates immediately without retry).
        assert_eq!(
            inference.calls(),
            3,
            "on_error: retry with max_retries: 2 must call generate 3 times (1 + 2 retries)"
        );
    }

    // ── Tool-call recording on the timeout path (Phase 5 grounding) ───

    /// A `ToolPort` whose `invoke` hangs forever for tools whose name
    /// starts with `test/hang`. Other tools (e.g. `curator_report_skill_use_issue`,
    /// used by the `on_failure: report` path) return a quick `NotFound` error so
    /// the cascade's `on_failure` handling completes and the `CascadeOutcome`
    /// (with `tool_calls`) is returned. Used to drive the `execute` step's
    /// timeout path so the test can verify the tool call is still recorded in
    /// `CascadeOutcome.tool_calls` (paper: absence ≠ verdict — a timed-out
    /// call that supplied no data is an Unsourced field, not an absent one).
    struct HangingToolPort;

    impl hkask_capability::ToolPort for HangingToolPort {
        fn invoke<'a>(
            &'a self,
            _server: &'a str,
            tool: &'a str,
            _args: serde_json::Value,
            _agent: hkask_types::WebID,
        ) -> hkask_capability::ToolFuture<
            'a,
            std::result::Result<serde_json::Value, hkask_capability::ToolPortError>,
        > {
            // `tool` is the full mcp ref (e.g. "test/hang") passed as
            // `tool_name` to `invoke_tool`.
            if tool.starts_with("test/hang") {
                Box::pin(async {
                    // Never resolves — the timeout fires first.
                    std::future::pending::<
                        std::result::Result<serde_json::Value, hkask_capability::ToolPortError>,
                    >()
                    .await
                })
            } else {
                // Non-hang tools (e.g. curator_report_skill_use_issue) return
                // a quick NotFound so the `on_failure: report` path completes
                // and the cascade exits with `Effect::Exit(Escalated)`,
                // preserving `tool_calls` in the returned `CascadeOutcome`.
                Box::pin(async {
                    Err(hkask_capability::ToolPortError::NotFound(
                        hkask_types::NotFound {
                            entity_type: "tool".to_string(),
                            id: tool.to_string(),
                        },
                    ))
                })
            }
        }
        fn discover_tools<'a>(&'a self) -> hkask_capability::ToolFuture<'a, Vec<String>> {
            Box::pin(async { Vec::new() })
        }
        fn get_tool_info<'a>(
            &'a self,
            tool_name: &'a str,
        ) -> hkask_capability::ToolFuture<'a, Option<hkask_capability::ToolInfo>> {
            // Return a ToolInfo so `invoke_tool` proceeds to `invoke` (which
            // hangs for test/hang*). Without this, `invoke_tool` returns
            // NotFound before the timeout path is reached, and the test would
            // not exercise the timeout recording branch.
            Box::pin(async move {
                Some(hkask_capability::ToolInfo {
                    name: tool_name.to_string(),
                    description: "hanging tool".to_string(),
                    input_schema: serde_json::json!({}),
                    server_id: "test".to_string(),
                })
            })
        }
    }

    /// A single `execute` step whose tool hangs must record the tool call
    /// in `CascadeOutcome.tool_calls` with `ok: false` even though the
    /// step times out and returns `Err`. Without the fix, the timeout path
    /// returned early before the `self.tool_calls.push(...)` line, leaving
    /// grounding unable to distinguish "tool timed out" from "tool never
    /// called."
    #[tokio::test]
    async fn execute_step_records_tool_call_on_timeout() {
        use crate::executor::ManifestExecutor;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        let inference = Arc::new(RecordingInference::new()) as Arc<dyn InferencePort>;
        let executor = ManifestExecutor::new(
            inference,
            Arc::new(HangingToolPort) as Arc<dyn hkask_capability::ToolPort>,
            LLMParameters::default(),
        );

        let manifest_yaml = r#"
manifest:
  id: test-timeout-record
  category: skill
steps:
  - ordinal: 1
    action: execute
    description: "Hanging tool call that times out"
    mcp: test/hang
    timeout_seconds: 1
    on_failure:
      action: escalate
      resume: "The tool timed out"
convergence:
  max_iterations: 1
error_handling:
  on_error: abort
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let result = executor
            .clone()
            .execute_manifest_into(manifest.clone(), std::collections::HashMap::new())
            .await;

        let outcome = result.expect(
            "execute_manifest returns Ok(CascadeOutcome) with ExitKind::Escalated when on_failure catches the timeout",
        );
        assert_eq!(
            outcome.exit_kind,
            crate::step_graph::ExitKind::Escalated,
            "timeout with on_failure: escalate must exit Escalated, not propagate Err",
        );
        // The tool call must be recorded with ok=false. Without the fix,
        // tool_calls is empty (the timeout returned before the push).
        assert_eq!(
            outcome.tool_calls.len(),
            1,
            "timed-out tool call must be recorded in tool_calls; got {:?}",
            outcome.tool_calls,
        );
        assert_eq!(
            outcome.tool_calls[0]["tool"].as_str(),
            Some("test/hang"),
            "recorded tool name must match the mcp ref",
        );
        assert_eq!(
            outcome.tool_calls[0]["ok"].as_bool(),
            Some(false),
            "timed-out tool call must be recorded with ok=false",
        );
    }

    /// An `mcp_batch` step whose tools all hang must record every batch
    /// entry in `CascadeOutcome.tool_calls` with `ok: false` even though the
    /// batch times out. Without the fix, the batch timeout returned early
    /// before the recording loop, leaving zero entries.
    #[tokio::test]
    async fn execute_batch_records_all_tool_calls_on_timeout() {
        use crate::executor::ManifestExecutor;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        let inference = Arc::new(RecordingInference::new()) as Arc<dyn InferencePort>;
        let executor = ManifestExecutor::new(
            inference,
            Arc::new(HangingToolPort) as Arc<dyn hkask_capability::ToolPort>,
            LLMParameters::default(),
        );

        let manifest_yaml = r#"
manifest:
  id: test-batch-timeout-record
  category: skill
steps:
  - ordinal: 1
    action: execute
    description: "Batch of hanging tools that all time out"
    mcp_batch:
      - mcp: test/hang_a
      - mcp: test/hang_b
      - mcp: test/hang_c
    timeout_seconds: 1
    on_failure:
      action: escalate
      resume: "The batch timed out"
convergence:
  max_iterations: 1
error_handling:
  on_error: abort
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let result = executor
            .clone()
            .execute_manifest_into(manifest.clone(), std::collections::HashMap::new())
            .await;

        let outcome = result.expect(
            "execute_manifest returns Ok(CascadeOutcome) with ExitKind::Escalated when on_failure catches the batch timeout",
        );
        assert_eq!(
            outcome.exit_kind,
            crate::step_graph::ExitKind::Escalated,
            "batch timeout with on_failure: escalate must exit Escalated, not propagate Err",
        );
        // All three batch entries must be recorded with ok=false. Without
        // the fix, tool_calls is empty (the batch timeout returned before
        // the recording loop).
        assert_eq!(
            outcome.tool_calls.len(),
            3,
            "all timed-out batch entries must be recorded; got {:?}",
            outcome.tool_calls,
        );
        for entry in &outcome.tool_calls {
            assert_eq!(
                entry["ok"].as_bool(),
                Some(false),
                "timed-out batch entry must be recorded with ok=false; got {entry:?}",
            );
        }
        let tool_names: std::collections::HashSet<Option<&str>> = outcome
            .tool_calls
            .iter()
            .map(|e| e["tool"].as_str())
            .collect();
        assert_eq!(
            tool_names,
            std::collections::HashSet::from([
                Some("test/hang_a"),
                Some("test/hang_b"),
                Some("test/hang_c")
            ]),
            "recorded tool names must match the batch mcp refs",
        );
    }

    // ── Tool-call summary consistency (Phase 5 grounding) ──────────────

    /// A single `execute` step whose tool succeeds must record the tool call
    /// in `CascadeOutcome.tool_calls` with `ok: true`. This is the success-path
    /// counterpart to the timeout tests above — grounding enforcement reads
    /// `tool_calls` to determine whether a field was sourced from a successful
    /// tool call.
    #[tokio::test]
    async fn execute_step_records_tool_call_on_success() {
        use crate::executor::ManifestExecutor;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        let inference = Arc::new(RecordingInference::new()) as Arc<dyn InferencePort>;
        let executor = ManifestExecutor::new(
            inference,
            Arc::new(SuccessToolPort) as Arc<dyn hkask_capability::ToolPort>,
            LLMParameters::default(),
        );

        let manifest_yaml = r#"
manifest:
  id: test-success-record
  category: skill
steps:
  - ordinal: 1
    action: execute
    description: "Successful tool call"
    mcp: test_server/test_tool
convergence:
  max_iterations: 1
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let outcome = executor
            .clone()
            .execute_manifest_into(manifest.clone(), std::collections::HashMap::new())
            .await
            .expect("cascade succeeds");

        assert_eq!(
            outcome.tool_calls.len(),
            1,
            "successful tool call must be recorded; got {:?}",
            outcome.tool_calls,
        );
        assert_eq!(
            outcome.tool_calls[0]["tool"].as_str(),
            Some("test_server/test_tool"),
            "recorded tool name must match the mcp ref",
        );
        assert_eq!(
            outcome.tool_calls[0]["ok"].as_bool(),
            Some(true),
            "successful tool call must be recorded with ok=true",
        );
    }

    /// A single `execute` step whose tool fails must record the tool call
    /// with `ok: false`. The `NoopToolPort` returns `NotFound` for every call.
    #[tokio::test]
    async fn execute_step_records_tool_call_on_failure() {
        use crate::executor::ManifestExecutor;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        let inference = Arc::new(RecordingInference::new()) as Arc<dyn InferencePort>;
        let executor = ManifestExecutor::new(
            inference,
            Arc::new(NoopToolPort) as Arc<dyn hkask_capability::ToolPort>,
            LLMParameters::default(),
        );

        let manifest_yaml = r#"
manifest:
  id: test-failure-record
  category: skill
steps:
  - ordinal: 1
    action: execute
    description: "Failing tool call"
    mcp: test_server/noop
    on_failure:
      action: escalate
      resume: "The tool failed"
convergence:
  max_iterations: 1
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let outcome = executor
            .clone()
            .execute_manifest_into(manifest.clone(), std::collections::HashMap::new())
            .await
            .expect("cascade exits Escalated via on_failure");

        assert_eq!(
            outcome.exit_kind,
            crate::step_graph::ExitKind::Escalated,
            "failed tool with on_failure must exit Escalated",
        );
        assert_eq!(
            outcome.tool_calls.len(),
            1,
            "failed tool call must be recorded; got {:?}",
            outcome.tool_calls,
        );
        assert_eq!(
            outcome.tool_calls[0]["ok"].as_bool(),
            Some(false),
            "failed tool call must be recorded with ok=false",
        );
    }

    /// An `mcp_batch` with mixed success/failure must record every entry in
    /// order with the correct `ok` status. `join: allSettled` collects partial
    /// results so the cascade doesn't abort on the first error.
    #[tokio::test]
    async fn execute_batch_records_mixed_success_failure_in_order() {
        use crate::executor::ManifestExecutor;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        // A tool port that succeeds for "good" tools and fails for "bad" ones.
        struct MixedToolPort;
        impl hkask_capability::ToolPort for MixedToolPort {
            fn invoke<'a>(
                &'a self,
                _server: &'a str,
                tool: &'a str,
                _args: serde_json::Value,
                _agent: hkask_types::WebID,
            ) -> hkask_capability::ToolFuture<
                'a,
                std::result::Result<serde_json::Value, hkask_capability::ToolPortError>,
            > {
                let ok = tool.contains("good");
                Box::pin(async move {
                    if ok {
                        Ok(serde_json::json!({"result": "ok"}))
                    } else {
                        Err(hkask_capability::ToolPortError::NotFound(
                            hkask_types::NotFound {
                                entity_type: "tool".to_string(),
                                id: tool.to_string(),
                            },
                        ))
                    }
                })
            }
            fn discover_tools<'a>(&'a self) -> hkask_capability::ToolFuture<'a, Vec<String>> {
                Box::pin(async { Vec::new() })
            }
            fn get_tool_info<'a>(
                &'a self,
                tool_name: &'a str,
            ) -> hkask_capability::ToolFuture<'a, Option<hkask_capability::ToolInfo>> {
                Box::pin(async move {
                    Some(hkask_capability::ToolInfo {
                        name: tool_name.to_string(),
                        description: "mixed tool".to_string(),
                        input_schema: serde_json::json!({}),
                        server_id: "test".to_string(),
                    })
                })
            }
        }

        let inference = Arc::new(RecordingInference::new()) as Arc<dyn InferencePort>;
        let executor = ManifestExecutor::new(
            inference,
            Arc::new(MixedToolPort) as Arc<dyn hkask_capability::ToolPort>,
            LLMParameters::default(),
        );

        let manifest_yaml = r#"
manifest:
  id: test-batch-mixed
  category: skill
steps:
  - ordinal: 1
    action: execute
    description: "Batch with mixed success/failure"
    mcp_batch:
      - mcp: test/good_a
      - mcp: test/bad_b
      - mcp: test/good_c
    input_mapping:
      join: allSettled
convergence:
  max_iterations: 1
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let outcome = executor
            .clone()
            .execute_manifest_into(manifest.clone(), std::collections::HashMap::new())
            .await
            .expect("allSettled batch succeeds with partial results");

        // Three entries, in batch order, with correct ok status.
        assert_eq!(
            outcome.tool_calls.len(),
            3,
            "all batch entries must be recorded; got {:?}",
            outcome.tool_calls,
        );
        assert_eq!(outcome.tool_calls[0]["tool"].as_str(), Some("test/good_a"));
        assert_eq!(outcome.tool_calls[0]["ok"].as_bool(), Some(true));
        assert_eq!(outcome.tool_calls[1]["tool"].as_str(), Some("test/bad_b"));
        assert_eq!(outcome.tool_calls[1]["ok"].as_bool(), Some(false));
        assert_eq!(outcome.tool_calls[2]["tool"].as_str(), Some("test/good_c"));
        assert_eq!(outcome.tool_calls[2]["ok"].as_bool(), Some(true));
    }

    // ── Rendered mcp_ref recording (item 3 fix) ───────────────────────

    /// A batch entry with a `{{ tool_name }}` template variable must record
    /// the *rendered* mcp ref in `tool_calls`, not the raw template string.
    /// Grounding enforcement matches tool names against contract sources —
    /// an unrendered template would never match, causing false-positive
    /// nulling of sourced fields.
    #[tokio::test]
    async fn execute_batch_records_rendered_mcp_ref_not_raw_template() {
        use crate::executor::ManifestExecutor;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        let inference = Arc::new(RecordingInference::new()) as Arc<dyn InferencePort>;
        let executor = ManifestExecutor::new(
            inference,
            Arc::new(SuccessToolPort) as Arc<dyn hkask_capability::ToolPort>,
            LLMParameters::default(),
        );

        let manifest_yaml = r#"
manifest:
  id: test-batch-rendered-ref
  category: skill
steps:
  - ordinal: 1
    action: execute
    description: "Batch with template variable in mcp ref"
    mcp_batch:
      - mcp: "test_server/{{tool_name}}"
    input_mapping:
      join: allSettled
convergence:
  max_iterations: 1
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(manifest_yaml).expect("parse manifest");

        let mut context = std::collections::HashMap::new();
        context.insert("tool_name".to_string(), serde_json::json!("my_tool"));

        let outcome = executor
            .clone()
            .execute_manifest_into(manifest.clone(), context)
            .await
            .expect("cascade succeeds");

        assert_eq!(outcome.tool_calls.len(), 1);
        assert_eq!(
            outcome.tool_calls[0]["tool"].as_str(),
            Some("test_server/my_tool"),
            "recorded tool name must be the rendered ref, not the raw template",
        );
        assert_eq!(
            outcome.tool_calls[0]["ok"].as_bool(),
            Some(true),
            "successful tool call must be recorded with ok=true",
        );
    }

    // ── Tool-call summary consistency proptest (Phase 5) ──────────────

    use proptest::prelude::*;

    proptest! {
        /// For a batch of N entries with a random success/failure pattern
        /// (at least one success — allSettled propagates Err when all fail),
        /// `CascadeOutcome.tool_calls` must have exactly N entries, in order,
        /// with the correct `ok` status and rendered tool name. This is the
        /// Phase 5 proptest — it verifies the recording logic is consistent
        /// across random batch sizes and outcomes.
        #[test]
        fn tool_batch_recording_consistent_with_batch_size_and_status(
            success_mask in proptest::collection::vec(any::<bool>(), 1..7)
                .prop_filter("at least one success", |v| v.iter().any(|&b| b)),
        ) {
            use crate::executor::ManifestExecutor;
            use hkask_types::template::LLMParameters;
            use std::sync::Arc;

            let batch_size = success_mask.len();

            // Build manifest YAML with batch entries based on success_mask.
            let mut batch_yaml = String::new();
            for (i, &ok) in success_mask.iter().enumerate() {
                let label = if ok { "good" } else { "bad" };
                batch_yaml.push_str(&format!("      - mcp: test/{label}_{i}\n"));
            }
            let manifest_yaml = format!(
                r#"manifest:
  id: test-proptest-batch
  category: skill
steps:
  - ordinal: 1
    action: execute
    description: "Proptest batch"
    mcp_batch:
{batch_yaml}    input_mapping:
      join: allSettled
convergence:
  max_iterations: 1
"#
            );

            let manifest = crate::manifest_loader::load_manifest_from_yaml(&manifest_yaml)
                .expect("parse manifest");

            let inference = Arc::new(RecordingInference::new()) as Arc<dyn InferencePort>;
            let executor = ManifestExecutor::new(
                inference,
                Arc::new(MaskedToolPort) as Arc<dyn hkask_capability::ToolPort>,
                LLMParameters::default(),
            );

            let rt = tokio::runtime::Runtime::new().expect("create runtime");
            let outcome = rt.block_on(executor.execute_manifest_into(
                manifest,
                std::collections::HashMap::new(),
            )).expect("allSettled batch with at least one success must produce Ok");

            prop_assert_eq!(
                outcome.tool_calls.len(),
                batch_size,
                "tool_calls count must match batch size"
            );

            for (i, (&expected_ok, tc)) in success_mask.iter().zip(outcome.tool_calls.iter()).enumerate() {
                let label = if expected_ok { "good" } else { "bad" };
                let expected_tool = format!("test/{label}_{i}");
                prop_assert_eq!(
                    tc["tool"].as_str(),
                    Some(expected_tool.as_str()),
                    "tool_calls[{}].tool must match expected name", i
                );
                prop_assert_eq!(
                    tc["ok"].as_bool(),
                    Some(expected_ok),
                    "tool_calls[{}].ok must match success_mask", i
                );
            }
        }
    }

    /// Without depth propagation in `execute_parallel`, a self-referencing
    /// parallel manifest hangs (stack-overflow). With the fix, the guard fires.
    #[tokio::test]
    async fn execute_parallel_propagates_matryoshka_depth() {
        use crate::executor::ManifestExecutor;
        use hkask_types::concurrency::ConcurrencyLimiter;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        let inference = Arc::new(RecordingInference::new()) as Arc<dyn InferencePort>;
        let limiter = Arc::new(ConcurrencyLimiter::new(1, 1));
        let tmp = std::env::temp_dir().join(format!("hkask-matryoshka-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        let self_ref_yaml = r#"
manifest:
  id: matryoshka-parallel-self-ref
  category: skill
convergence:
  max_iterations: 1
steps:
  - ordinal: 1
    action: parallel
    description: "self-ref"
    input_mapping:
      branches:
        - template_ref: matryoshka-parallel-self-ref
      join: allSettled
"#;
        std::fs::write(tmp.join("matryoshka-parallel-self-ref.yaml"), self_ref_yaml).unwrap();

        let executor = ManifestExecutor::new(
            inference,
            Arc::new(NoopToolPort) as Arc<dyn hkask_capability::ToolPort>,
            LLMParameters::default(),
        )
        .with_concurrency_limiter(limiter)
        .with_template_base_path(tmp.clone());

        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(self_ref_yaml).expect("parse");
        let result = executor
            .execute_manifest_into(manifest, std::collections::HashMap::new())
            .await;
        let _ = std::fs::remove_dir_all(&tmp);

        let msg = result
            .expect_err("must hit depth guard, not hang")
            .to_string();
        assert!(msg.contains("Matryoshka depth limit"), "got: {msg}");
    }

    /// Discriminates `execute_parallel`'s depth increment from `execute_flowdef`'s.
    /// A 2-level parallel-only manifest (no `flowdef` in any sub-cascade) must
    /// still hit the matryoshka guard. Without the `parent_depth + 1` line in
    /// `execute_parallel`, this test hangs (each parallel branch resets depth
    /// to 0, so the guard never fires).
    #[tokio::test]
    async fn execute_parallel_depth_increment_independent_of_flowdef() {
        use crate::executor::ManifestExecutor;
        use hkask_types::concurrency::ConcurrencyLimiter;
        use hkask_types::template::LLMParameters;
        use std::sync::Arc;

        let inference = Arc::new(RecordingInference::new()) as Arc<dyn InferencePort>;
        let limiter = Arc::new(ConcurrencyLimiter::new(1, 1));
        let tmp = std::env::temp_dir().join(format!(
            "hkask-matryoshka-parallel-only-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        // Self-referencing parallel manifest — no flowdef anywhere.
        let self_ref_yaml = r#"
manifest:
  id: parallel-only-self-ref
  category: skill
convergence:
  max_iterations: 1
steps:
  - ordinal: 1
    action: parallel
    description: "parallel-only self-ref"
    input_mapping:
      branches:
        - template_ref: parallel-only-self-ref
      join: allSettled
"#;
        std::fs::write(tmp.join("parallel-only-self-ref.yaml"), self_ref_yaml).unwrap();

        let executor = ManifestExecutor::new(
            inference,
            Arc::new(NoopToolPort) as Arc<dyn hkask_capability::ToolPort>,
            LLMParameters::default(),
        )
        .with_concurrency_limiter(limiter)
        .with_template_base_path(tmp.clone());

        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(self_ref_yaml).expect("parse");
        let result = executor
            .execute_manifest_into(manifest, std::collections::HashMap::new())
            .await;
        let _ = std::fs::remove_dir_all(&tmp);

        let msg = result
            .expect_err("parallel-only self-ref must hit depth guard")
            .to_string();
        assert!(msg.contains("Matryoshka depth limit"), "got: {msg}");
    }

    #[test]
    fn cap_string_value_caps_long_strings_and_preserves_other_types() {
        use serde_json::json;
        assert_eq!(cap_string_value(&json!("short")), json!("short"));
        let long = "x".repeat(64 * 1024 + 100);
        let capped = cap_string_value(&json!(long));
        assert_eq!(capped.as_str().unwrap().len(), 64 * 1024);
        assert_eq!(cap_string_value(&json!(42)), json!(42));
        assert_eq!(cap_string_value(&json!({"a": 1})), json!({"a": 1}));
        assert_eq!(cap_string_value(&json!(null)), json!(null));
    }

    #[test]
    fn tool_call_summary_shape_for_ok_and_err() {
        use serde_json::json;
        let ok = tool_call_summary("srv/tool", Ok(&json!({"path": "/x"})));
        assert_eq!(ok["tool"], "srv/tool");
        assert_eq!(ok["ok"], true);
        assert_eq!(ok["result"]["path"], "/x");
        let err = tool_call_summary("srv/tool", Err(()));
        assert_eq!(err["tool"], "srv/tool");
        assert_eq!(err["ok"], false);
        assert!(err.get("result").is_none(), "err must not carry result");
    }

    #[test]
    fn tool_call_summary_caps_long_string_result() {
        use serde_json::json;
        let long = "x".repeat(64 * 1024 + 100);
        let summary = tool_call_summary("t", Ok(&json!(long)));
        assert_eq!(summary["result"].as_str().unwrap().len(), 64 * 1024);
    }
}
