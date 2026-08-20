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
pub(crate) const MAX_TOOL_ROUNDS: usize = 4;

/// The raw result of running an agent — text, model, token usage, and the
/// tool execution summary. NOT debited. The caller
/// (`LocalSwarmRuntime::delegate`) debits the ledger.
pub(crate) struct RawDelegateResult {
    pub text: String,
    pub model: String,
    pub tokens_used: i64,
    pub tool_calls: Vec<serde_json::Value>,
}

/// The agent-run policy: how a local agent executes (tool-loop
/// orchestration). Owns the inference and tool-dispatch ports.
/// Ledger-unaware — the runtime owns spending.
pub(crate) struct AgentExecutor {
    inference: Arc<dyn hkask_types::InferencePort>,
    tool_dispatch: Arc<dyn hkask_types::ToolDispatchPort>,
}

impl AgentExecutor {
    pub(crate) fn new(
        inference: Arc<dyn hkask_types::InferencePort>,
        tool_dispatch: Arc<dyn hkask_types::ToolDispatchPort>,
    ) -> Self {
        Self {
            inference,
            tool_dispatch,
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
        let tools_slice: Option<&[hkask_types::ChatToolDefinition]> =
            (!tool_defs.is_empty()).then_some(&tool_defs[..]);

        // Run the tool loop: messages → inference → (tool calls → dispatch →
        // append results) → inference … The round cap bounds cost
        // amplification; the per-dispatch ceiling is the credit gate.
        let params = hkask_types::LLMParameters::default();
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
        for _round in 0..MAX_TOOL_ROUNDS {
            let result = self
                .inference
                .generate_with_messages(&messages, &params, model_override.as_deref(), tools_slice)
                .await
                .map_err(|e| {
                    LocalSwarmError::Unavailable(format!("local inference failed: {e}"))
                })?;
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
        })
    }
}
