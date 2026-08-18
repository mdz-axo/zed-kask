//! Agent executor — the local agent-run policy, extracted from
//! `LocalSwarmRuntime::delegate`.
//!
//! `AgentExecutor::run` runs a local agent: executes the declared skills,
//! builds the declared tool set, and runs the multi-round inference/tool-
//! dispatch loop. It returns a `RawDelegateResult` carrying the raw output
//! text, model, token usage, and tool/skill summaries.
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

/// Maximum declared skills executed per delegation. Each skill is a cascade
/// with its own gas budget on the zed side; the cap bounds context bloat and
/// cascade amplification from a maliciously-large `skills` list.
pub(crate) const MAX_SKILLS_PER_DELEGATION: usize = 3;

/// The raw result of running an agent — text, model, token usage, and the
/// tool/skill execution summaries. NOT debited. The caller
/// (`LocalSwarmRuntime::delegate`) debits the ledger.
pub(crate) struct RawDelegateResult {
    pub text: String,
    pub model: String,
    pub tokens_used: i64,
    pub tool_calls: Vec<serde_json::Value>,
    pub executed_skills: Vec<serde_json::Value>,
}

/// The agent-run policy: how a local agent executes (skill cascade,
/// tool-loop orchestration). Owns the inference, tool-dispatch, and skill-exec
/// ports. Ledger-unaware — the runtime owns spending.
pub(crate) struct AgentExecutor {
    inference: Arc<dyn hkask_types::InferencePort>,
    tool_dispatch: Arc<dyn hkask_types::ToolDispatchPort>,
    skill_exec: Arc<dyn hkask_types::SkillExecPort>,
    /// Directory containing the zed-kask skill corpus (`.agents/skills/`),
    /// used to inject skill descriptions into the local agent's system prompt
    /// (Slice 6 — local agent skill-awareness). `None` = skill-awareness
    /// disabled (the agent runs skill-blind).
    skills_dir: Option<std::path::PathBuf>,
}

