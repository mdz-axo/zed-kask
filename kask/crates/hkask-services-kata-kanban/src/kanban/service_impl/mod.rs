//! KanbanService — core kanban board and task coordination.
//!
//! Implements kanban board and task coordination operations.
//! Every operation carries ownership tracking (P12) and enforces agent consent
//! on assignment (P1). State transitions are column-ordered.
//!
//! Persistence: boards and tasks stored as RDF h_mems via HMemStore (MDS §2).
//!
//! ## Module Structure
//!
//! - `service` — `KanbanService` struct and all coordination methods
//! - `types` — `KanbanError`, `UnjamItem`, `UnjamFix`
//! - `comments` — Task-level comment threads
//! - `decompose` — Task decomposition operations
//! - `dejam` — Stuck-task detection and auto-fix
//! - `kata` — Kata cycle execution on tasks
//! - `phases` — Board phase management
//! - `spawn` — Agent spawn from task specs
//! - `verification` — LLM-based task verification

// Imports needed by child submodules via `use super::*`
#[allow(unused_imports)]
use crate::kanban::{
    Comment, GasEntry, KanbanPhase, Priority, SpawnSpec, Task, TaskFilter, TaskSpec, TaskStatus,
    Verification, VerificationCriterion,
};
use hkask_types::NotFound;
use hkask_types::WebID;
use hkask_types::id::{BoardId, PhaseId, TaskId};

pub(crate) mod comments;
pub(crate) mod decompose;
pub(crate) mod dejam;
pub(crate) mod kata;
pub(crate) mod phases;
mod service;
pub(crate) mod spawn;
#[cfg(test)]
mod tests;
mod types;
pub(crate) mod verification;

pub use service::KanbanService;
pub use types::{KanbanError, UnjamFix, UnjamItem};
