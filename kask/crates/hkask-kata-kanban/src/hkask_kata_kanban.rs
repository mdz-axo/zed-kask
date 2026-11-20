#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hkask-kata-kanban — Kata-Kanban domain service, types, and idempotency store.
//!
//! Owns the durable board/task/goal state machine, mermaid export/import,
//! and replay-protection idempotency store consumed by both
//! `hkask-mcp-kata-kanban` (the MCP server surface) and `kask_bridge`
//! (the native goal host).

pub mod idempotency;
pub mod kanban;
pub mod wire;

pub use kanban::KanbanError;
pub use kanban::KanbanService;
pub use kanban::types::goal::*;
pub use kanban::{
    Board, ColumnDef, Comment, KataExperimentRecord, KataStateUpdate, Priority, Task,
    TaskKataState, TaskSpec, TaskStatus, Verification, VerificationCriterion,
};
