//! Pure tool-response parsing types for `swarm_panel.rs`.
//!
//! These types take a tool response (the raw `&str` envelope the MCP runtime
//! returns, or an already-unwrapped `serde_json::Value` content object) and
//! turn it into view-model data. They have no `cx`/`window` dependencies so
//! they are fully unit-testable without a GPUI test harness.
//!
//! The envelope seam lives in `hkask_types::tool_response::parse_tool_response`
//! — the same unwrapper the MCP server test helpers use, so a change to the
//! `{"content": ...}` envelope shape is one edit in one crate.

use serde::Deserialize;

// ── View model ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub(crate) struct AgentCard {
    pub(crate) id: String,
    pub(crate) agent_type: String,
    pub(crate) description: String,
    /// Human-readable label for UI display. When empty, the panel falls back
    /// to `id`.
    pub(crate) display_name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SwarmCard {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
}

// ── Local agent response ────────────────────────────────────────────────────

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
    /// to `agent_id`.
    #[serde(default)]
    pub(crate) display_name: String,
}

// ── Local swarm response ────────────────────────────────────────────────────
//
// `swarm_list_local_swarms` returns `{ count, swarms: [LocalSwarm] }` where each
// `LocalSwarm` is `{ swarm_id, name, mission }`. The id/name fields are
// declared `Option` so a malformed card degrades to an empty row rather than
// failing the whole list parse.
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
}

/// The canonical list of tool names exposed by the `swarm` MCP server —
/// re-exported from `hkask_mcp_swarm::TOOL_NAMES`, the single source of truth.
/// `panel_tool_names_match_server` asserts the panel's copy matches the
/// server's live router surface, so a rename/add/remove in the server
/// surfaces here rather than degrading to "tool not found" at runtime. The
/// Steer-mode system prompt's backticked `swarm_*` mentions are verified
/// against this list in `steer_system_prompt`, and asserted in
/// `steer_prompt_mentions_only_known_tools`.
pub(crate) use hkask_mcp_swarm::TOOL_NAMES as SWARM_TOOLS;

/// The kanban MCP server's canonical tool-name list — re-exported from
/// `hkask_mcp_kata_kanban::TOOL_NAMES` (build.rs-generated from the server's
/// `#[tool]` fns), the single source of truth. The Steer-mode system prompt's
/// backticked `kanban_*` mentions are verified against this list in
/// `steer_system_prompt` and asserted in
/// `steer_prompt_mentions_only_known_tools`. One tool
/// (`contract_propose_expect`) does not use the `kanban_` prefix.
pub(crate) use hkask_mcp_kata_kanban::TOOL_NAMES as KANBAN_TOOLS;
