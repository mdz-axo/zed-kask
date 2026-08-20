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

pub(crate) mod phase;
pub(crate) mod priority;
pub(crate) mod spawn;
pub(crate) mod spend;
pub(crate) mod task;
pub(crate) mod task_spec;
pub(crate) mod verification;

// Re-export all public types. `TaskStatus` lives in `hkask_types` (the shared
// single source of truth for the widget and the server); re-exported here so
// existing `crate::kanban::TaskStatus` paths keep working.
pub use board::Board;
pub use column::ColumnDef;

pub use hkask_types::TaskStatus;
pub use phase::KanbanPhase;
pub use priority::Priority;
pub use spawn::SpawnSpec;
pub use spend::SpendEntry;
pub use task::{Comment, Task, TaskFilter};
pub use task_spec::TaskSpec;
pub use verification::{Verification, VerificationCriterion};
