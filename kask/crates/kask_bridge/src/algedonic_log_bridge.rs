//! Bridge between the agent's `CuratorClearAlgedonicLogTool` and the
//! `RegulationLedger` (which holds the `AlgedonicManager`).
//!
//! `BridgeAlgedonicLogSink` holds the shared `Arc<RwLock<RegulationLedger>>`
//! (the same handle the `MetacognitionLoop` and `CyberneticsLoop` use) and
//! exposes the async `AlgedonicLogSink` trait so the
//! `CuratorClearAlgedonicLogTool` can clear reviewed alerts and query the
//! log cap status from the agent's tool surface.
//!
//! Lesson 2 resolution: the sink holds the shared `Arc`, not a cloned
//! snapshot. A previous version cloned the `RegulationLedger` out of the
//! `Arc<RwLock<...>>` via `try_read()`, with a fallback that created a
//! fresh, disconnected `RegulationLedger` when the lock was contended.
//! That fallback was a broken feedback loop — clearing alerts on a
//! throwaway ledger while the real one kept accumulating. Holding the
//! `Arc` directly (like `MetacognitionLoop` does) eliminates the race
//! and the fallback.

use std::sync::Arc;

use agent::AlgedonicLogSink;
use async_trait::async_trait;
use hkask_regulation::RegulationLedger;
use tokio::sync::RwLock;

/// Bridge: `CuratorClearAlgedonicLogTool` → `RegulationLedger`.
///
/// Holds the shared `Arc<RwLock<RegulationLedger>>` — the same handle the
/// `MetacognitionLoop` and `CyberneticsLoop` use. The sink methods acquire
/// the read or write lock on the tokio runtime, so they are async.
pub struct BridgeAlgedonicLogSink {
    ledger: Arc<RwLock<RegulationLedger>>,
}

impl BridgeAlgedonicLogSink {
    pub fn new(ledger: Arc<RwLock<RegulationLedger>>) -> Self {
        Self { ledger }
    }
}

#[async_trait]
impl AlgedonicLogSink for BridgeAlgedonicLogSink {
    async fn clear_reviewed_alerts(&self) -> Result<usize, String> {
        let ledger = self.ledger.read().await;
        let count_before = ledger.alert_log_count().await;
        ledger.clear_reviewed_alerts().await;
        let count_after = ledger.alert_log_count().await;
        let cleared = count_before.saturating_sub(count_after);
        tracing::info!(
            target: "hkask.algedonic",
            cleared = cleared,
            remaining = count_after,
            "Algedonic log: cleared reviewed alerts"
        );
        Ok(cleared)
    }

    async fn clear_all_alerts(&self) -> Result<usize, String> {
        let ledger = self.ledger.read().await;
        let count_before = ledger.alert_log_count().await;
        ledger.clear_all_alerts().await;
        tracing::warn!(
            target: "hkask.algedonic",
            cleared = count_before,
            "Algedonic log: cleared ALL alerts (including unresolved Critical)"
        );
        Ok(count_before)
    }

    async fn alert_log_count(&self) -> Option<usize> {
        let ledger = self.ledger.read().await;
        Some(ledger.alert_log_count().await)
    }

    async fn alert_log_approaching_cap(&self) -> Option<bool> {
        let ledger = self.ledger.read().await;
        Some(ledger.alert_log_approaching_cap().await)
    }
}
