//! `TaskStatus` — the lifecycle state of a kanban task.
//!
//! This is the single source of truth for the five standard task-status wire
//! strings. Both the `hkask-mcp-kata-kanban` MCP server and the
//! `hkask-kanban-widget` GPUI view import `TaskStatus` from here, so the wire
//! strings and the transition rules cannot drift between them.
//!
//! Column ordering is strict: transitions may only advance forward or regress
//! one step backward. Skipping columns is prohibited.
//!
//! Exception: `KanbanService::task_reopen` moves Done→InProgress directly
//! (skipping Review) as an explicit rework escape hatch. This is the only
//! sanctioned multi-step transition.
//!
//! ```text
//! Backlog → Ready → InProgress → Review → Done
//! ```

use serde::{Deserialize, Serialize};

/// TaskStatus — lifecycle state of a kanban task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    /// Task is queued, not yet ready for work.
    Backlog,
    /// Task is ready to be picked up.
    Ready,
    /// Task is actively being worked on.
    InProgress,
    /// Task is complete and awaiting review/verification.
    Review,
    /// Task has been verified and is done.
    Done,
}

impl TaskStatus {
    /// Returns the string representation (lowercase). This is the wire string
    /// used in ```` ```kanban ```` block bodies and `kanban_task_move` args.
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Backlog => "backlog",
            TaskStatus::Ready => "ready",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Review => "review",
            TaskStatus::Done => "done",
        }
    }

    /// Parses a case-insensitive string into a `TaskStatus`. The input is
    /// normalized first (trimmed; matched surrounding quotes stripped — the
    /// canonical `normalize_closed_vocab` contract, operator ruling
    /// 2026-10-13: every closed-vocab MCP field parses through it), so
    /// padded or quoted values like `" in_progress "` resolve instead of
    /// burning a retry on an invalid-argument refusal. Accepts the five
    /// standard wire strings plus the aliases `inprogress` and `in-progress`.
    pub fn parse_str(s: &str) -> Option<Self> {
        match crate::normalize_closed_vocab(s).to_lowercase().as_str() {
            "backlog" => Some(TaskStatus::Backlog),
            "ready" => Some(TaskStatus::Ready),
            "in_progress" | "inprogress" | "in-progress" => Some(TaskStatus::InProgress),
            "review" => Some(TaskStatus::Review),
            "done" => Some(TaskStatus::Done),
            _ => None,
        }
    }

    /// Returns the next status in the workflow, or `None` if already `Done`.
    pub fn next(&self) -> Option<TaskStatus> {
        match self {
            TaskStatus::Backlog => Some(TaskStatus::Ready),
            TaskStatus::Ready => Some(TaskStatus::InProgress),
            TaskStatus::InProgress => Some(TaskStatus::Review),
            TaskStatus::Review => Some(TaskStatus::Done),
            TaskStatus::Done => None,
        }
    }

    /// The five standard statuses in display order.
    pub const STANDARD_ORDER: [TaskStatus; 5] = [
        TaskStatus::Backlog,
        TaskStatus::Ready,
        TaskStatus::InProgress,
        TaskStatus::Review,
        TaskStatus::Done,
    ];
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_str(s).ok_or_else(|| format!("invalid TaskStatus: {s}"))
    }
}

#[cfg(test)]
mod tests {
    use super::TaskStatus;

    /// Closed-vocab normalization (operator ruling 2026-10-13): padded and
    /// quoted wire values resolve instead of burning a retry on an
    /// invalid-argument refusal.
    #[test]
    fn parse_str_normalizes_padding_and_quotes() {
        assert_eq!(TaskStatus::parse_str(" ready "), Some(TaskStatus::Ready));
        assert_eq!(TaskStatus::parse_str("\"ready\""), Some(TaskStatus::Ready));
        assert_eq!(
            TaskStatus::parse_str(" 'In-Progress' "),
            Some(TaskStatus::InProgress)
        );
        assert_eq!(TaskStatus::parse_str("backlog"), Some(TaskStatus::Backlog));
        assert_eq!(TaskStatus::parse_str("nonsense"), None);
    }
}
