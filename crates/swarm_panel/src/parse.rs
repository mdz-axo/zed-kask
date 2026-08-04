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

use gpui::SharedString;
use hkask_types::tool_response::parse_tool_response;
use ui::Color;

use crate::{PendingPublish, SwarmRosterAgent};

/// The canonical list of tool names exposed by the `swarm` MCP server —
/// every `#[tool]` fn in `hkask-mcp-swarm/src/hkask_mcp_swarm.rs`. This is the
/// single source of truth shared by `panel_tool_names_match_server` (which
/// pins the count against the server so a rename/add/remove is caught) and the
/// Steer-mode system prompt (whose backticked `swarm_*` mentions are
/// `debug_assert!`ed against this list in `steer_system_prompt`, and asserted
/// in `steer_prompt_mentions_only_known_tools`). A rename in `hkask-mcp-swarm`
/// must update this list — the count test fails first, and any stale prompt
/// mention is caught next, so a rename surfaces here rather than degrading to
/// "tool not found" at runtime.
pub(crate) const SWARM_TOOLS: &[&str] = &[
    // ABW (cloud) tools.
    "swarm_list_agents",
    "swarm_get_swarm",
    "swarm_get_agent",
    "swarm_list_apps",
    "swarm_ontology_templates",
    "swarm_execute_agent",
    "swarm_hire_cost",
    "swarm_request_consent",
    "swarm_authorize_session",
    "swarm_hire",
    "swarm_delegate",
    "swarm_delegate_and_wait",
    "swarm_fanout",
    "swarm_run_status",
    "swarm_generate_prompt",
    "swarm_generate_ontology",
    "swarm_create_agent",
    "swarm_create_swarm",
    "swarm_xaman",
    "swarm_create_app",
    "swarm_search_knowledge",
    "swarm_fork_agent",
    "swarm_fire",
    "swarm_delete_agent",
    "swarm_delete_swarm",
    "swarm_publish_checks",
    "swarm_publish_agent",
    // v2 §15 local tools (Slices 9 + 11).
    "swarm_fund_local",
    "swarm_balance_local",
    "swarm_local_history",
    "swarm_delegate_local",
    "swarm_fanout_local",
    "swarm_pipeline_local",
    "swarm_list_local_agents",
    "swarm_clone_to_local",
    "swarm_remove_local",
    "swarm_create_local_agent",
    "swarm_reconfigure_local_agent",
    "swarm_push_to_cloud",
    "swarm_search_knowledge_local",
    "swarm_generate_prompt_local",
    "swarm_generate_ontology_local",
    // Local swarms (the local replica of ABW workspaces).
    "swarm_create_local_swarm",
    "swarm_list_local_swarms",
    "swarm_get_local_swarm",
    "swarm_delete_local_swarm",
    "swarm_add_agent_local",
    "swarm_remove_agent_local",
    // Agent2Agent protocol.
    "swarm_a2a_send",
    "swarm_a2a_card",
];

/// Extract the algedonic wallet balance from a tool response (the
/// `with_wallet` shape: `content.wallet.balance`). Returns `None` when
/// absent — never a fabricated zero.
pub(crate) fn extract_wallet_balance(output: &str) -> Option<i64> {
    parse_tool_response(output)
        .and_then(|content| content.get("wallet").cloned())
        .and_then(|w| w.get("balance").and_then(|b| b.as_i64()))
}

