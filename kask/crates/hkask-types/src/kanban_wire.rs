//! Shared kanban wire constants — the contract between the `hkask-mcp-kata-kanban`
//! MCP server and the `hkask-kanban-widget` GPUI view.
//!
//! The five standard task-status wire strings now live in
//! [`crate::TaskStatus`] (re-exported from [`crate::kanban_status`]), which is
//! the single source of truth shared by the server and the widget. This module
//! retains only the server-binary name and the move-tool name — the wire
//! constants that are *not* derivable from the `TaskStatus` enum.

/// MCP server id — MUST match the `id` field in
/// `kask_bridge::mcp_servers::BUILT_IN_MCP_SERVERS` (currently `"kata-kanban"`).
/// The runtime stores live connections keyed by this id, so any mismatch
/// between this constant and the registry id causes `invoke_tool` to fail
/// with "Server registered but not connected" — the connection exists under
/// the registry id but the panel looks it up under this constant.
/// The widget also resolves the server name from block provenance.
pub const KANBAN_SERVER_NAME: &str = "kata-kanban";

/// The MCP tool the widget dispatches to move a task between columns. The
/// widget's move affordance invokes this tool (not the tool that produced the
/// block) with `{ task_id, target_status }` args.
pub const KANBAN_TASK_MOVE_TOOL: &str = "kanban_task_move";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_name_and_tool_are_stable() {
        // These are part of the wire contract; a rename is a breaking change
        // that must update both the server's run() and the widget's move tool.
        assert_eq!(KANBAN_SERVER_NAME, "kata-kanban");
        assert_eq!(KANBAN_TASK_MOVE_TOOL, "kanban_task_move");
    }
}
