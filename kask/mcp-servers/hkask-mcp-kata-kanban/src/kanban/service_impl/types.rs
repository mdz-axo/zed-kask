//! Kanban error types.

use hkask_types::NotFound;
use hkask_types::id::TaskId;

use crate::Task;
use crate::TaskStatus;

/// Errors specific to kanban operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum KanbanError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("not found: {0}")]
    NotFound(NotFound),

    #[error("invalid state transition: task {task} cannot move from {from} to {to}")]
    InvalidTransition {
        task: TaskId,
        from: TaskStatus,
        to: TaskStatus,
    },

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("WIP limit exceeded: column '{column}' has {current}/{limit} tasks (limit: {limit})")]
    WipLimitExceeded {
        column: String,
        limit: u32,
        current: u32,
    },
}

impl From<NotFound> for KanbanError {
    fn from(nf: NotFound) -> Self {
        KanbanError::NotFound(nf)
    }
}

/// One indexed task row that could not be read for a board listing or WIP
/// count: the row is named and the reason stated, so the degradation is
/// surfaced to the caller instead of silently shrinking the read (the
/// fail-visible contract goal rows already follow — a task read degrades
/// into a response note rather than an error, so one bad row cannot blind
/// the whole board; the WIP admission additionally fails closed on these
/// rows, because an unreadable row can only undercount and undercounting
/// over-admits).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskIndexFault {
    /// The indexed task id whose row could not be read.
    pub task_id: String,
    /// Why the row could not be read (missing row, or the decode error).
    pub reason: String,
}

/// The result of a board task listing: the readable tasks plus the indexed
/// rows that could not be read — surfaced, never silently dropped.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TaskListReport {
    pub tasks: Vec<Task>,
    pub undecodable: Vec<TaskIndexFault>,
}
