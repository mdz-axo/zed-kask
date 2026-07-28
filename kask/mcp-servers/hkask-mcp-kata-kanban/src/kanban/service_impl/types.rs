//! Kanban error types and de-jam data structures.

use crate::kata::KataError;
use hkask_types::NotFound;
use hkask_types::id::TaskId;

use crate::TaskStatus;

/// UnjamItem — a stuck state detected by the de-jammer.
#[derive(Debug, Clone)]
pub struct UnjamItem {
    pub task_id: TaskId,
    pub task_title: String,
    pub issue: String,
    pub suggestion: String,
}

/// UnjamFix — records an auto-fix action taken by the de-jammer.
#[derive(Debug, Clone)]
pub struct UnjamFix {
    pub task_id: TaskId,
    pub task_title: String,
    pub action: String,
}

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

impl From<KataError> for KanbanError {
    fn from(e: KataError) -> Self {
        KanbanError::Internal(format!("kata engine: {e}"))
    }
}

impl From<NotFound> for KanbanError {
    fn from(nf: NotFound) -> Self {
        KanbanError::NotFound(nf)
    }
}
