//! Event store types — the record, filter, and error shapes.

use serde::{Deserialize, Serialize};

/// How a verdict was produced. The determinism constraint mirrors
/// `TaskSuccessProvenance`: only a deterministic evaluator is trusted as a
/// ground-truth label; operator and regulation-impact verdicts carry their
/// own provenance so consumers can weight them accordingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictSource {
    DeterministicEvaluator,
    Operator,
    RegulationImpact,
}

/// The coarse rollout classification. A rollout is the unit that has a
/// lifecycle and a judge — a local swarm delegation, a curator/user turn,
/// or a harness run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutKind {
    Delegation,
    Turn,
    HarnessRun,
}

/// One event in the log. `position` is the identity — there is no separate
/// event ID (Agent Lightning's `schemas.py` pattern).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    /// Position in the log. Monotonic; assigned by `append`.
    pub position: i64,
    /// The rollout this event belongs to. Caller-assigned; groups events
    /// into the unit that has a lifecycle and a judge.
    pub rollout_id: String,
    /// The event kind. Two well-known kinds (`model_request`, `verdict`);
    /// everything else is opaque pass-through.
    pub kind: String,
    /// The event payload, stored as JSON. The store does not parse it.
    pub payload: serde_json::Value,
    /// RFC3339 timestamp assigned by `append`.
    pub created_at: String,
}

/// Query filter. `None` fields match everything; set fields AND together.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventFilter {
    /// Match events for one rollout.
    pub rollout: Option<String>,
    /// Match one event kind.
    pub kind: Option<String>,
    /// Match events strictly after this position (incremental reading).
    pub after_position: Option<i64>,
    /// Cap the number of results.
    pub limit: Option<usize>,
}

/// Event store errors.
#[derive(Debug, thiserror::Error)]
pub enum EventStoreError {
    #[error("database error: {0}")]
    Database(#[from] hkask_storage::database::types::DbError),
    #[error("rollout id must be non-empty")]
    EmptyRolloutId,
    #[error("event kind must be non-empty")]
    EmptyKind,
    #[error("append succeeded but no position was returned")]
    NoPosition,
}
