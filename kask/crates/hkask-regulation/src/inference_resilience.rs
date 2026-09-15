//! Typed observation seam for inference resilience.
//!
//! The Zed-facing bridge owns enforcement. Regulation consumes coherent
//! snapshots and intervention receipts through this Zed-free contract.

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceCircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceInterventionKind {
    CircuitOpened,
    CircuitHalfOpened,
    CircuitClosed,
    CircuitReopened,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InferenceInterventionReceipt {
    pub id: u64,
    pub kind: InferenceInterventionKind,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferencePermanentFailureKind {
    Authorization,
    Configuration,
    Model,
    Provider,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InferencePermanentFailureReceipt {
    pub id: u64,
    pub kind: InferencePermanentFailureKind,
    pub detail: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InferenceSnapshot {
    pub observed_at: DateTime<Utc>,
    pub in_flight: usize,
    pub max_concurrency: usize,
    pub recent_timeout_count: u64,
    pub circuit_state: InferenceCircuitState,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InferenceObservation {
    pub snapshot: InferenceSnapshot,
    pub interventions: Vec<InferenceInterventionReceipt>,
    pub permanent_failures: Vec<InferencePermanentFailureReceipt>,
    pub next_cursor: u64,
}

#[derive(Debug, thiserror::Error)]
#[error("Inference resilience observation failed: {0}")]
pub struct InferenceObservationError(pub String);

#[async_trait::async_trait]
pub trait InferenceResilienceSource: Send + Sync {
    /// expect: "Regulation can observe inference interventions and their later state coherently"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: `cursor` is zero or a cursor returned by an earlier observation
    /// post: the snapshot is one coherent reading and receipts have ids greater than `cursor`
    async fn observe_since(
        &self,
        cursor: u64,
    ) -> Result<InferenceObservation, InferenceObservationError>;
}