/// Extract a swarm's hired agents from a `swarm_get_swarm` response.
/// ABW's exact roster shape is not part of the verified surface, so this
/// parses defensively across the plausible envelopes: an `agents` array at
/// the top level, under `workspace`, or under `team`. Each agent's
/// `description` is a plain sanitized string (the server's display-field
/// sanitizer guarantees that). Returns `None` when no roster array is found
/// (a malformed response is an error, never an empty roster).
pub(crate) fn parse_swarm_roster(content: serde_json::Value) -> Option<Vec<SwarmRosterAgent>> {
    let candidates = [
        content.get("agents"),
        content.get("workspace").and_then(|w| w.get("agents")),
        content.get("team").and_then(|t| t.get("agents")),
        content
            .get("workspace")
            .and_then(|w| w.get("team"))
            .and_then(|t| t.get("agents")),
    ];
    let agents = candidates.into_iter().find_map(|c| c?.as_array())?;
    Some(
        agents
            .iter()
            .filter_map(|a| {
                let agent_id = a
                    .get("agent_id")
                    .or_else(|| a.get("agent_name"))
                    .and_then(|v| v.as_str())?;
                Some(SwarmRosterAgent {
                    agent_id: agent_id.to_string(),
                    agent_type: a
                        .get("agent_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    description: a
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
            })
            .collect(),
    )
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

/// Compute a freshness chip for a cloud agent from its `updated_at`
/// timestamp (fermi v0.10.27). Returns `None` for local cards (no
/// timestamp) or when the timestamp can't be parsed — never fabricates an
/// age. Cloud agents render the age muted, switching to Warning past 30 days
/// (the same heuristic window kask uses for chronic staleness).
pub(crate) fn staleness_chip(updated_at: &Option<String>) -> Option<(SharedString, Color)> {
    let ts = updated_at.as_ref()?;
    let dt = chrono::DateTime::parse_from_rfc3339(ts.trim()).ok()?;
    let days = chrono::Utc::now()
        .signed_duration_since(dt.with_timezone(&chrono::Utc))
        .num_days();
    let label = if days <= 0 {
        "updated today".to_string()
    } else {
        format!("updated {days}d ago")
    };
    let color = if days >= 30 {
        Color::Warning
    } else {
        Color::Muted
    };
    Some((SharedString::from(label), color))
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
    use ui::Color;

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

    #[test]
    fn extract_wallet_balance_absent_on_garbage() {
        assert_eq!(extract_wallet_balance("not json"), None);
        assert_eq!(extract_wallet_balance("{}"), None);
    }

    // Item 4: the roster drill-down parses ABW's workspace payload
    // defensively across envelope shapes, and never fabricates an empty
    // roster from a malformed response.
    #[test]
    fn parse_swarm_roster_reads_top_level_agents() {
        let content = serde_json::json!({
            "agents": [
                { "agent_id": "market_analyst", "agent_type": "research", "description": "d" },
                { "agent_id": "writer", "agent_type": "creative" }
            ]
        });
        let roster = parse_swarm_roster(content).expect("roster");
        assert_eq!(roster.len(), 2);
        assert_eq!(roster[0].agent_id, "market_analyst");
        assert_eq!(roster[0].description, "d");
        assert_eq!(roster[1].description, ""); // missing description defaults empty
    }

    #[test]
    fn parse_swarm_roster_reads_nested_workspace_agents() {
        let content =
            serde_json::json!({ "workspace": { "id": "ws", "agents": [{ "agent_id": "a1" }] } });
        let roster = parse_swarm_roster(content).expect("roster");
        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].agent_id, "a1");
    }

    #[test]
    fn parse_swarm_roster_none_without_agents_array() {
        assert!(parse_swarm_roster(serde_json::json!({ "error": "x" })).is_none());
        assert!(parse_swarm_roster(serde_json::json!({ "workspace": {} })).is_none());
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

    #[test]
    fn parse_swarm_roster_reads_verified_detail_shape() {
        // The verified `/workspaces/{id}` detail shape (live, 2026-08-02):
        // top-level `agents` whose entries carry agent_id/agent_type/
        // description (plus more fields the panel ignores).
        let content = serde_json::json!({
            "id": "ws-1",
            "name": "alpha",
            "is_composition": false,
            "members": [],
            "agents": [{
                "agent_id": "sensor_advisor",
                "agent_name": "sensor_advisor",
                "agent_type": "sensor",
                "description": "reads telemetry",
                "accepts": [],
                "produces": [],
                "total_executions": 12,
                "tags": [],
            }],
            "workspace_budget": 500,
            "workspace_remaining": 200,
        });
        let roster = parse_swarm_roster(content).expect("roster");
        assert_eq!(roster.len(), 1);
        assert_eq!(roster[0].agent_id, "sensor_advisor");
        assert_eq!(roster[0].agent_type, "sensor");
        assert_eq!(roster[0].description, "reads telemetry");
    }

    // Publish-checks parse (fermi v0.10.15). Pins the `can_publish` contract
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

    #[test]
    fn staleness_chip_none_without_timestamp() {
        // Local cards carry no `updated_at` — never fabricate an age.
        assert!(staleness_chip(&None).is_none());
    }

    #[test]
    fn staleness_chip_none_on_unparseable_timestamp() {
        assert!(staleness_chip(&Some("not-a-date".to_string())).is_none());
    }

    #[test]
    fn staleness_chip_warns_past_30_days() {
        // A timestamp 40 days in the past renders a Warning chip.
        let old = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(40))
            .expect("40 days ago")
            .to_rfc3339();
        let (label, color) = staleness_chip(&Some(old)).expect("chip");
        assert_eq!(color, Color::Warning);
        assert!(label.contains("40d ago"));
    }
}
