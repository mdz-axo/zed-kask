//! Shared kanban wire constants — the contract between the `hkask-mcp-kata-kanban`
//! MCP server, the `hkask-kanban-widget` GPUI view, and the `kanban_panel`.
//!
//! The five standard task-status wire strings now live in
//! [`crate::TaskStatus`] (re-exported from [`crate::kanban_status`]), which is
//! the single source of truth shared by the server and the widget. This module
//! retains only the server-binary name, the move-tool name, and the
//! board-name length cap — the wire constants that are *not* derivable from
//! the `TaskStatus` enum.

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

/// The maximum board-name length, in characters, enforced at the
/// kata-kanban service boundary (`validate_board_name` — the single
/// validation point every caller routes through: the panel's forms, MCP
/// agents, and mermaid import). Operator decision (2026-09-18),
/// Planka-aligned: Planka caps board names at 128
/// (`AddBoardStep.jsx` `maxLength={128}`; Kan allows 255). The panel's
/// create/rename forms refuse longer names client-side so the typed text
/// is not lost to a server rejection.
pub const KANBAN_BOARD_NAME_MAX_CHARS: usize = 128;