impl AgentExecutor {
    pub(crate) fn new(
        inference: Arc<dyn hkask_types::InferencePort>,
        tool_dispatch: Arc<dyn hkask_types::ToolDispatchPort>,
        skill_exec: Arc<dyn hkask_types::SkillExecPort>,
        skills_dir: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            inference,
            tool_dispatch,
            skill_exec,
            skills_dir,
        }
    }

    /// The resolved local inference port. Exposed so the local knowledge tools
    /// (`swarm_generate_prompt_local` / `swarm_generate_ontology_local`) can do a
    /// one-shot generate without going through the full agent-run loop (they
    /// are authoring aids — no ledger debit, no tool loop).
    pub(crate) fn inference(&self) -> Arc<dyn hkask_types::InferencePort> {
        Arc::clone(&self.inference)
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
    /// `StubInferencePort` pattern).
    #[cfg(test)]
    pub(crate) fn with_deps(
        inference: Arc<dyn hkask_types::InferencePort>,
        tool_dispatch: Arc<dyn hkask_types::ToolDispatchPort>,
        skill_exec: Arc<dyn hkask_types::SkillExecPort>,
    ) -> Self {
        Self::new(inference, tool_dispatch, skill_exec, None)
    }

    /// Build a skill catalog block for the agent's declared skills, reading
    /// the `name` and `description` fields from each skill's `SKILL.md`
    /// frontmatter. Returns `None` when `skills_dir` is unset or no declared
    /// skill has a readable `SKILL.md` — the agent runs skill-blind (the
    /// pre-Slice-6 behavior). The catalog is injected into the system prompt
    /// so the agent understands what skills were run for it and why, but the
    /// card's `skills` list remains the execution allowlist (no runtime
    /// discovery — the executor pre-runs the declared skills, the model
    /// cannot invoke new ones).
    ///
    /// The frontmatter is parsed with a minimal YAML extractor (the `name`
    /// and `description` fields only) — the swarm server does not depend on
    /// the zed-side `agent_skills` crate (which is GPUI-bound). A missing or
    /// malformed `SKILL.md` is logged and skipped — the catalog is best-effort,
    /// not a gate.
    fn build_skill_catalog(&self, declared_skills: &[String]) -> Option<String> {
        let skills_dir = self.skills_dir.as_ref()?;
        let mut entries = Vec::new();
        for skill_id in declared_skills {
            // Validate the skill id before joining it into a path — a
            // malicious cloned ABW card could declare `skills:
            // ["../../../etc/passwd"]` to read arbitrary files via path
            // traversal. Skill ids must be lowercase letters, numbers, and
            // hyphens only (mirrors `agent_skills::validate_name`, which the
            // swarm server can't depend on — it's GPUI-bound). Reject any id
            // containing path separators or `..`.
            if !is_valid_skill_id(skill_id) {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    skill = skill_id.as_str(),
                    "invalid skill id (path traversal or invalid chars) — skipped from catalog"
                );
                continue;
            }
            let skill_md = skills_dir.join(skill_id).join("SKILL.md");
            match std::fs::read_to_string(&skill_md) {
                Ok(content) => {
                    if let Some((name, description)) = parse_skill_frontmatter(&content) {
                        entries.push(format!(
                            "  <skill>\n    <name>{name}</name>\n    <description>{description}</description>\n  </skill>"
                        ));
                    } else {
                        tracing::warn!(
                            target: "hkask.mcp.swarm",
                            skill = skill_id.as_str(),
                            path = %skill_md.display(),
                            "SKILL.md frontmatter missing name/description — skipped from catalog"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.mcp.swarm",
                        skill = skill_id.as_str(),
                        error = %e,
                        path = %skill_md.display(),
                        "could not read SKILL.md — skipped from catalog"
                    );
                }
            }
        }
        if entries.is_empty() {
            return None;
        }
        Some(format!(
            "\n<declared_skills>\n  The following skills were pre-executed for this task. \
             Their cascade outputs appear as context below. You cannot invoke \
             additional skills at runtime — the card's `skills` list is the \
             execution allowlist.\n{}\n</declared_skills>",
            entries.join("\n")
        ))
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
        // Inject the skill catalog (name + description for declared skills)
        // into the system prompt when `skills_dir` is configured, so the
        // agent understands what skills were pre-run for it and why. The
        // card's `skills` list remains the execution allowlist — the catalog
        // is awareness, not discovery.
        let base_system_prompt = agent
            .capabilities
            .system_prompt
            .as_deref()
            .unwrap_or("You are a helpful assistant.");
        let skill_catalog = self.build_skill_catalog(&agent.capabilities.skills);
        let system_prompt = match &skill_catalog {
            Some(catalog) => format!("{base_system_prompt}{catalog}"),
            None => base_system_prompt.to_string(),
        };

        // Run the declared skills (capped) against the task BEFORE the LLM
        // call. Each cascade runs on the zed side (`ManifestExecutor`, own
        // gas/OCAP enforcement). A missing skill or cascade failure is
        // recorded, not fatal — the delegation proceeds with whatever
        // context the successful skills produced.
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
                                // memory growth in the tool_calls summary. The
                                // grounding check only needs to find short
                                // field values (paths, URLs, verdicts) in the
                                // result — a 64KB prefix is sufficient. Object
                                // and array returns are typically structured
                                // and small enough; only raw string returns
                                // (file contents, terminal output) grow large.
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
            executed_skills,
        })
    }
}

