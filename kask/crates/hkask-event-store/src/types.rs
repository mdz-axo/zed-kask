//! Event store types — the record, filter, and error shapes.
//!
//! # Canonical model
//!
//! The provenance and classification types follow **Agent Lightning**'
//! (Microsoft, 2025) `schemas.py` data model, combined with **Event
//! Sourcing** (DDD, Evans 2003; Fowler, "Event Sourcing", 2005):
//!
//! - **`VerdictSource`** ≅ AL's `RewardData.source` — *who produced the
//!   judgment and by what mechanism*. Determines trust level: only a
//!   deterministic evaluator is trusted for the C0 `s` axis (the
//!   determinism constraint); an LLM-judged verdict is downgraded to a
//!   hypothesis. This is the single provenance type for all verdicts in
//!   the system — the runtime-layer `TaskSuccessProvenance` was merged
//!   into it to eliminate the parallel-but-never-connected duplication.
//!
//! - **`RolloutKind`** ≅ AL's `Rollout.kind` — *what type of execution
//!   unit produced these events*. A rollout is the unit that has a
//!   lifecycle and a judge: a delegation (one `swarm_delegate_local`
//!   call), a turn (a curator/user interaction), or a harness run (one
//!   `swarm_eval_agent_local` call). Carried in event payloads so
//!   consumers can classify without a separate rollouts table.
//!
//! Both types serialize to snake_case strings in event payloads — the
//! store does not parse payloads, so the types are carried as
//! self-describing JSON, not as typed columns.

use serde::{Deserialize, Serialize};

/// Who produced a verdict and by what mechanism. The single provenance
/// type for all verdicts in the system (event-store layer + runtime layer).
///
/// # Trust levels
///
/// | Variant | Trusted for task success? | Rationale |
/// |---|---|---|
/// | `DeterministicEvaluator` | Yes | Deterministic check (contains/regex/exit_code/file_exists). The only automated source trusted for the C0 `s` axis. |
/// | `Operator` | Yes | Human ground truth (the operator or Curator stamped it). |
/// | `LlmJudged` | No | An LLM judged the response. ORIENT must downgrade to a hypothesis — the determinism constraint forbids an LLM judging `task_success`. |
/// | `RegulationImpact` | No (for task success) | The cybernetics loop's `verify_impact` produced this — a before/after measurement, not a task-success check. Trusted for regulation, not for `s`. |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerdictSource {
    /// A deterministic evaluator (contains/not_contains/regex/exit_code/
    /// file_exists) run against the response. The only automated source
    /// trusted for the C0 `s` axis of the swarm-state distance.
    DeterministicEvaluator,
    /// A human (the operator or Curator) stamped the verdict. Ground truth.
    Operator,
    /// An LLM judged the response. Untrusted per the determinism
    /// constraint — ORIENT must downgrade to a hypothesis, not a trusted
    /// `s`.
    LlmJudged,
    /// The cybernetics loop's `verify_impact` produced this verdict — the
    /// regulation system's before/after measurement, not a task-success
    /// check. Trusted for regulation impact analysis, not for `s`.
    RegulationImpact,
}

impl VerdictSource {
    /// Stable wire string for event payloads. Matches the serde
    /// snake_case representation so the payload is self-describing JSON
    /// that round-trips through `VerdictSource::from_str`.
    pub fn as_str(&self) -> &'static str {
        match self {
            VerdictSource::DeterministicEvaluator => "deterministic_evaluator",
            VerdictSource::Operator => "operator",
            VerdictSource::LlmJudged => "llm_judged",
            VerdictSource::RegulationImpact => "regulation_impact",
        }
    }

    /// Parse a wire string back to `VerdictSource`. Returns `None` for
    /// unknown strings — absence, not a fabricated default (the `.rules`
    /// trap: a missing source is not a deterministic evaluator).
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "deterministic_evaluator" => Some(VerdictSource::DeterministicEvaluator),
            "operator" => Some(VerdictSource::Operator),
            "llm_judged" => Some(VerdictSource::LlmJudged),
            "regulation_impact" => Some(VerdictSource::RegulationImpact),
            _ => None,
        }
    }

    /// Whether this source is trusted for task-success determination
    /// (the C0 `s` axis). `DeterministicEvaluator` and `Operator` are
    /// trusted; `LlmJudged` and `RegulationImpact` are not.
    pub fn is_trusted_for_task_success(&self) -> bool {
        matches!(
            self,
            VerdictSource::DeterministicEvaluator | VerdictSource::Operator
        )
    }
}

/// The coarse rollout classification. A rollout is the unit that has a
/// lifecycle and a judge — a local swarm delegation, a curator/user turn,
/// or a harness run. Carried in event payloads so consumers can classify
/// a rollout without a separate rollouts table.
///
/// Canonical reference: Agent Lightning's `Rollout.kind` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutKind {
    /// A single `swarm_delegate_local` call — one agent executing one
    /// task through the inference + tool loop. Produces `model_request`
    /// and `verdict` events.
    Delegation,
    /// A curator or user interaction turn. Not yet produced by any code
    /// path (reserved for the regulation consumer's curator-turn rollout
    /// concept).
    Turn,
    /// A `swarm_eval_agent_local` call — one agent × one task set × N
    /// repeats. Produces `harness_summary` events alongside the per-
    /// delegation `model_request` and `verdict` events.
    HarnessRun,
}

impl RolloutKind {
    /// Stable wire string for event payloads.
    pub fn as_str(&self) -> &'static str {
        match self {
            RolloutKind::Delegation => "delegation",
            RolloutKind::Turn => "turn",
            RolloutKind::HarnessRun => "harness_run",
        }
    }

    /// Parse a wire string back to `RolloutKind`. Returns `None` for
    /// unknown strings.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "delegation" => Some(RolloutKind::Delegation),
            "turn" => Some(RolloutKind::Turn),
            "harness_run" => Some(RolloutKind::HarnessRun),
            _ => None,
        }
    }
}

/// One event in the log. `position` is the identity — there is no separate
/// event ID (Agent Lightning's `schemas.py` pattern).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct EventRecord {
    /// Position in the log. Monotonic; assigned by `append`.
    pub position: i64,
    /// The rollout this event belongs to. Caller-assigned; groups events
    /// into the unit that has a lifecycle and a judge.
    pub rollout_id: String,
    /// The event kind. Two well-known kinds (`model_request`, `verdict`);
    /// everything else is opaque pass-through.
    pub kind: String,
    /// The event payload, stored as JSON. The store does not parse it.
    /// Verdict events carry `source` (a `VerdictSource` wire string) and
    /// `rollout_kind` (a `RolloutKind` wire string) so consumers can
    /// classify without the store parsing payloads.
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
pub(crate) enum EventStoreError {
    #[error("database error: {0}")]
    Database(#[from] hkask_storage::database::types::DbError),
    #[error("stored payload was not valid JSON: {0}")]
    PayloadParse(#[from] serde_json::Error),
    #[error("rollout id must be non-empty")]
    EmptyRolloutId,
    #[error("event kind must be non-empty")]
    EmptyKind,
    #[error("append succeeded but no position was returned")]
    NoPosition,
}
