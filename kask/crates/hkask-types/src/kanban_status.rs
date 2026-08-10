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

    /// Parses a case-insensitive string into a `TaskStatus`. Accepts the five
    /// standard wire strings plus the aliases `inprogress` and `in-progress`.
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "backlog" => Some(TaskStatus::Backlog),
            "ready" => Some(TaskStatus::Ready),
            "in_progress" | "inprogress" | "in-progress" => Some(TaskStatus::InProgress),
            "review" => Some(TaskStatus::Review),
            "done" => Some(TaskStatus::Done),
            _ => None,
        }
    }

    /// Returns `true` iff the transition from `self` to `target` is valid
    /// (forward one step, or backward one step — no skipping).
    pub fn can_transition_to(&self, target: TaskStatus) -> bool {
        use TaskStatus::*;
        matches!(
            (self, target),
            (Backlog, Ready)
                | (Ready, Backlog)
                | (Ready, InProgress)
                | (InProgress, Ready)
                | (InProgress, Review)
                | (Review, InProgress)
                | (Review, Done)
        )
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
    use super::*;

    #[test]
    fn as_str_round_trips_through_parse_str() {
        for status in TaskStatus::STANDARD_ORDER {
            let s = status.as_str();
            assert_eq!(TaskStatus::parse_str(s), Some(status));
            assert_eq!(TaskStatus::parse_str(&s.to_uppercase()), Some(status));
        }
    }

    #[test]
    fn parse_str_accepts_aliases() {
        assert_eq!(
            TaskStatus::parse_str("inprogress"),
            Some(TaskStatus::InProgress)
        );
        assert_eq!(
            TaskStatus::parse_str("in-progress"),
            Some(TaskStatus::InProgress)
        );
    }

    #[test]
    fn parse_str_rejects_unknown() {
        assert!(TaskStatus::parse_str("").is_none());
        assert!(TaskStatus::parse_str("archived").is_none());
        assert!(TaskStatus::parse_str("blocked").is_none());
    }

    #[test]
    fn next_returns_none_at_done() {
        assert_eq!(TaskStatus::Done.next(), None);
        assert_eq!(TaskStatus::Review.next(), Some(TaskStatus::Done));
    }

    #[test]
    fn can_transition_to_allows_one_step_either_direction() {
        assert!(TaskStatus::Backlog.can_transition_to(TaskStatus::Ready));
        assert!(TaskStatus::Ready.can_transition_to(TaskStatus::Backlog));
        assert!(TaskStatus::Ready.can_transition_to(TaskStatus::InProgress));
        assert!(!TaskStatus::Backlog.can_transition_to(TaskStatus::InProgress));
        assert!(!TaskStatus::Backlog.can_transition_to(TaskStatus::Done));
    }

    #[test]
    fn standard_order_is_backlog_ready_inprogress_review_done() {
        assert_eq!(
            TaskStatus::STANDARD_ORDER,
            [
                TaskStatus::Backlog,
                TaskStatus::Ready,
                TaskStatus::InProgress,
                TaskStatus::Review,
                TaskStatus::Done,
            ]
        );
    }

    #[test]
    fn display_matches_as_str() {
        assert_eq!(format!("{}", TaskStatus::InProgress), "in_progress");
    }

    #[test]
    fn from_str_round_trips() {
        let status: TaskStatus = "review".parse().unwrap();
        assert_eq!(status, TaskStatus::Review);
        assert!("invalid".parse::<TaskStatus>().is_err());
    }
}
