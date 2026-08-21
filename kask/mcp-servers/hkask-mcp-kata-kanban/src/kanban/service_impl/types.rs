//! Kanban error types.

use hkask_types::NotFound;
use hkask_types::id::TaskId;

use crate::TaskStatus;

/// Errors specific to kanban operations.
#[derive(Debug, Clone, thiserror::Error)]
pub(crate) enum KanbanError {
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
