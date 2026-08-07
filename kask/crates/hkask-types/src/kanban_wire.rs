//! Shared kanban wire constants — the contract between the `hkask-mcp-kata-kanban`
//! MCP server and the `hkask-kanban-widget` GPUI view.
//!
//! Both crates reference these constants so the server's binary name, the
//! `kanban_task_move` tool name, and the five standard task-status wire strings
//! live in one place. A rename in the server is reflected here; the widget
//! picks it up without a silent break. The widget's `STANDARD_STATUSES` display
//! labels pair the wire keys (defined here) with human labels (widget-local,
//! since the server has no display concern).
//!
//! These are **wire strings**, not the `TaskStatus` enum (which lives in the
//! MCP server crate). The enum is the server's internal representation; the
//! wire strings are the JSON-level contract that crosses the crate boundary
//! via the ```` ```kanban ```` block body and the `kanban_task_move` args.

/// MCP server binary name. Used by the widget as the fallback dispatch target
/// when a block carries no dispatchable provenance, and by the server's own
/// `run()` entrypoint.
pub const KANBAN_SERVER_NAME: &str = "hkask-mcp-kata-kanban";

/// The MCP tool the widget dispatches to move a task between columns. The
/// widget's move affordance invokes this tool (not the tool that produced the
/// block) with `{ task_id, target_status }` args.
pub const KANBAN_TASK_MOVE_TOOL: &str = "kanban_task_move";

/// The five standard task-status wire strings in display order, matching the
/// server's `TaskStatus::as_str()` output. The widget renders columns in this
/// order; the server's `TaskStatus::parse_str()` accepts these (plus
/// case-insensitive aliases like `inprogress`).
/// (`backlog`, `ready`, `in_progress`, `review`, `done`).
pub const STANDARD_STATUS_KEYS: &[&str] =
    &["backlog", "ready", "in_progress", "review", "done"];

/// Returns `true` if `status` is one of the five standard wire strings.
/// The widget uses this to validate a move target before dispatch; the server
/// accepts the same set via `TaskStatus::parse_str`.
#[must_use]
pub fn is_standard_status(status: &str) -> bool {
    STANDARD_STATUS_KEYS.contains(&status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_status_keys_match_server_order() {
        // Pin the wire order: Backlog → Ready → InProgress → Review → Done.
        assert_eq!(STANDARD_STATUS_KEYS.len(), 5);
        assert_eq!(STANDARD_STATUS_KEYS[0], "backlog");
        assert_eq!(STANDARD_STATUS_KEYS[1], "ready");
        assert_eq!(STANDARD_STATUS_KEYS[2], "in_progress");
        assert_eq!(STANDARD_STATUS_KEYS[3], "review");
        assert_eq!(STANDARD_STATUS_KEYS[4], "done");
    }

    #[test]
    fn is_standard_status_accepts_the_five() {
        for &key in STANDARD_STATUS_KEYS {
            assert!(is_standard_status(key), "expected '{key}' to be standard");
        }
    }

    #[test]
    fn is_standard_status_rejects_non_standard() {
        assert!(!is_standard_status(""));
        assert!(!is_standard_status("inprogress"));
        assert!(!is_standard_status("IN_PROGRESS"));
        assert!(!is_standard_status("archived"));
    }

    #[test]
    fn server_name_and_tool_are_stable() {
        // These are part of the wire contract; a rename is a breaking change
        // that must update both the server's run() and the widget's fallback.
        assert_eq!(KANBAN_SERVER_NAME, "hkask-mcp-kata-kanban");
        assert_eq!(KANBAN_TASK_MOVE_TOOL, "kanban_task_move");
    }
}
