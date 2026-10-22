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

/// Validate a card's declared `mcp_tools`. Each entry must be `server/tool`
/// with charset-safe, non-empty segments. When `allowed_servers` is set (the
/// governed server set from `HKASK_MCP_SERVER_IDS`), entries whose server is
/// not in it are dropped — a card must not extend the delegated tool surface
/// beyond the operator's own governed servers. Dropped entries are logged so
/// the operator sees what was filtered (the `.rules` startup-failure-signal
/// trap: a silent drop is indistinguishable from "nothing to drop").
pub fn filter_mcp_tools(tools: Vec<String>, allowed_servers: Option<&[String]>) -> Vec<String> {
    let mut kept = Vec::new();
    for qualified in tools {
        let Some((server, tool)) = qualified.split_once('/') else {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                tool = %qualified,
                "card tool dropped: not server/tool shaped"
            );
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
            continue;
        }
        if let Some(allowed) = allowed_servers
            && !allowed.iter().any(|s| s == server)
        {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                tool = %qualified,
                "card tool dropped: server not in the governed set (HKASK_MCP_SERVER_IDS)"
            );
            continue;
        }
        kept.push(qualified);
    }
    kept
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
