//! Content sanitization + declared-capability filtering.
//!
//! Extracted from the swarm server root. `sanitize_agent_id` prevents path
//! traversal; `strip_leading_mentions` strips cross-mention injection from
//! delegate tasks; `filter_mcp_tools`/`filter_declared_skills` keep an agent
//! card's declared capabilities within the operator's governed set.
//!
//! This is defense-in-depth, not a complete injection defense — the agent's
//! system prompt must also treat tool output as untrusted data.

/// Sanitize an agent id for filesystem use. Only allows alphanumerics,
/// dash, underscore, and dot — strips everything else. Returns `None` if
/// the result is empty or only dots (which would be `.` or `..`, a path
/// traversal).
pub fn sanitize_agent_id(id: &str) -> Option<String> {
    let sanitized: String = id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect();
    // Reject empty or path-traversal-only results.
    if sanitized.is_empty() || sanitized.chars().all(|c| c == '.') {
        None
    } else {
        Some(sanitized)
    }
}

/// Strip leading @mentions from a delegate task (KA-06): a task starting
/// with `@other_agent` would address a different agent than the delegation
/// target, a semantic injection at the dispatch layer. This is
/// defense-in-depth against accidental cross-mention. Strips all leading
/// `@` tokens (and intervening whitespace) so `@a @b do x` becomes `do x`.
pub fn strip_leading_mentions(task: &str) -> String {
    let mut remaining = task.trim_start();
    while remaining.starts_with('@') {
        // Skip the @ and the following token (up to whitespace).
        let after_at = &remaining[1..];
        match after_at.find(char::is_whitespace) {
            Some(end) => {
                remaining = after_at[end..].trim_start();
            }
            None => {
                // The entire task is `@token` with no trailing content.
                return String::new();
            }
        }
    }
    remaining.to_string()
}

/// One declared `mcp_tools` entry dropped at create/reconfigure, with the
/// reason. Returned in the create/reconfigure result and persisted on the
/// card so `swarm_get_local_agent` and every delegation result can report
/// it — a silent drop is indistinguishable from "nothing to drop"
/// (the instrument-avoidance trap, observed 2026-10-23).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct DroppedTool {
    pub tool: String,
    pub reason: String,
}

/// Validate a card's declared `mcp_tools`. Each entry must be `server/tool`
/// with charset-safe, non-empty segments. When `allowed_servers` is set (the
/// governed server set from `HKASK_MCP_SERVER_IDS`), entries whose server is
/// not in it are dropped — a card must not extend the delegated tool surface
/// beyond the operator's own governed servers. Every dropped entry is
/// returned in the drop report (and logged) so the caller sees what was
/// filtered and why — the `.rules` startup-failure-signal trap: a silent drop
/// is indistinguishable from "nothing to drop".
pub fn filter_mcp_tools(
    tools: Vec<String>,
    allowed_servers: Option<&[String]>,
) -> (Vec<String>, Vec<DroppedTool>) {
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for qualified in tools {
        let Some((server, tool)) = qualified.split_once('/') else {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                tool = %qualified,
                "card tool dropped: not server/tool shaped"
            );
            dropped.push(DroppedTool {
                tool: qualified.clone(),
                reason: "not server/tool shaped (expected server/tool)".to_string(),
            });
            continue;
        };
        let server_ok = !server.is_empty()
            && server
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        let tool_ok = !tool.is_empty()
            && tool
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        if !server_ok || !tool_ok {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                tool = %qualified,
                "card tool dropped: invalid characters"
            );
            dropped.push(DroppedTool {
                tool: qualified.clone(),
                reason: "invalid characters in server or tool segment".to_string(),
            });
            continue;
        }
        if let Some(allowed) = allowed_servers
            && !allowed.iter().any(|s| s == server)
        {
            // The host quartet is granted through the skills-declaration
            // path (any declared skill auto-adds it at the executor), never
            // through mcp_tools — say so instead of the generic governed-set
            // reason, which would point the caller at the wrong fix.
            let reason = if server == "host" {
                "host/* tools are granted via the card's skills declaration, not mcp_tools — declare a skill to receive the host quartet".to_string()
            } else {
                "server not in the governed set (HKASK_MCP_SERVER_IDS)".to_string()
            };
            tracing::warn!(
                target: "hkask.mcp.swarm",
                tool = %qualified,
                "card tool dropped: {reason}"
            );
            dropped.push(DroppedTool {
                tool: qualified.clone(),
                reason,
            });
            continue;
        }
        kept.push(qualified);
    }
    (kept, dropped)
}

/// Validate a card's declared `skills`. Skill ids are resolved on the zed
/// side, so an unknown id is already non-fatal (recorded, delegation
/// proceeds) — the shape check just keeps garbage out of the card.
pub fn filter_declared_skills(skills: Vec<String>) -> Vec<String> {
    skills
        .into_iter()
        .filter(|id| {
            let ok = !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
            if !ok {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    skill = %id,
                    "card skill dropped: invalid id shape"
                );
            }
            ok
        })
        .collect()
}