/// Parse the `name` and `description` fields from a `SKILL.md` frontmatter
/// block (the YAML between the opening and closing `---` lines). Returns
/// `None` if either field is missing or the frontmatter is malformed.
///
/// This is a minimal extractor — the swarm server does not depend on the
/// zed-side `agent_skills` crate (which is GPUI-bound) or a full YAML parser.
/// The `name` field is a plain scalar; the `description` field may be a
/// quoted scalar, a folded scalar (`>`), or a literal scalar (`|`).
/// Validate a skill id for path safety. Skill ids must be non-empty,
/// lowercase letters, numbers, and hyphens only — no path separators, no
/// `..`, no leading/trailing hyphens. Mirrors `agent_skills::validate_name`
/// (which the swarm server can't depend on — it's GPUI-bound). This is the
/// path-traversal gate for `build_skill_catalog`: a malicious cloned ABW card
/// could declare `skills: ["../../../etc/passwd"]` to read arbitrary files.
fn is_valid_skill_id(id: &str) -> bool {
    if id.is_empty() || id.starts_with('-') || id.ends_with('-') {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn parse_skill_frontmatter(content: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        return None;
    }
    // Find the closing `---`.
    let end = lines.iter().skip(1).position(|l| l.trim() == "---")? + 1;
    let frontmatter = &lines[1..end];
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut i = 0;
    while i < frontmatter.len() {
        let line = frontmatter[i];
        if let Some(rest) = line.strip_prefix("name:") {
            name = Some(rest.trim().trim_matches('"').to_string());
        } else if let Some(rest) = line.strip_prefix("description:") {
            let trimmed = rest.trim();
            if trimmed.starts_with('"') {
                // Quoted scalar — may span multiple lines (folded quotes).
                let mut buf = trimmed.trim_start_matches('"').to_string();
                while !buf.contains('"') && i + 1 < frontmatter.len() {
                    i += 1;
                    buf.push('\n');
                    buf.push_str(frontmatter[i].trim());
                }
                // Remove the closing quote.
                description = Some(buf.trim_end_matches('"').trim().to_string());
            } else if trimmed == ">" || trimmed == "|" {
                // Folded/literal scalar — collect indented continuation lines.
                let mut buf = String::new();
                i += 1;
                while i < frontmatter.len() && frontmatter[i].starts_with(' ') {
                    if !buf.is_empty() {
                        buf.push(' ');
                    }
                    buf.push_str(frontmatter[i].trim());
                    i += 1;
                }
                description = Some(buf.trim().to_string());
                continue;
            } else if !trimmed.is_empty() {
                // Plain scalar.
                description = Some(trimmed.to_string());
            }
        }
        i += 1;
    }
    match (name, description) {
        (Some(n), Some(d)) if !n.is_empty() && !d.is_empty() => Some((n, d)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_quoted_description() {
        let content = "---\nname: grill-me\nvisibility: public\ndescription: \"Socratic interrogation skill.\"\n---\n\n# Body";
        let (name, desc) = parse_skill_frontmatter(content).expect("should parse");
        assert_eq!(name, "grill-me");
        assert_eq!(desc, "Socratic interrogation skill.");
    }

    #[test]
    fn parse_frontmatter_folded_description() {
        let content = "---\nname: grill-me\ndescription: >\n  Socratic interrogation skill.\n  Tests deep understanding.\n---\n\n# Body";
        let (name, desc) = parse_skill_frontmatter(content).expect("should parse");
        assert_eq!(name, "grill-me");
        assert!(desc.contains("Socratic interrogation skill."));
        assert!(desc.contains("Tests deep understanding."));
    }

    #[test]
    fn parse_frontmatter_missing_fields() {
        let content = "---\nvisibility: public\n---\n\n# Body";
        assert!(parse_skill_frontmatter(content).is_none());
    }

    #[test]
    fn parse_frontmatter_no_frontmatter() {
        let content = "# Grill Me\n\nNo frontmatter here.";
        assert!(parse_skill_frontmatter(content).is_none());
    }

    #[test]
    fn is_valid_skill_id_rejects_path_traversal() {
        // B1 fix: a malicious cloned ABW card could declare these skill ids
        // to read arbitrary files via path traversal. All must be rejected.
        assert!(!is_valid_skill_id("../../../etc/passwd"));
        assert!(!is_valid_skill_id("../etc/passwd"));
        assert!(!is_valid_skill_id(".."));
        assert!(!is_valid_skill_id("a/b"));
        assert!(!is_valid_skill_id("a\\b"));
        assert!(!is_valid_skill_id(""));
        assert!(!is_valid_skill_id("-leading"));
        assert!(!is_valid_skill_id("trailing-"));
        assert!(!is_valid_skill_id("UPPERCASE"));
        assert!(!is_valid_skill_id("has space"));
        assert!(!is_valid_skill_id("has.special"));
    }

    #[test]
    fn is_valid_skill_id_accepts_valid_names() {
        assert!(is_valid_skill_id("grill-me"));
        assert!(is_valid_skill_id("bug-hunt"));
        assert!(is_valid_skill_id("metacognition"));
        assert!(is_valid_skill_id("kata-improvement"));
        assert!(is_valid_skill_id("adversarial-red-team"));
        assert!(is_valid_skill_id("a1"));
        assert!(is_valid_skill_id("1"));
    }
}
