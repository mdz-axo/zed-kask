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
use crate::step_machine::{CascadeOutcome, Infra, StepMachine};
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

        // Copy convergence signal from protocol to inputs so the
        // convergence tracker can find it.
        if let Some(v) = self.context.protocol("convergence_signal") {
            self.context
                .inputs
                .insert("convergence_signal".to_string(), v.clone());
        }

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

        let timeout_dur = std::time::Duration::from_secs(node.timeout_seconds as u64);

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
        let timeout_dur = std::time::Duration::from_secs(node.timeout_seconds as u64);
        let tools = infra.tools.clone();
        let mcp_ref_for_tracking = mcp_ref.clone();
        let tool_result =
            match tokio::time::timeout(timeout_dur, invoke_tool(tools, mcp_ref, input)).await {
                Ok(inner) => inner,
                Err(_elapsed) => {
                    // Record the timed-out tool call before propagating.
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
        // summary shape matches `LocalDelegateResult.tool_calls`:
        // `{"tool": "server/tool_name", "ok": true/false, "result": <value>}`.
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

        let timeout_dur = std::time::Duration::from_secs(node.timeout_seconds as u64);
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
                // cannot distinguish "all tools timed out" from "no tools
                // called" (paper: absence ≠ verdict). Each entry is recorded
                // with `ok: false` so `failed_tools` includes it.
                //
                // Render the mcp ref through the template engine so the
                // recorded tool name matches what `execute_tool_invoke`
                // records (the resolved ref, not the raw template string).
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
        // entry is recorded with its rendered MCP ref and success/failure
        // status. The results vec is in the same order as the batch entries
        // (join_all preserves order). Rendering the mcp ref through the
        // template engine ensures the recorded tool name matches the
        // resolved reference that `execute_tool_invoke` would record —
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
        let sub_convergence =
            crate::convergence::ConvergenceTracker::new(
                sub_manifest.convergence.max_iterations,
                sub_manifest.convergence.min_iterations,
                sub_manifest.convergence.threshold,
            );

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

        
        Ok(Effect::Stored {
            step_id: node.id,
            value: result_value,
        })
    }

    /// **Parallel** — run a list of sub-cascades concurrently (K2). Branches
    /// live under `input_mapping.branches`; `concurrency_cap` bounds in-flight
    /// branches; `join` is `"list"` (first cut). Each branch: its own
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

        let branch_count = branches.len();
        let batch_started = std::time::Instant::now();

        let mut tool_futs = Vec::with_capacity(branches.len());
        for (i, branch) in branches.iter().enumerate() {
            let template_ref = branch.get("template_ref").and_then(|v| v.as_str()).map(|s| s.to_string());
            let branch_id = branch.get("branch_id").and_then(|v| v.as_u64()).unwrap_or(i as u64) as usize;
            let timeout = effective_timeout(branch.get("timeout_seconds").and_then(|v| v.as_u64()).map(|s| s as u64));
            let limiter = infra.concurrency_limiter.clone();
            let depth = self.depth + 1;
            let ctx = self.context.clone();
            let err_handling = self.error_handling.clone();
            let manifest_id = self.manifest_id.clone();
            let infra_clone = infra.clone();
            tool_futs.push(async move {
                let limiter_guard = if let Some(ref l) = limiter { l.acquire().await } else { None };
                let _ = limiter_guard;
                let manifest = sub_manifest.ok_or_else(|| TemplateError::Manifest(
                    format!("parallel branch {i} has no manifest")
                ))?;
                let graph = crate::step_graph::StepGraph::new(&manifest.steps, manifest.convergence.max_iterations);
                let context = crate::step_context::StepContext::new(ctx.inputs.clone());
                let convergence = crate::convergence::ConvergenceTracker::new(
                    manifest.convergence.max_iterations,
                    manifest.convergence.min_iterations,
                    manifest.convergence.threshold,
                );
                let machine = crate::step_machine::StepMachine::new(
                    graph, context, convergence, err_handling, manifest_id,
                );
                let mut sub_machine = machine;
                sub_machine.depth = depth;
                let outcome = sub_machine.run(infra_clone).await?;
                Ok((branch_id, outcome))
            });
        }

        let results = match tokio::time::timeout(
            std::time::Duration::from_secs(node.timeout_seconds as u64),
            futures_util::future::join_all(tool_futs),
        ).await {
            Ok(joined) => joined,
            Err(_) => return Err(TemplateError::Timeout {
                step_ordinal: node.ordinal,
                elapsed_seconds: node.timeout_seconds,
            }),
        };

        let elapsed_ms = batch_started.elapsed().as_millis();
        let (oks, errs): (Vec<_>, Vec<_>) = results.into_iter().partition(Result::is_ok);

        tracing::info!(
            target: "reg.skill.cascade.parallel_joined",
            step_ordinal = node.ordinal,
            branch_count,
            ok_count = oks.len(),
            err_count = errs.len(),
            elapsed_ms,
            "REG parallel wave joined"
        );

        if oks.is_empty() {
            if errs.is_empty() {
                return Err(TemplateError::Manifest(format!(
                    "Step {} (action 'parallel') has no branches", node.ordinal
                )));
            }
            return Err(errs.into_iter().next().unwrap().unwrap_err());
        }

        let mut ordered: Vec<(usize, _)> = oks.into_iter().map(|r| r.unwrap()).collect();
        ordered.sort_by_key(|(id, _)| *id);

        if errs.is_empty() {
            let branch_results: Vec<Value> = ordered.iter()
                .map(|(_, o)| crate::executor::extract_final_step_result(o))
                .collect();
            return Ok(Effect::Stored {
                step_id: node.id,
                value: Value::Array(branch_results),
            });
        }

        let branch_results: Vec<Value> = ordered.iter()
            .map(|(_, o)| crate::executor::extract_final_step_result(o))
            .collect();
        let err_summaries: Vec<Value> = errs.iter().map(|e| {
            let err = e.as_ref().unwrap_err();
            Value::Object(serde_json::Map::from_iter([
                ("code".to_string(), Value::String(err.code().to_string())),
                ("message".to_string(), Value::String(err.to_string())),
            ]))
        }).collect();
        let mut map = serde_json::Map::new();
        map.insert("results".to_string(), Value::Array(branch_results));
        map.insert("errors".to_string(), Value::Array(err_summaries));
        Ok(Effect::Stored {
            step_id: node.id,
            value: Value::Object(map),
        })
    }
}

