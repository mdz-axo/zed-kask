use crate::InfrastructureError;
use crate::event::{RegulationRecord, SpanNamespace};
use crate::id::WebID;
use crate::loops::LoopId;
use chrono::{DateTime, Utc};

use async_trait::async_trait;

/// Parameters for consolidation. All fields except `limit` optional.
#[derive(Debug, Clone)]
pub struct ConsolidationRequest {
    pub limit: usize,
    pub confidence_floor: Option<f64>,
    pub max_semantic_triples: Option<usize>,
}

impl Default for ConsolidationRequest {
    fn default() -> Self {
        Self {
            limit: 100,
            confidence_floor: None,
            max_semantic_triples: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConsolidationOutcome {
    pub consolidated_count: usize,
    pub deleted_count: usize,
    pub failed_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DepletionSignal {
    pub agent: WebID,
    pub remaining: u64,
    pub cap: u64,
    pub usage_ratio: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackpressureSignal {
    pub source: LoopId,
    pub reason: String,
    pub severity: f64,
}

/// Subscribes to Regulation events by span namespace.

#[async_trait]
pub trait LedgerObserver: Send + Sync {
    fn interest_mask(&self) -> Vec<SpanNamespace>;

    async fn on_event(&self, event: &RegulationRecord);

    async fn on_depletion(&self, signal: &DepletionSignal);

    async fn on_backpressure(&self, signal: &BackpressureSignal);
}

/// Storage port for Regulation event queries.
///
/// Abstracts the RegulationArchive behind a trait so the cybernetic regulation
/// layer (GasBudgetManager, WalletManager) can be tested without a real SQLite
/// database.
///
/// Concrete impl: `RegulationArchive` in `hkask-storage`.
pub trait LedgerStoragePort: Send + Sync {
    fn query_algedonic(
        &self,
        since: DateTime<Utc>,
        limit: u64,
    ) -> Result<Vec<RegulationRecord>, InfrastructureError>;

    /// Replay events with temporal decay weighting. Events older than
    /// the lookback window or below the weight threshold are excluded.
    fn replay_weighted(
        &self,
        since: DateTime<Utc>,
        limit: u64,
        config: &DecayConfig,
    ) -> Result<Vec<WeightedEvent>, InfrastructureError>;

    /// Persist a loop cursor key-value pair for crash recovery.
    fn persist_cursor(&self, key: &str, value: i64) -> Result<(), InfrastructureError>;

    /// Load a persisted loop cursor. Returns `None` if no cursor exists.
    fn load_cursor(&self, key: &str) -> Result<Option<i64>, InfrastructureError>;

    /// Query events by span_category prefix.
    ///
    /// The `namespace_prefix` is the short-name prefix stored in the `span_category`
    /// column (e.g., "guard" matches "guard.input", "guard.output", etc.).
    /// Pass the short name — NOT the full `reg.*` namespace.
    ///
    /// \[P9\] Motivating: Homeostatic Self-Regulation — query Regulation span history
    /// pre:  `namespace_prefix` is a non-empty short-name prefix
    /// post: returns Vec of RegulationRecords with span_category starting with the prefix,
    ///       since the given timestamp, ordered by timestamp ASC, limited to `limit`
    fn query_by_namespace(
        &self,
        namespace_prefix: &str,
        since: DateTime<Utc>,
        limit: u64,
    ) -> Result<Vec<RegulationRecord>, InfrastructureError>;

    /// Count events by span_category, grouped by exact category.
    ///
    /// The `namespace_prefix` is the short-name prefix stored in the `span_category`
    /// column.
    ///
    /// \[P9\] Motivating: Homeostatic Self-Regulation — aggregate Regulation span stats
    /// pre:  `namespace_prefix` is a non-empty short-name prefix
    /// post: returns Vec of (span_category, count) tuples, ordered by count DESC
    fn query_span_stats(
        &self,
        namespace_prefix: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<(String, u64)>, InfrastructureError>;
}

/// A RegulationRecord with its computed replay weight.
#[derive(Debug, Clone)]
pub struct WeightedEvent {
    pub event: RegulationRecord,
    pub weight: f64,
}

/// Per-domain decay constants for weighted replay.
///
/// Each loop domain has its own lambda for exponential decay.
/// Half-life = ln(2)/lambda.
#[derive(Debug, Clone)]
pub struct DecayConfig {
    pub cybernetics_lambda: f64,
    pub curation_lambda: f64,
    pub inference_lambda: f64,
    pub episodic_lambda: f64,
    pub weight_threshold: f64,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            cybernetics_lambda: std::f64::consts::LN_2 / 300.0,
            curation_lambda: std::f64::consts::LN_2 / 900.0,
            inference_lambda: std::f64::consts::LN_2 / 120.0,
            episodic_lambda: std::f64::consts::LN_2 / 600.0,
            weight_threshold: 0.001,
        }
    }
}
