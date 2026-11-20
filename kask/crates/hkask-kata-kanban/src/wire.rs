//! Wire contract shared between `hkask-mcp-kata-kanban` (the MCP server) and
//! in-process consumers such as `kanban_panel` (P12.1 — panels do not link
//! `hkask-mcp-*` server crates).
//!
//! `hkask-mcp-kata-kanban` pins `TOOL_NAMES` against its live `rmcp` tool
//! router in unit tests so any added, removed, or renamed tool fails the pin
//! instead of drifting silently.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Canonical sorted list of MCP tool names exposed by `hkask-mcp-kata-kanban`.
///
/// Pinned against `KanbanServer::tool_router()` by
/// `hkask_mcp_kata_kanban::wire_tool_names_match_server_surface`.
pub const TOOL_NAMES: &[&str] = &[
    "kanban_board_create",
    "kanban_board_delete",
    "kanban_board_export",
    "kanban_board_import",
    "kanban_board_list",
    "kanban_board_update",
    "kanban_goal_create",
    "kanban_goal_get",
    "kanban_goal_judge",
    "kanban_goal_list",
    "kanban_task_add_deliverable",
    "kanban_task_assign",
    "kanban_task_claim_next",
    "kanban_task_comment",
    "kanban_task_comments_since",
    "kanban_task_create",
    "kanban_task_delete",
    "kanban_task_kata_prompt",
    "kanban_task_kata_record",
    "kanban_task_list",
    "kanban_task_move",
    "kanban_task_reopen",
    "kanban_task_spawn",
    "kanban_task_unassign",
    "kanban_task_update",
    "kanban_task_verify",
];

/// Request for `kanban_task_spawn`. Unknown fields are refused: the former
/// `delegation_level` and `memory_scope` were accepted, validated and had no
/// effect on the child (mcp-tool-review KK-18), so a caller still sending
/// them is told rather than silently ignored.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskSpawnRequest {
    pub task_id: String,
    pub idempotency_key: Option<String>,
    /// Skills named in the child's prompt (e.g. ["bug-hunt", "tdd"]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delegated_skills: Vec<String>,
}
