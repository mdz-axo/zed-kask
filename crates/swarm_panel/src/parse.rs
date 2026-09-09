//! Pure tool-response parsing helpers extracted from `swarm_panel.rs`.
//!
//! These functions take a tool response (the raw `&str` envelope the MCP runtime
//! returns, or an already-unwrapped `serde_json::Value` content object) and turn
//! it into view-model data. They have no `cx`/`window` dependencies — none of
//! them touch `Context`, `Window`, or `Task` — so they are fully unit-tested in
//! this module's own `tests` submodule without a GPUI test harness.
//!
//! The envelope seam lives in `hkask_types::tool_response::parse_tool_response`
//! — the same unwrapper the MCP server test helpers use, so a change to the
//! `{"content": ...}` envelope shape is one edit in one crate. `extract_wallet_balance`
//! calls it; the other parsers here operate on the already-unwrapped content.

use hkask_types::tool_response::parse_tool_response;
use serde::Deserialize;

use crate::PendingPublish;

// ── View model ─────────────────────────────────────────────────────────────

/// Where an agent card lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentSource {
    /// Exists only on ABW (cloud). Can be cloned to local.
    Cloud,
    /// Exists only in the local registry. Can be pushed to cloud.
    Local,
    /// Exists in both — synced via `cloud_swarm_id`. Changes can flow both
    /// directions.
    Synced,
}

#[derive(Clone, Debug)]
pub(crate) struct AgentCard {
    pub(crate) id: String,
    pub(crate) agent_type: String,
    pub(crate) description: String,
    pub(crate) author: String,
    /// Human-readable label for UI display. When empty, the panel falls back
    /// to `id`. Cloned cards carry a display name like "Xaman Ek (Clone)" so
    /// the operator can distinguish the local clone from the cloud original.
    pub(crate) display_name: String,
    /// Where this agent card lives: cloud (ABW only), local (local registry
    /// only), or synced (both, linked by `cloud_swarm_id`).
    pub(crate) source: AgentSource,
}

#[derive(Clone, Debug)]
pub(crate) struct SwarmCard {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    /// Where this swarm lives: `Cloud` (an ABW workspace), `Local` (a
    /// `LocalSwarmRegistry` entry), or `Synced` (a local swarm with a
    /// `cloud_workspace_id` link to an ABW workspace). The backend mode
    /// toggle filters the browse list by this field.
    pub(crate) source: AgentSource,
}

// ── MCP response structs (minimal, mirror hkask-mcp-swarm's tool output) ────

#[derive(Debug, Deserialize)]
pub(crate) struct AgentListResponse {
    pub(crate) agents: Vec<AgentInfo>,
    /// Whether the swarm MCP server has the ABW API key configured
    /// (`self.client.is_authenticated()`). The panel reads this field to
    /// determine API-key status from the same source the server uses —
    /// rather than inferring it from the `swarm_get_swarm` error message,
    /// which conflates "no key configured" with "key configured but
    /// rejected by ABW" (both surface as `permission_denied`).
    pub(crate) authenticated: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AgentInfo {
    pub(crate) agent_id: Option<String>,
    pub(crate) agent_type: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) author: Option<String>,
    /// fermi v0.16.x: human-readable display name. Forwarded by
    /// `swarm_list_agents` from ABW's `build_agent_json`. The cloud fetch path
    /// populates `AgentCard.display_name` from this so the catalogue shows
    /// "Xaman Ek" instead of the `agent_id` slug "xaman_ek". Empty/absent
    /// falls back to `agent_id` in the card renderer (see `card.rs`). Local
    /// cards carry their name under `display_name`, not `display_alias` — the
    /// field-name difference is an ABW-vs-local shape split, not a semantic
    /// one, and the panel unifies them into `AgentCard.display_name`.
    #[serde(default)]
    pub(crate) display_alias: Option<String>,
}

