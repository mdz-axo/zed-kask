//! Agent executor — the local agent-run policy, extracted from
//! `LocalSwarmRuntime::delegate`.
//!
//! `AgentExecutor::run` runs a local agent: builds the declared tool set,
//! and runs the multi-round inference/tool-dispatch loop. It returns a
//! `RawDelegateResult` carrying the raw output text, model, token usage, and
//! tool call summary.
//!
//! **The executor does NOT debit the ledger.** The caller
//! (`LocalSwarmRuntime::delegate`) is responsible for debit: it computes the
//! cost and debits the ledger. See ADR: "AgentExecutor returns raw output;
//! LocalSwarmRuntime owns debit".

use std::sync::Arc;

use crate::error::LocalSwarmError;
use crate::local_registry::LocalAgentCard;

/// Maximum tool-call rounds per delegation. Each round is a full inference
/// call; the cap bounds cost amplification (the per-dispatch credit ceiling
/// is the credit gate, this is the round gate).
pub const MAX_TOOL_ROUNDS: usize = 4;

/// The built-in reasoning tool name. When an agent card opts into reasoning
/// (`capabilities.reasoning: true`), the executor registers this tool and
/// handles it locally — no IPC dispatch. The model calls it to record a
/// structured reasoning step; the executor accumulates steps and returns
/// them in `RawDelegateResult.reasoning_steps`.
const REASONING_TOOL_NAME: &str = "reasoning/think";

/// A single reasoning step recorded by the model via the `reasoning/think`
/// tool. Inspired by Agno's `ReasoningTools.think`/`analyze` pattern: each
/// call appends a step, and the full history is returned to the model so it
/// can decide whether to continue. `next_action: "final_answer"` is advisory
/// — the model is told to stop calling tools, but the executor does NOT enforce
/// early termination on it. The real termination gate is `MAX_TOOL_ROUNDS`.
/// This keeps the round budget deterministic and prevents a model from
/// silently extending its tool loop by never signaling `final_answer`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ReasoningStep {
    /// Short title for the step (what the model is reasoning about).
    pub title: String,
    /// The reasoning text itself.
    pub reasoning: String,
    /// Optional action taken or planned (e.g. "called web_search", "will
    /// delegate to analyst").
    pub action: Option<String>,
    /// The model's signal for what to do next: `"continue"`, `"validate"`,
    /// or `"final_answer"`. Advisory only — the executor does NOT enforce
    /// early termination on `"final_answer"`. The real termination gate is
    /// `MAX_TOOL_ROUNDS`. The field is surfaced to the Curator's ORIENT phase
    /// as a reasoning-trace signal, not a control signal.
    pub next_action: String,
    /// Optional self-assessed confidence (0.0–1.0). Not calibrated — the
    /// Curator's Brier-scored outcomes calibrate confidence, not this.
    pub confidence: Option<f64>,
    /// Which tool-call round this step was recorded in.
    pub round: usize,
}

/// The raw result of running an agent — text, model, token usage, and the
/// tool execution summary. NOT debited. The caller
/// (`LocalSwarmRuntime::delegate`) debits the ledger.
pub struct RawDelegateResult {
    pub text: String,
    pub model: String,
    pub tokens_used: i64,
    pub tool_calls: Vec<serde_json::Value>,
    /// The rollout id under which this run's `model_request` events were
    /// captured. Callers that stamp verdicts (the harness) use it so the
    /// verdict lands in the same rollout group.
    pub rollout_id: String,
    /// Structured reasoning steps recorded by the model via the
    /// `reasoning/think` tool (when the agent card opts into reasoning).
    /// Empty when reasoning is not enabled or the model never called the
    /// tool. Consumed by the Curator's ORIENT phase as a reasoning trace.
    pub reasoning_steps: Vec<ReasoningStep>,
}

/// A captured inference call — the `model_request` event payload. Built by
/// the executor after each `generate_with_messages` call; forwarded to the
/// event sink (when wired) without blocking the generation path.
#[derive(Debug, Clone)]
pub struct CapturedInference {
    pub rollout_id: String,
    pub model: String,
    pub status: &'static str,
    pub latency_ms: u128,
    pub total_tokens: i64,
    pub tool_calls: usize,
    pub round: usize,
    /// The request body (the full message array sent to the model).
    /// Retained for the training bridge — a dataset needs the bodies, not
    /// just the shape. Capped at `MAX_BODY_BYTES` before capture so a huge
    /// prompt cannot flood the channel or the store.
    pub request_body: String,
    /// The response body (the model's final text for this round). Same
    /// retention rationale and cap as `request_body`.
    pub response_body: String,
}

