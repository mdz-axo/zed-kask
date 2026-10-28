//! Kanban board mechanics — agent coordination via headless task boards.
//!
//! Every type carries `owner: WebID` (P12 — anonymous agency prohibition).
//! Task status transitions are column-ordered: Backlog → Ready → InProgress → Review → Done.
//!
//! ## Module Structure
//!
//! - `types` — Core types: Board, Task, TaskSpec, TaskStatus, Priority, SpawnSpec, etc.
//! - `service_impl` — KanbanService + submodules: comments, goals, kata, service, spawn, types

pub(crate) mod mermaid;
mod service_impl;
pub(crate) mod types;

// Common imports for submodule files that use `use super::*`

// Re-export the public API from types
pub(crate) use types::{
    Board, ColumnDef, Comment, CriterionCitation, KanbanPhase, KataExperimentRecord, Priority,
    SpawnSpec, Task, TaskFilter, TaskKataState, TaskSpec, TaskStatus, Verification,
    VerificationCriterion,
};

// Re-export the service and errors from service_impl
pub use service_impl::KanbanError;
pub use service_impl::KanbanService;
pub(crate) use service_impl::kata::KataStateUpdate;
