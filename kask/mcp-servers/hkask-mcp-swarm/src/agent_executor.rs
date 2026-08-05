//! Agent executor — the local agent-run policy, extracted from
//! `LocalSwarmRuntime::delegate`.
//!
//! `AgentExecutor::run` runs a local agent: scans the system prompt, executes
//! the declared skills (guard-scanning each output), builds the declared
//! tool set, and runs the multi-round inference/tool-dispatch loop (guard-
//! scanning + redacting each tool result). It returns a `RawDelegateResult`
//! carrying the raw output text, model, token usage, and tool/skill
//! summaries.
//!
//! **The executor does NOT scan the final output or debit the ledger.** The
//! caller (`LocalSwarmRuntime::delegate`) is responsible for debit-then-scan:
//! it computes the cost, debits the ledger, and *then* calls
//! `AgentExecutor::scan_output` on the raw text. This ordering is load-bearing
//! — the "compute was spent" invariant requires the debit to happen before
//! the output guard scan so a guard-quarantined result still costs credits.
//! Moving `scan_output` into `run` would break that invariant (the scan would
//! precede the debit). See ADR: "AgentExecutor returns raw output;
//! LocalSwarmRuntime owns debit-then-scan".
//!
//! The executor also exposes `scan_input` so the runtime can scan the task
//! *before* the funds check (preserving the original ordering: reject
//! injected input before rejecting insufficient funds).

use std::sync::Arc;

use crate::error::SwarmError;
use crate::local_registry::LocalAgentCard;

/// Maximum tool-call rounds per delegation. Each round is a full inference
/// call; the cap bounds cost amplification (the per-dispatch credit ceiling
/// is the credit gate, this is the round gate).
pub(crate) const MAX_TOOL_ROUNDS: usize = 4;

/// Maximum declared skills executed per delegation. Each skill is a cascade
/// with its own gas budget on the zed side; the cap bounds context bloat and
/// cascade amplification from a maliciously-large `skills` list.
pub(crate) const MAX_SKILLS_PER_DELEGATION: usize = 3;

/// The raw result of running an agent — text, model, token usage, and the
/// tool/skill execution summaries. NOT output-scanned and NOT debited. The
/// caller (`LocalSwarmRuntime::delegate`) debits the ledger, then calls
/// `AgentExecutor::scan_output` on `text` to produce the final response.
pub(crate) struct RawDelegateResult {
    pub text: String,
    pub model: String,
    pub tokens_used: i64,
    pub tool_calls: Vec<serde_json::Value>,
    pub executed_skills: Vec<serde_json::Value>,
}

/// The agent-run policy: how a local agent executes (input scanning, skill
/// cascade, tool-loop orchestration). Owns the inference, tool-dispatch,
/// skill-exec, and guard ports. Ledger-unaware — the runtime owns spending.
pub(crate) struct AgentExecutor {
    inference: Arc<dyn hkask_types::InferencePort>,
    tool_dispatch: Arc<dyn hkask_types::ToolDispatchPort>,
    skill_exec: Arc<dyn hkask_types::SkillExecPort>,
    guard: Arc<hkask_guard::ContentGuard>,
}

impl AgentExecutor {
    pub(crate) fn new(
        inference: Arc<dyn hkask_types::InferencePort>,
        tool_dispatch: Arc<dyn hkask_types::ToolDispatchPort>,
        skill_exec: Arc<dyn hkask_types::SkillExecPort>,
        guard: hkask_guard::ContentGuard,
    ) -> Self {
        Self {
            inference,
            tool_dispatch,
            skill_exec,
            guard: Arc::new(guard),
        }
    }

    /// The resolved local inference port. Exposed so the local knowledge tools
    /// (`swarm_generate_prompt_local` / `swarm_generate_ontology_local`) can do a
    /// one-shot generate without going through the full agent-run loop (they
    /// are authoring aids — no ledger debit, no tool loop).
    pub(crate) fn inference(&self) -> Arc<dyn hkask_types::InferencePort> {
        Arc::clone(&self.inference)
    }

    /// The content guard. Exposed so the local knowledge tools can scan their
    /// LLM-generated output for canary/secret leakage before returning it.
    pub(crate) fn guard(&self) -> Arc<hkask_guard::ContentGuard> {
        Arc::clone(&self.guard)
    }

    /// The resolved skill-execution port. Exposed so `swarm_ai_assist` can run
    /// the on-disk `swarm-compose-guide` skill cascade (rendering the Jinja2
    /// guidance template) rather than building the prompt from hardcoded Rust
    /// strings — the template is the single source of truth for composition
    /// guidance.
    pub(crate) fn skill_exec(&self) -> Arc<dyn hkask_types::SkillExecPort> {
        Arc::clone(&self.skill_exec)
    }

    /// Test-only constructor with injected dependencies (mirrors the
    /// `StubInferencePort` pattern). Accepts a pre-built guard so tests can
    /// use `ContentGuard::mandatory(&default)`.
    #[cfg(test)]
    pub(crate) fn with_deps(
        inference: Arc<dyn hkask_types::InferencePort>,
        tool_dispatch: Arc<dyn hkask_types::ToolDispatchPort>,
        skill_exec: Arc<dyn hkask_types::SkillExecPort>,
        guard: hkask_guard::ContentGuard,
    ) -> Self {
        Self::new(inference, tool_dispatch, skill_exec, guard)
    }

