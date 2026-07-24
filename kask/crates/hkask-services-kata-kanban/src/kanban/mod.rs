//! Kanban board mechanics — agent coordination via headless task boards.
//!
//! Every type carries `owner: WebID` (P12 — anonymous agency prohibition).
//! Task status transitions are column-ordered: Backlog → Ready → InProgress → Review → Done.
//!
//! ## Module Structure
//!
//! - `types` — Core types: Board, Task, TaskSpec, TaskStatus, Priority, SpawnSpec, etc.
//! - `service_impl` — KanbanService + submodules: comments, decompose, dejam, kata, phases, spawn, verification
//! - `socratic` — Socratic inquiry cycle for task diagnosis

mod service_impl;
pub mod socratic;
pub mod types;

// Common imports for submodule files that use `use super::*`

// Re-export the public API from types
pub use types::{
    Board, ColumnDef, Comment, GasEntry, KanbanPhase, Priority, SpawnSpec, Task, TaskFilter,
    TaskSpec, TaskStatus, Verification, VerificationCriterion,
};

// Re-export the service and errors from service_impl
pub use service_impl::{KanbanError, KanbanService, UnjamFix, UnjamItem};

// Re-export socratic types
pub use socratic::{QualityGate, SocraticRole};