fn render_step_template(
    node: &crate::step_graph::StepNode,
    context: &crate::step_context::StepContext,
    infra: &Infra,
) -> Result<String> {
    let template_ref = node.template_ref.as_deref().ok_or_else(|| {
        TemplateError::Manifest(format!("Step {} has no template_ref", node.ordinal))
    })?;
    let template_content = infra.template_renderer.load(template_ref, 0)?;
    infra.template_renderer.render(&template_content, context)
}

fn render_step_template_with_raw(
    node: &crate::step_graph::StepNode,
    context: &crate::step_context::StepContext,
    infra: &Infra,
) -> Result<(String, String, crate::template_renderer::InferenceBlock)> {
    let template_ref = node.template_ref.as_deref().ok_or_else(|| {
        TemplateError::Manifest(format!("Step {} has no template_ref", node.ordinal))
    })?;
    let raw = infra.template_renderer.load(template_ref, 0)?;
    let (renderable, inference_block) = crate::template_renderer::parse_and_strip_inference_block(
        crate::template_renderer::strip_front_matter(&raw),
    );
    let rendered = infra.template_renderer.render(&renderable, context)?;
    Ok((rendered, raw, inference_block))
}

fn effective_timeout(seconds: Option<u64>) -> std::time::Duration {
    std::time::Duration::from_secs(seconds)
}

fn build_cascade_messages(
    prior_messages: &[ChatMessage],
    memory_snippets: &[MemorySnippet],
    prompt: &str,
    verbosity: Option<&str>,
) -> Vec<ChatMessage> {
    let mut messages = Vec::new();

    if !memory_snippets.is_empty() {
        let memory_text = memory_snippets
            .iter()
            .map(|s| format!("- {}", s.text))
            .collect::<Vec<_>>()
            .join("\n");
        messages.push(ChatMessage {
            role: "system".into(),
            content: format!("Long-term memory context:\n{memory_text}"),
        });
    }

    messages.extend(prior_messages.iter().cloned());

    if let Some(v) = verbosity {
        if !v.is_empty() && v != "standard" {
            messages.push(ChatMessage {
                role: "system".into(),
                content: format!("Be {v} in your response."),
            });
        }
    }

    messages.push(ChatMessage {
        role: "user".into(),
        content: prompt.to_string(),
    });

    messages
}

async fn call_inference_stream_with_messages(
    inference: Arc<dyn InferencePort + 'static>,
    messages: Vec<ChatMessage>,
    params: LLMParameters,
    model_override: Option<&str>,
    tools: Option<Vec<ChatToolDefinition>>,
    timeout: std::time::Duration,
    step_ordinal: u32,
    progress: Option<Arc<dyn Fn(&str) + Send + Sync>>,
) -> Result<(String, Vec<hkask_types::StructuredToolCall>, Option<f64>, Option<String>)> {
    let stream = inference.generate_stream_with_messages(&messages, &params, model_override, tools.as_deref());
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
        }).await {
            Ok(Ok(stuff)) => stuff,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(TemplateError::Timeout {
                step_ordinal,
                elapsed_seconds: timeout.as_secs(),
            }),
        };
    Ok((full_text, tool_calls, cost_usd, finish_reason))
}

fn invoke_tool(tools: Arc<dyn ToolPort>, tool_name: String, input: Value) -> Result<Value> {
    let info = tools.get_tool_info(&tool_name)?;
    let result = tools.invoke(&info.server_id, &info.tool_name, &input, std::sync::Arc::new(|| {}))?;
    Ok(result)
}

fn parse_json_response(text: &str, step_ordinal: u32) -> Result<Value> {
    let text = text.trim();
    if text.is_empty() {
        return Err(TemplateError::Manifest(format!(
            "Step {step_ordinal}: model returned empty output"
        )));
    }
    // Try direct parse
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        return Ok(v);
    }
    // Try extracting JSON from markdown code blocks
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            if let Ok(v) = serde_json::from_str::<Value>(after[..end].trim()) {
                return Ok(v);
            }
        }
    }
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        if let Some(end) = after.find("```") {
            if let Ok(v) = serde_json::from_str::<Value>(after[..end].trim()) {
                return Ok(v);
            }
        }
    }
    // Try finding first { or [ and last } or ]
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if let Ok(v) = serde_json::from_str::<Value>(&text[start..=end]) {
                return Ok(v);
            }
        }
    }
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            if let Ok(v) = serde_json::from_str::<Value>(&text[start..=end]) {
                return Ok(v);
            }
        }
    }
    Err(TemplateError::Manifest(format!(
        "Step {step_ordinal}: could not parse JSON from model output"
    )))
}