// ── Local agent response (v2 §15 Slice 11) ──────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct LocalAgentListResponse {
    pub(crate) agents: Vec<LocalAgentInfo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LocalAgentInfo {
    pub(crate) agent_id: String,
    pub(crate) agent_type: String,
    #[serde(default)]
    pub(crate) description: String,
    /// Human-readable label for UI display. When empty, the panel falls back
    /// to `agent_id`. Cloned cards carry a display name like "Xaman Ek (Clone)".
    #[serde(default)]
    pub(crate) display_name: String,
    /// The ABW agent id this local card is synced with. `None` = local-only.
    #[serde(default, rename = "cloud_id")]
    pub(crate) cloud_swarm_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkspaceListResponse {
    pub(crate) workspaces: Vec<WorkspaceInfo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkspaceInfo {
    pub(crate) id: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
}

// ── Local swarm response (v2 §15) ─────────────────────────────────────────
//
// `swarm_list_local_swarms` returns `{ count, swarms: [LocalSwarm] }` where each
// `LocalSwarm` is `{ swarm_id, name, mission, cloud_workspace_id }`. Fields are
// declared `Option` so a malformed card degrades to an empty row rather than
// failing the whole list parse — the same defensive pattern as
// `WorkspaceInfo` above.
#[derive(Debug, Deserialize)]
pub(crate) struct LocalSwarmListResponse {
    pub(crate) swarms: Vec<LocalSwarmInfo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LocalSwarmInfo {
    pub(crate) swarm_id: Option<String>,
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) mission: String,
    /// The ABW workspace id this local swarm is synced with. `None` =
    /// local-only. Used to determine the `AgentSource::Synced` source
    /// for swarms.
    #[serde(default)]
    pub(crate) cloud_workspace_id: Option<String>,
}

// ── App primitive response (fermi v0.10.15+) ────────────────────────────────
//
// `swarm_list_apps` returns the App catalogue. The response shape is not
// part of the verified ABW surface, so every field is `Option`/defaulting —
// a malformed entry degrades to an empty row rather than failing the whole
// list parse (same defensive pattern as `WorkspaceInfo`).

/// A single App from the catalogue.
#[derive(Debug, Deserialize, Clone)]
pub(crate) struct AppInfo {
    /// App slug (unique identifier).
    #[serde(default)]
    pub(crate) slug: String,
    /// Human-readable name.
    #[serde(default)]
    pub(crate) name: String,
    /// One-line tagline.
    #[serde(default)]
    pub(crate) tagline: String,
    /// Longer description.
    #[serde(default)]
    pub(crate) description: String,
    /// Visibility: "private", "unlisted", or "public".
    #[serde(default)]
    pub(crate) visibility: String,
    /// Whether the App has been archived.
    #[serde(default)]
    pub(crate) archived: bool,
}

/// Parse the app-list response defensively across plausible envelope
/// shapes (top-level array, `apps` key, or `data.apps`). Returns an empty
/// vec when no array is found.
pub(crate) fn parse_app_list(content: serde_json::Value) -> Vec<AppInfo> {
    let candidates = [Some(&content), content.get("data")];
    for candidate in candidates.into_iter().flatten() {
        if let Some(arr) = candidate.as_array() {
            return arr
                .iter()
                .filter_map(|a| serde_json::from_value::<AppInfo>(a.clone()).ok())
                .collect();
        }
        if let Some(arr) = candidate.get("apps").and_then(|a| a.as_array()) {
            return arr
                .iter()
                .filter_map(|a| serde_json::from_value::<AppInfo>(a.clone()).ok())
                .collect();
        }
    }
    Vec::new()
}

/// The canonical list of tool names exposed by the `swarm` MCP server —
/// re-exported from `hkask_mcp_swarm::TOOL_NAMES`, the single source of truth.
/// `panel_tool_names_match_server` asserts the panel's copy matches the
/// server's live `combined_router()` surface, so a rename/add/remove in the
/// server surfaces here rather than degrading to "tool not found" at runtime.
/// The Steer-mode system prompt's backticked `swarm_*` mentions are
/// verified against this list in `steer_system_prompt`, and asserted
/// in `steer_prompt_mentions_only_known_tools`.
pub(crate) use hkask_mcp_swarm::TOOL_NAMES as SWARM_TOOLS;

/// The kanban MCP server's canonical tool-name list — re-exported from
/// `hkask_mcp_kata_kanban::TOOL_NAMES` (build.rs-generated from the server's
/// `#[tool]` fns), the single source of truth. The Steer-mode system prompt's
/// backticked `kanban_*` mentions are verified against this list in
/// `steer_system_prompt` and asserted in
/// `steer_prompt_mentions_only_known_tools`. One tool
/// (`contract_propose_expect`) does not use the `kanban_` prefix.
pub(crate) use hkask_mcp_kata_kanban::TOOL_NAMES as KANBAN_TOOLS;

/// Extract the algedonic wallet balance from a tool response (the
/// `with_wallet` shape: `content.wallet.balance`). Returns `None` when
/// absent — never a fabricated zero.
pub(crate) fn extract_wallet_balance(output: &str) -> Option<i64> {
    parse_tool_response(output)
        .and_then(|content| content.get("wallet").cloned())
        .and_then(|w| w.get("balance").and_then(|b| b.as_i64()))
}

/// Extract the honest-drop note from a `swarm_create_agent` response (the
/// `unsupported_fields` key the server adds when the caller supplied fields
/// the ABW API cannot store on a non-curated agent — `skills`,
/// `sample_queries`, `dependencies`). Returns `None` when nothing was
/// dropped, so the author status stays clean for fully-stored creates and
/// for `swarm_create_local_agent` (which emits no such key).
pub(crate) fn extract_unsupported_fields_note(output: &str) -> Option<String> {
    let content = parse_tool_response(output)?;
    let unsupported = content.get("unsupported_fields")?.as_array()?;
    if unsupported.is_empty() {
        return None;
    }
    let fields = unsupported
        .iter()
        .filter_map(|f| f.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "Note: the ABW API cannot store these fields on a non-curated agent; they were dropped: {fields}."
    ))
}

/// Extract renderable message lines from a `swarm_run_status` response.
/// The server sanitizes each message's `content`/`response` into the
/// `{content, source, trust}` container; extract the inner text. A missing
/// `messages` array is an error (never an empty status).
pub(crate) fn parse_run_status_messages(content: serde_json::Value) -> Option<Vec<String>> {
    let messages = content.get("messages")?.as_array()?;
    let mut lines = Vec::new();
    for msg in messages {
        // The verified ABW message shape (live, 2026-08-02) carries the
        // sender in `sender_name` (with `sender_id`/`sender_type` beside it),
        // not `sender`/`role` — check it before the fallback so the strip
        // renders the real sender instead of a generic "agent".
        let sender = msg
            .get("agent_id")
            .or_else(|| msg.get("sender_name"))
            .or_else(|| msg.get("sender"))
            .or_else(|| msg.get("role"))
            .and_then(|v| v.as_str())
            .unwrap_or("agent");
        let text = msg
            .get("content")
            .or_else(|| msg.get("response"))
            .and_then(|v| {
                if v.is_string() {
                    v.as_str().map(str::to_string)
                } else {
                    v.get("content")
                        .and_then(|c| c.as_str())
                        .map(str::to_string)
                }
            })
            .unwrap_or_default();
        if !text.trim().is_empty() {
            lines.push(format!("{sender}: {text}"));
        }
    }
    Some(lines)
}

/// Extract agent-name mentions from a Xaman Ek composition response. The
/// curator recommends members in its `response` text and `in_progress` plan;
/// we match `lowercase_with_underscores` tokens that look like agent names.
/// Heuristic by design — the operator reviews before applying.
pub(crate) fn extract_agent_mentions(content: &serde_json::Value) -> Vec<String> {
    let mut found = Vec::new();
    // Prefer the structured plan when present.
    if let Some(members) = content
        .get("in_progress")
        .and_then(|p| p.get("members"))
        .and_then(|m| m.as_array())
    {
        for member in members {
            if let Some(name) = member
                .get("agent_id")
                .and_then(|a| a.as_str())
                .or_else(|| member.get("agent_name").and_then(|a| a.as_str()))
            {
                found.push(name.to_string());
            }
        }
    }
    if !found.is_empty() {
        return found;
    }
    // Fall back to scanning the response text for agent-name-shaped tokens.
    if let Some(text) = content.get("response").and_then(|r| r.as_str()) {
        for token in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if token.len() > 3
                && token.contains('_')
                && token
                    .chars()
                    .all(|c| c.is_lowercase() || c.is_numeric() || c == '_')
            {
                found.push(token.to_string());
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Parse the unwrapped `swarm_publish_checks` response (fermi v0.10.15)
/// into a `PendingPublish`. The contract key is `can_publish` (bool); a
/// missing key is an `Err` rather than a silent false (guessing false would
/// route every publish through the force path). Failing checks are read
/// tolerantly from `checks` or `failing_checks`, each entry a string or an
/// object with a `check`/`name`/`message`/`description` text field — the
/// exact per-check shape is not part of the verified API surface, so we
/// extract whatever text we can without fabricating.
pub(crate) fn parse_publish_checks(
    agent_name: String,
    checks: &serde_json::Value,
) -> Result<PendingPublish, String> {
    let Some(can_publish) = checks.get("can_publish").and_then(|v| v.as_bool()) else {
        return Err(format!("missing can_publish in publish-checks: {checks}"));
    };
    let failing_checks = checks
        .get("checks")
        .or_else(|| checks.get("failing_checks"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    c.as_str()
                        .map(String::from)
                        .or_else(|| c.get("check").and_then(|v| v.as_str()).map(String::from))
                        .or_else(|| c.get("name").and_then(|v| v.as_str()).map(String::from))
                        .or_else(|| c.get("message").and_then(|v| v.as_str()).map(String::from))
                        .or_else(|| {
                            c.get("description")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(PendingPublish {
        agent_name,
        can_publish,
        failing_checks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // The algedonic wallet signal must survive the content envelope and never
    // be fabricated. These pin the extraction against the server's actual
    // output shape (`{"content": {..., "wallet": {"balance": N}}}`).
    #[test]
    fn extract_wallet_balance_reads_content_envelope() {
        let out = r#"{"content":{"count":2,"wallet":{"balance":9977}}}"#;
        assert_eq!(extract_wallet_balance(out), Some(9977));
    }

    #[test]
    fn extract_wallet_balance_absent_when_no_wallet() {
        // Catalogue-only mode: no wallet key → None, never a fabricated zero.
        let out = r#"{"content":{"count":2,"authenticated":false}}"#;
        assert_eq!(extract_wallet_balance(out), None);
    }

    // The honest-drop note rides the same content envelope as the wallet
    // balance — these pin the extraction against the server's actual
    // `swarm_create_agent` output shape.
    #[test]
    fn extract_unsupported_fields_note_lists_dropped_fields() {
        let out = r#"{"content":{"agent_id":"abc","agent_name":"my_agent","unsupported_fields":["sample_queries"],"note":"the ABW API cannot store these fields on a non-curated agent; they were dropped: sample_queries"}}"#;
        let note = extract_unsupported_fields_note(out).expect("note must be extracted");
        assert!(note.contains("sample_queries"));
    }

    #[test]
    fn extract_unsupported_fields_note_absent_when_fully_stored() {
        // No unsupported_fields key (fully-stored create, or a local create
        // which never emits it) → None, so the author status stays clean.
        let out = r#"{"content":{"agent_id":"abc","agent_name":"my_agent"}}"#;
        assert_eq!(extract_unsupported_fields_note(out), None);
    }

    #[test]
    fn extract_wallet_balance_absent_on_garbage() {
        assert_eq!(extract_wallet_balance("not json"), None);
        assert_eq!(extract_wallet_balance("{}"), None);
    }

    // Item 3: the run-status strip extracts message lines, unwrapping the
    // server's {content, source, trust} sanitize container.
    #[test]
    fn parse_run_status_messages_unwraps_sanitize_container() {
        let content = serde_json::json!({
            "messages": [
                { "agent_id": "market_analyst", "content": { "content": "analyzed the sector", "source": "abw" } },
                { "sender": "system", "content": "plain text message" }
            ]
        });
        let lines = parse_run_status_messages(content).expect("messages");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "market_analyst: analyzed the sector");
        assert_eq!(lines[1], "system: plain text message");
    }

    #[test]
    fn parse_run_status_messages_none_without_messages() {
        assert!(parse_run_status_messages(serde_json::json!({ "error": "x" })).is_none());
    }

    #[test]
    fn parse_run_status_messages_uses_verified_sender_name() {
        // The verified ABW message shape (live, 2026-08-02) carries the
        // sender in `sender_name` — the strip must render it, not the
        // generic "agent" fallback.
        let content = serde_json::json!({
            "messages": [{
                "content": {"content": "telemetry reported", "source": "abw", "trust": "untrusted"},
                "sender_name": "sensor_advisor",
                "sender_type": "agent",
                "message_id": "m1",
                "created_at": "2026-08-02T00:00:00Z",
            }]
        });
        let lines = parse_run_status_messages(content).expect("parse");
        assert_eq!(lines, vec!["sensor_advisor: telemetry reported"]);
    }

    // key and the tolerant failing-checks extraction, so a server shape change
    // surfaces here rather than silently routing every publish through the
    // force path.
    #[test]
    fn parse_publish_checks_reads_can_publish_true() {
        let checks = serde_json::json!({
            "can_publish": true,
            "checks": []
        });
        let pending = parse_publish_checks("sensor_advisor".to_string(), &checks).expect("parse");
        assert!(pending.can_publish);
        assert!(pending.failing_checks.is_empty());
        assert_eq!(pending.agent_name, "sensor_advisor");
    }

    #[test]
    fn parse_publish_checks_collects_failing_checks_as_strings() {
        let checks = serde_json::json!({
            "can_publish": false,
            "checks": ["missing description", "system_prompt empty"]
        });
        let pending = parse_publish_checks("alpha".to_string(), &checks).expect("parse");
        assert!(!pending.can_publish);
        assert_eq!(
            pending.failing_checks,
            vec!["missing description", "system_prompt empty"]
        );
    }

    #[test]
    fn parse_publish_checks_extracts_object_check_text_fields() {
        // The per-check object shape is not part of the verified API surface;
        // tolerate `check`/`name`/`message`/`description` text fields.
        let checks = serde_json::json!({
            "can_publish": false,
            "checks": [
                {"check": "name"},
                {"name": "desc"},
                {"message": "tags"},
                {"description": "prompt"},
                {"unrelated": 7}
            ]
        });
        let pending = parse_publish_checks("alpha".to_string(), &checks).expect("parse");
        assert_eq!(
            pending.failing_checks,
            vec!["name", "desc", "tags", "prompt"]
        );
    }

    #[test]
    fn parse_publish_checks_accepts_failing_checks_alias() {
        // Some servers emit `failing_checks`; tolerate it as a fallback.
        let checks = serde_json::json!({
            "can_publish": false,
            "failing_checks": ["no tags"]
        });
        let pending = parse_publish_checks("alpha".to_string(), &checks).expect("parse");
        assert_eq!(pending.failing_checks, vec!["no tags"]);
    }

    #[test]
    fn parse_publish_checks_missing_can_publish_is_error() {
        // A missing `can_publish` must be an error, not a silent false —
        // guessing false would route every publish through the force path.
        let checks = serde_json::json!({ "checks": [] });
        let result = parse_publish_checks("alpha".to_string(), &checks);
        assert!(result.is_err());
    }
}