    /// Scan input text through the content guard. Returns `Err` if the guard
    /// rejects the input (prompt injection, role override, etc.). Exposed so
    /// the runtime can scan the task *before* the funds check, preserving the
    /// original ordering (reject injected input before rejecting insufficient
    /// funds).
    pub(crate) fn scan_input(&self, text: &str) -> Result<(), SwarmError> {
        let result = self.guard.scan_input(text);
        if !result.passed {
            let violations: Vec<String> = result
                .violations
                .iter()
                .map(|v| format!("{}: {}", v.scanner, v.description))
                .collect();
            return Err(SwarmError::Unavailable(format!(
                "input guard rejected: {}",
                violations.join("; ")
            )));
        }
        Ok(())
    }

    /// Scan output text through the content guard. Returns the (possibly
    /// sanitized) output text, or `Err` if canary exfiltration is detected.
    ///
    /// Policy: canary exfiltration is a hard failure (the system prompt was
    /// leaked — OWASP LLM07), but secret leakage is sanitized and returned
    /// (the output may be legitimately useful despite a false-positive secret
    /// match). This asymmetry is intentional: canary = exfiltration = reject;
    /// secret = leakage = sanitize and return. Do not "fix" this by making
    /// both paths hard-fail — that would reject legitimate outputs that
    /// happen to match a secret scanner pattern.
    pub(crate) fn scan_output(&self, text: &str) -> Result<String, SwarmError> {
        let result = self.guard.scan_output(text);
        if self.guard.check_canary(text) {
            return Err(SwarmError::Unavailable(
                "canary token detected in output — system prompt exfiltration suspected"
                    .to_string(),
            ));
        }
        if !result.passed {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                violations = ?result.violations,
                "output guard violations — sanitizing"
            );
        }
        Ok(result.output.content(text).to_string())
    }

    /// Run a local agent: scan the system prompt, execute declared skills
    /// (guard-scanning each output), build the declared tool set, and run
    /// the multi-round inference/tool-dispatch loop (guard-scanning +
    /// redacting each tool result). Returns the raw result; the caller debits
    /// and scans the output.
    ///
    /// `task_clean` is the already-stripped, already-input-scanned task (the
    /// runtime strips `@mentions` and calls `scan_input` before the funds
    /// check, then passes the clean task here). This method scans the system
    /// prompt and every skill/tool output, but NOT the task (pre-scanned) and
    /// NOT the final output (the runtime scans it after debit).
    pub(crate) async fn run(
        &self,
        agent: &LocalAgentCard,
        task_clean: &str,
    ) -> Result<RawDelegateResult, SwarmError> {
        // Build the prompt: system prompt + task.
        let system_prompt = agent
            .capabilities
            .system_prompt
            .as_deref()
            .unwrap_or("You are a helpful assistant.");

        // Guard-scan the system_prompt before injecting it into the prompt.
        // The task was already scanned by the caller, and each skill output is
        // scanned below — but the system_prompt was not. For locally-authored
        // cards the operator controls it; for cloned cards
        // (`swarm_clone_to_local`) it is third-party ABW data that could carry
        // prompt injection. The clone path strips obvious patterns via
        // `sanitize_abw_text`, but the guard is the hard gate: a system_prompt
        // that trips the input guard IS fatal. The `.rules` trap: the input
        // guard is the advertised enforcement point for the delegate path — it
        // must scan all untrusted text that reaches the model, not just the
        // task.
        self.scan_input(system_prompt)?;

        // Run the declared skills (capped) against the task BEFORE the LLM
        // call. Each cascade runs on the zed side (`ManifestExecutor`, own
        // gas/OCAP enforcement). Skill output is untrusted context — it flows
        // into the prompt, so it is guard-scanned before injection; a skill
        // output that trips the input guard IS fatal (an injection from a
        // skill is a finding, not a cosmetic issue). A missing skill or
        // cascade failure is recorded, not fatal — the delegation proceeds
        // with whatever context the successful skills produced.
        let mut executed_skills: Vec<serde_json::Value> = Vec::new();
        let mut skill_context = String::new();
        for skill in agent
            .capabilities
            .skills
            .iter()
            .take(MAX_SKILLS_PER_DELEGATION)
        {
            match self.skill_exec.execute_skill(skill, task_clean).await {
                Ok(output) => {
                    self.scan_input(&output)?;
                    executed_skills.push(serde_json::json!({ "skill": skill, "ok": true }));
                    skill_context.push_str(&format!("\n\n## Skill '{skill}' output\n{output}"));
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.mcp.swarm",
                        skill,
                        error = %e,
                        "declared skill failed — delegation proceeds without it"
                    );
                    executed_skills.push(serde_json::json!({
                        "skill": skill,
                        "ok": false,
                        "error": e.to_string(),
                    }));
                }
            }
        }
        let prompt = format!("{system_prompt}{skill_context}\n\n---\n\nTask: {task_clean}");

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
                .map_err(|e| SwarmError::UpstreamModelError {
                    provider: "local".to_string(),
                    message: format!("inference failed: {e}"),
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
                                let text = serde_json::to_string(&value)
                                    .unwrap_or_else(|_| value.to_string());
                                // Redact-and-continue: a tool result that trips
                                // the input guard is quarantined from the model
                                // context, but the delegation proceeds — tool
                                // output is data, and a false positive must not
                                // abort the run.
                                let (injected, ok, error) = match self.scan_input(&text) {
                                    Ok(()) => (text, true, None),
                                    Err(e) => (
                                        "[redacted: tool output tripped the input guard — not injected]".to_string(),
                                        false,
                                        Some(e.to_string()),
                                    ),
                                };
                                let mut summary =
                                    serde_json::json!({ "tool": qualified, "ok": ok });
                                if let Some(err) = error {
                                    summary["error"] = serde_json::Value::String(err);
                                }
                                (
                                    format!("Tool call '{qualified}' returned:\n{injected}"),
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
            executed_skills,
        })
    }
}
