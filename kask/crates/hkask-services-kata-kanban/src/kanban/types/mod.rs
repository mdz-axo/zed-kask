//! Kanban types — Agent coordination via headless task boards.
//!
//! Every type carries `owner: WebID` (P12 — anonymous agency prohibition).
//! Task status transitions are column-ordered: Backlog → Ready → InProgress → Review → Done.
//! Verification criteria accept natural-language acceptance specs with optional LLM evaluation prompts.

use chrono::{DateTime, Utc};
use hkask_types::id::{BoardId, ColumnId, CommentId, PhaseId, TaskId, WebID};
use serde::{Deserialize, Serialize};

pub(crate) mod board;
pub(crate) mod column;

pub(crate) mod gas;
pub(crate) mod phase;
pub(crate) mod priority;
pub(crate) mod spawn;
pub(crate) mod status;
pub(crate) mod task;
pub(crate) mod task_spec;
pub(crate) mod tests;
pub(crate) mod verification;

// Re-export all public types
pub use board::Board;
pub use column::ColumnDef;

pub use gas::GasEntry;
pub use phase::KanbanPhase;
pub use priority::Priority;
pub use spawn::SpawnSpec;
pub use status::TaskStatus;
pub use task::{Comment, Task, TaskFilter};
pub use task_spec::TaskSpec;
pub use verification::{Verification, VerificationCriterion};
