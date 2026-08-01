use crate::event::{RegulationRecord, SpanNamespace};
use crate::id::WebID;
use crate::loops::LoopId;

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