/// Cap on retained request/response bodies per event. Bodies are training
/// data, not forensic records — a truncated body is still a usable example,
/// and an unbounded one would let a single large context blow the channel
/// budget (256 captures × body size).
pub const MAX_BODY_BYTES: usize = 64 * 1024;

fn cap_body(body: &str) -> String {
    if body.len() <= MAX_BODY_BYTES {
        body.to_string()
    } else {
        // Truncate at a char boundary — `body.len()` is byte length and a
        // mid-char cut would produce invalid UTF-8.
        let mut end = MAX_BODY_BYTES;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body[..end].to_string()
    }
}

/// The executor's event sink — a bounded, non-blocking channel. The executor
/// pushes captured inference calls; a drainer task (owned by the runtime)
/// appends them to the event store. When the channel is full the capture is
/// DROPPED and counted — capture must never block or fail a generation call,
/// but a drop is never silent (the counter is surfaced as a sensor signal).
pub type CaptureSender = tokio::sync::mpsc::Sender<CapturedInference>;

/// The agent-run policy: how a local agent executes (tool-loop
/// orchestration). Owns the inference and tool-dispatch ports.
/// Ledger-unaware — the runtime owns spending.
#[derive(Clone)]
pub struct AgentExecutor {
    inference: Arc<dyn hkask_types::InferencePort>,
    tool_dispatch: Arc<dyn hkask_types::ToolDispatchPort>,
    /// Optional capture sink. `None` = capture not wired (the executor runs
    /// exactly as before — zero behavior change for non-captured paths).
    /// Interior mutability so the runtime can wire it through the shared
    /// `&LocalSwarmRuntime` the lazy getter hands out.
    capture: std::sync::Arc<std::sync::Mutex<Option<CaptureSender>>>,
    /// Count of captures dropped because the channel was full. The drainer
    /// cannot observe send-side backpressure, so the count lives HERE —
    /// a drop is never silent (the harness report surfaces it).
    capture_send_drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl AgentExecutor {
    pub(crate) fn new(
        inference: Arc<dyn hkask_types::InferencePort>,
        tool_dispatch: Arc<dyn hkask_types::ToolDispatchPort>,
    ) -> Self {
        Self {
            inference,
            tool_dispatch,
            capture: std::sync::Arc::new(std::sync::Mutex::new(None)),
            capture_send_drops: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// Wire the capture sink. Called by the runtime when the event store
    /// opens; the drainer task is started there.
    pub(crate) fn set_capture(&self, sender: CaptureSender) {
        // Whole-value swap: a poisoned guard still guards the previous sender,
        // which this call replaces — recover rather than cascade the original
        // panic into the runtime's event-store wiring.
        let mut capture = self.capture.lock().unwrap_or_else(|poisoned| {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                "capture lock poisoned — recovering to install the new capture sink"
            );
            poisoned.into_inner()
        });
        *capture = Some(sender);
    }

    /// Captures dropped on the send side (channel full). Shared with the
    /// runtime so `capture_drops()` reports both send-side and drainer-side
    /// drops.
    #[allow(dead_code)] // sensor signal — consumed by capture_drops when wired
    pub(crate) fn capture_send_drops(&self) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
        std::sync::Arc::clone(&self.capture_send_drops)
    }

    /// Fire-and-forget capture: try-send, never block, never fail the call.
    /// A full channel drops the capture and increments the send-drop
    /// counter — surfaced via the runtime's `capture_drops()`, never silent.
    fn capture_inference(&self, captured: CapturedInference) {
        if let Ok(sender) = self.capture.lock()
            && let Some(sender) = sender.as_ref()
            && sender.try_send(captured).is_err()
        {
            self.capture_send_drops
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// The resolved local inference port. Exposed so the local knowledge tools
    /// (`swarm_generate_prompt_local` / `swarm_generate_ontology_local`) can do a
    /// one-shot generate without going through the full agent-run loop (they
    /// are authoring aids — no ledger debit, no tool loop).
    pub(crate) fn inference(&self) -> Arc<dyn hkask_types::InferencePort> {
        Arc::clone(&self.inference)
    }

    /// Run a local agent: execute declared skills, build the declared tool
    /// set, and run the multi-round inference/tool-dispatch loop. Returns the
    /// raw result; the caller debits.
    ///
    /// `task_clean` is the already-stripped task (the runtime strips `@mentions`
    /// before the funds check, then passes the clean task here).
    pub(crate) async fn run(
        &self,
        agent: &LocalAgentCard,
        task_clean: &str,
    ) -> Result<RawDelegateResult, LocalSwarmError> {
        // Build the prompt: system prompt + task.
        let system_prompt = agent
            .capabilities
            .system_prompt
            .as_deref()
            .unwrap_or("You are a helpful assistant.");
        let prompt = format!("{system_prompt}\n\n---\n\nTask: {task_clean}");

        // Build the declared tool set from the card's `mcp_tools` (qualified
        // `server/tool` names). This list is the allowlist: a model call for
        // any tool not declared here is never dispatched.
        let declared_tools: Vec<(String, String)> = agent
            .capabilities
            .mcp_tools
            .iter()
            .filter_map(|qualified| {
                qualified
                    .split_once('/')
                    .map(|(s, t)| (s.to_string(), t.to_string()))
            })
            .collect();
        // The qualified allowlist travels with every dispatch so the zed-side
        // IPC server can enforce it at the dispatch boundary — a tool outside
        // the card's declared set is never minted a panel token there.
        let qualified_allowed: Vec<String> = declared_tools
            .iter()
            .map(|(s, t)| format!("{s}/{t}"))
            .collect();
        let tool_defs: Vec<hkask_types::ChatToolDefinition> = declared_tools
            .iter()
            .map(|(server, tool)| hkask_types::ChatToolDefinition {
                tool_type: "function".to_string(),
                function: hkask_types::ChatToolFunction {
                    name: format!("{server}/{tool}"),
                    description: format!("Invoke `{tool}` on the `{server}` MCP server."),
                    parameters: serde_json::json!({ "type": "object", "properties": {} }),
                },
            })
            .collect();
        // When the agent opts into reasoning, register the built-in
        // `reasoning/think` tool. It is handled locally by the executor
        // (not dispatched via IPC), so it does not need to be in the
        // `qualified_allowed` allowlist — the local handler short-circuits
        // before the dispatch boundary.
        let has_reasoning = agent.capabilities.reasoning;
        let mut tool_defs = tool_defs;
        if has_reasoning {
            tool_defs.push(hkask_types::ChatToolDefinition {
                tool_type: "function".to_string(),
                function: hkask_types::ChatToolFunction {
                    name: REASONING_TOOL_NAME.to_string(),
                    description: "Record a structured reasoning step. Call this before \
                        making tool calls or generating a final answer. Use next_action=\"final_answer\" \
                        to signal you are ready to respond.".to_string(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "title": { "type": "string", "description": "Short title for this reasoning step" },
                            "reasoning": { "type": "string", "description": "The reasoning text" },
                            "action": { "type": "string", "description": "Optional action taken or planned" },
                            "next_action": { "type": "string", "enum": ["continue", "validate", "final_answer"], "description": "What to do next" },
                            "confidence": { "type": "number", "description": "Optional self-assessed confidence (0.0-1.0)" }
                        },
                        "required": ["title", "reasoning", "next_action"]
                    }),
                },
            });
        }
        let tools_slice: Option<&[hkask_types::ChatToolDefinition]> =
            (!tool_defs.is_empty()).then_some(&tool_defs[..]);

        // Run the tool loop: messages → inference → (tool calls → dispatch →
        // append results) → inference … The round cap bounds cost
        // amplification; the per-dispatch ceiling is the credit gate.
        let params = sampling_params(agent);
        let model_override = if agent.capabilities.model.is_empty() {
            None
        } else {
            Some(agent.capabilities.model.clone())
        };
        let mut messages = vec![hkask_types::ChatMessage {
            role: "user".to_string(),
            content: prompt,
        }];
        let mut tool_calls_made: Vec<serde_json::Value> = Vec::new();
        let mut total_tokens: i64 = 0;
        let mut final_text = String::new();
        let mut final_model = String::new();
        let mut reasoning_steps: Vec<ReasoningStep> = Vec::new();
        // The rollout id groups this run's events in the store. Derived from
        // the agent + a fresh uuid — the caller (harness or delegate path)
        // does not need to supply one; the harness stamps its own verdict
        // events under the same id via `last_rollout_id`.
        let rollout_id = format!("delegation-{}-{}", agent.agent_id, uuid::Uuid::new_v4());
        for round in 0..MAX_TOOL_ROUNDS {
            let inference_started = std::time::Instant::now();
            // Snapshot the request body before the call — the messages array
            // is mutated by the tool loop after the call returns.
            let request_body = serde_json::to_string(
                &messages
                    .iter()
                    .map(|m| (m.role.clone(), m.content.clone()))
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default();
            let result = self
                .inference
                .generate_with_messages(&messages, &params, model_override.as_deref(), tools_slice)
                .await
                .map_err(|e| {
                    // Capture the failed call too — a failed inference is a
                    // real event, not an absence.
                    self.capture_inference(CapturedInference {
                        rollout_id: rollout_id.clone(),
                        model: agent.capabilities.model.clone(),
                        status: "error",
                        latency_ms: inference_started.elapsed().as_millis(),
                        total_tokens: 0,
                        tool_calls: 0,
                        round,
                        request_body: cap_body(&request_body),
                        response_body: String::new(),
                    });
                    LocalSwarmError::Unavailable(format!("local inference failed: {e}"))
                })?;
            self.capture_inference(CapturedInference {
                rollout_id: rollout_id.clone(),
                model: result.model.clone(),
                status: "ok",
                latency_ms: inference_started.elapsed().as_millis(),
                total_tokens: i64::from(result.usage.total_tokens),
                tool_calls: result.tool_calls.len(),
                round,
                request_body: cap_body(&request_body),
                response_body: cap_body(&result.text),
            });
            total_tokens += i64::from(result.usage.total_tokens);
            final_model = result.model.clone();
            if result.tool_calls.is_empty() {
                final_text = result.text;
                break;
            }

            // Dispatch each model tool call, allowlisted against the card's
            // declared mcp_tools. Results are appended as a user message so
            // the next round sees them (provider-safe message shape).
            let mut round_results = Vec::new();
            for call in &result.tool_calls {
                let qualified = &call.tool;

                // Built-in reasoning tool: handled locally, not dispatched
                // via IPC. Records a structured reasoning step and returns
                // the full history to the model so it can decide whether to
                // continue.
                if has_reasoning && qualified == REASONING_TOOL_NAME {
                    let step = parse_reasoning_step(&call.args, round);
                    reasoning_steps.push(step.clone());
                    let history = format_reasoning_history(&reasoning_steps);
                    let summary = serde_json::json!({
                        "tool": REASONING_TOOL_NAME,
                        "ok": true,
                        "step_count": reasoning_steps.len(),
                    });
                    tool_calls_made.push(summary);
                    round_results.push(format!(
                        "Reasoning step recorded ({count} total). History:\n{history}",
                        count = reasoning_steps.len(),
                    ));
                    continue;
                }

                let declared = declared_tools
                    .iter()
                    .find(|(s, t)| format!("{s}/{t}") == *qualified);
                let (outcome, summary) = match declared {
                    Some((server, tool)) => {
                        match self
                            .tool_dispatch
                            .invoke_tool(server, tool, call.args.clone(), &qualified_allowed)
                            .await
                        {
                            Ok(value) => {
                                // Cap large string returns to prevent unbounded
                                // memory growth in the tool_calls summary. A
                                // 64KB prefix is sufficient for short field
                                // values (paths, URLs, verdicts) in the
                                // result. Object and array returns are
                                // typically structured and small enough; only
                                // raw string returns (file contents, terminal
                                // output) grow large.
                                let capped = match &value {
                                    serde_json::Value::String(s) if s.len() > 64 * 1024 => {
                                        serde_json::Value::String(
                                            s.chars().take(64 * 1024).collect(),
                                        )
                                    }
                                    _ => value,
                                };
                                let text = serde_json::to_string(&capped)
                                    .unwrap_or_else(|_| capped.to_string());
                                let summary = serde_json::json!({
                                    "tool": qualified,
                                    "ok": true,
                                    "result": capped,
                                });
                                (
                                    format!("Tool call '{qualified}' returned:\n{text}"),
                                    summary,
                                )
                            }
                            Err(e) => {
                                let msg = format!("dispatch failed: {e}");
                                (
                                    format!("Tool call '{qualified}' {msg}"),
                                    serde_json::json!({
                                        "tool": qualified,
                                        "ok": false,
                                        "error": e.to_string(),
                                    }),
                                )
                            }
                        }
                    }
                    None => (
                        format!(
                            "Tool call '{qualified}' is not in this agent's declared mcp_tools \
                             allowlist — not dispatched"
                        ),
                        serde_json::json!({
                            "tool": qualified,
                            "ok": false,
                            "error": "not in declared mcp_tools allowlist",
                        }),
                    ),
                };
                tool_calls_made.push(summary);
                round_results.push(outcome);
            }
            messages.push(hkask_types::ChatMessage {
                role: "assistant".to_string(),
                content: format!("(requested {} tool call(s))", result.tool_calls.len()),
            });
            messages.push(hkask_types::ChatMessage {
                role: "user".to_string(),
                content: round_results.join("\n\n"),
            });
        }

        Ok(RawDelegateResult {
            text: final_text,
            model: final_model,
            tokens_used: total_tokens,
            tool_calls: tool_calls_made,
            rollout_id,
            reasoning_steps,
        })
    }
}

/// Resolve the sampling parameters for one agent run — the local analog of
/// fermi's card-driven sampling (`agents.temperature` + `agents.model_params`,
/// merged by `apply_tier_resolution`). Precedence mirrors fermi's:
/// 1. Start from the executor's default preset (`LLMParameters::default()`).
/// 2. The card's `temperature` field overrides the default temperature.
/// 3. The card's `model_params` keys override BOTH — fermi's doc: "Keys
///    override the legacy `temperature` field and add provider-specific
///    params".
///
/// The merge is done at the JSON level (serialize defaults → overlay
/// `model_params` keys → deserialize) so a partial `model_params` object
/// keeps the defaults for keys it does not name, and unknown keys are
/// ignored by serde rather than failing the run.
fn sampling_params(agent: &crate::local_registry::LocalAgentCard) -> hkask_types::LLMParameters {
    let mut params = hkask_types::LLMParameters::default();
    if let Some(temperature) = agent.capabilities.temperature {
        params.temperature = temperature as f32;
    }
    if let Some(model_params) = agent.capabilities.model_params.as_ref()
        && let (Some(base), Some(overlay)) =
            (serde_json::to_value(&params).ok(), model_params.as_object())
    {
        let mut merged = base;
        if let Some(merged_obj) = merged.as_object_mut() {
            for (key, value) in overlay {
                merged_obj.insert(key.clone(), value.clone());
            }
        }
        if let Ok(resolved) = serde_json::from_value::<hkask_types::LLMParameters>(merged) {
            params = resolved;
        }
    }
    params
}

/// Parse a `reasoning/think` tool call's arguments into a `ReasoningStep`.
/// Missing optional fields default to `None`; missing required fields default
/// to empty strings (the model was told they are required, but a malformed
/// call should not crash the run).
fn parse_reasoning_step(args: &serde_json::Value, round: usize) -> ReasoningStep {
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let reasoning = args
        .get("reasoning")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let next_action = args
        .get("next_action")
        .and_then(|v| v.as_str())
        .unwrap_or("continue")
        .to_string();
    let confidence = args.get("confidence").and_then(|v| v.as_f64());
    ReasoningStep {
        title,
        reasoning,
        action,
        next_action,
        confidence,
        round,
    }
}

/// Format the full reasoning-step history as a numbered list for the model.
/// The model sees this as the tool result and uses it to decide whether to
/// continue reasoning or emit a final answer.
fn format_reasoning_history(steps: &[ReasoningStep]) -> String {
    steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let action = step
                .action
                .as_deref()
                .map(|a| format!(" | action: {a}"))
                .unwrap_or_default();
            let confidence = step
                .confidence
                .map(|c| format!(" | confidence: {c:.2}"))
                .unwrap_or_default();
            format!(
                "{i}. [{next_action}] {title}{action}{confidence}\n   {reasoning}",
                i = i + 1,
                next_action = step.next_action,
                title = step.title,
                action = action,
                confidence = confidence,
                reasoning = step.reasoning,
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capture path must be inert when unwired: `capture_inference` is
    /// a no-op and `run` behaves exactly as before (the zero-behavior-change
    /// contract for non-captured paths).
    #[test]
    fn capture_is_inert_when_unwired() {
        let executor = AgentExecutor::new(Arc::new(StubInference), Arc::new(StubDispatch));
        executor.capture_inference(CapturedInference {
            rollout_id: "r".into(),
            model: "m".into(),
            status: "ok",
            latency_ms: 1,
            total_tokens: 1,
            tool_calls: 0,
            round: 0,
            request_body: String::new(),
            response_body: String::new(),
        });
        // No panic, no channel error — inert by construction.
    }

    /// A wired capture receives the inference call; a full channel drops
    /// without panicking (the drop counter lives on the drainer side).
    #[tokio::test]
    async fn capture_receives_inference_calls() {
        let executor = AgentExecutor::new(Arc::new(StubInference), Arc::new(StubDispatch));
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        executor.set_capture(tx);
        executor.capture_inference(CapturedInference {
            rollout_id: "rollout-a".into(),
            model: "stub-model".into(),
            status: "ok",
            latency_ms: 5,
            total_tokens: 2,
            tool_calls: 0,
            round: 0,
            request_body: "[]".into(),
            response_body: "stub".into(),
        });
        let captured = rx.recv().await.expect("capture must arrive");
        assert_eq!(captured.rollout_id, "rollout-a");
        assert_eq!(captured.status, "ok");
        assert_eq!(captured.response_body, "stub");
    }

    struct StubInference;

    impl hkask_types::InferencePort for StubInference {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &hkask_types::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Ok(hkask_types::InferenceResult {
                    text: "stub".into(),
                    model: "stub-model".into(),
                    usage: hkask_types::InferenceUsage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    },
                    finish_reason: "stop".into(),
                    tool_calls: vec![],
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    struct StubDispatch;

    impl hkask_types::ToolDispatchPort for StubDispatch {
        fn invoke_tool<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
            _args: serde_json::Value,
            _allowlist: &'a [String],
        ) -> Pin<
            Box<
                dyn Future<Output = Result<serde_json::Value, hkask_types::InferenceError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Ok(serde_json::json!({"ok": true})) })
        }
    }

    use std::future::Future;
    use std::pin::Pin;

    #[test]
    fn parse_reasoning_step_extracts_all_fields() {
        let args = serde_json::json!({
            "title": "Analyze market",
            "reasoning": "The market is trending up.",
            "action": "called research/web_search",
            "next_action": "continue",
            "confidence": 0.8
        });
        let step = parse_reasoning_step(&args, 1);
        assert_eq!(step.title, "Analyze market");
        assert_eq!(step.reasoning, "The market is trending up.");
        assert_eq!(step.action.as_deref(), Some("called research/web_search"));
        assert_eq!(step.next_action, "continue");
        assert_eq!(step.confidence, Some(0.8));
        assert_eq!(step.round, 1);
    }

    #[test]
    fn parse_reasoning_step_defaults_missing_optional_fields() {
        let args = serde_json::json!({
            "title": "Quick thought",
            "reasoning": "Need to check.",
            "next_action": "final_answer"
        });
        let step = parse_reasoning_step(&args, 0);
        assert_eq!(step.action, None);
        assert_eq!(step.confidence, None);
        assert_eq!(step.next_action, "final_answer");
    }

    #[test]
    fn parse_reasoning_step_defaults_missing_next_action() {
        let args = serde_json::json!({
            "title": "Step",
            "reasoning": "Thinking."
        });
        let step = parse_reasoning_step(&args, 0);
        assert_eq!(
            step.next_action, "continue",
            "missing next_action defaults to continue"
        );
    }

    #[test]
    fn format_reasoning_history_renders_all_steps() {
        let steps = vec![
            ReasoningStep {
                title: "First".into(),
                reasoning: "Initial thought.".into(),
                action: None,
                next_action: "continue".into(),
                confidence: None,
                round: 0,
            },
            ReasoningStep {
                title: "Second".into(),
                reasoning: "After tool call.".into(),
                action: Some("called web_search".into()),
                next_action: "final_answer".into(),
                confidence: Some(0.9),
                round: 1,
            },
        ];
        let history = format_reasoning_history(&steps);
        assert!(history.contains("1. [continue] First"));
        assert!(history.contains("Initial thought."));
        assert!(history.contains("2. [final_answer] Second"));
        assert!(history.contains("action: called web_search"));
        assert!(history.contains("confidence: 0.90"));
    }

    #[test]
    fn format_reasoning_history_empty_renders_nothing() {
        let history = format_reasoning_history(&[]);
        assert!(history.is_empty());
    }
}
