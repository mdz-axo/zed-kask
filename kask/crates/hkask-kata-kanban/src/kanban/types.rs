//! Kanban types — Agent coordination via headless task boards.
//!
//! Every type carries `owner: WebID` (P12 — anonymous agency prohibition).
//! Task status transitions are column-ordered: Backlog → Ready → InProgress → Review → Done.
//! Verification criteria accept natural-language acceptance specs with optional LLM evaluation prompts.

use chrono::{DateTime, Utc};
use hkask_types::id::{BoardId, ColumnId, CommentId, GoalID, TaskId, WebID};
use serde::{Deserialize, Serialize};

pub mod board;
pub mod column;
pub mod goal;
pub mod kata;

pub mod priority;
pub mod task;
pub mod task_spec;
pub mod verification;

// Re-export all public types. `TaskStatus` lives in `hkask_types` (the shared
// single source of truth for the widget and the server); re-exported here so
// existing `crate::kanban::TaskStatus` paths keep working.
pub use board::Board;
pub use column::ColumnDef;
pub use goal::CriterionCitation;
pub use kata::{KataExperimentRecord, TaskKataState};

pub use hkask_types::TaskStatus;
pub use priority::Priority;
pub use task::{Comment, Task};
pub use task_spec::TaskSpec;
pub use verification::{Verification, VerificationCriterion};
