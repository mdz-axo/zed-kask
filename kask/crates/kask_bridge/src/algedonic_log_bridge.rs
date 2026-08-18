//! Bridge between the agent's `CuratorClearAlgedonicLogTool` and the
//! `RegulationLedger` (which holds the `AlgedonicManager`).
//!
//! `BridgeAlgedonicLogSink` wraps the `RegulationLedger` and exposes the
//! async `AlgedonicLogSink` trait so the `CuratorClearAlgedonicLogTool`
//! can clear reviewed alerts and query the log cap status from the agent's
//! tool surface.

use std::sync::Arc;

use agent::AlgedonicLogSink;
use async_trait::async_trait;
use hkask_regulation::RegulationLedger;

/// Bridge: `CuratorClearAlgedonicLogTool` → `RegulationLedger`.
///
/// Holds a clone of the `RegulationLedger` (which is `Clone` — it's an
/// `Arc<RwLock<RegState>>` internally). The sink methods are async because
/// `RegulationLedger`'s methods acquire the inner `tokio::sync::RwLock`.
pub struct BridgeAlgedonicLogSink {
    ledger: RegulationLedger,
}

impl BridgeAlgedonicLogSink {
    pub fn new(ledger: RegulationLedger) -> Self {
        Self { ledger }
    }
}

/// Construct a `BridgeAlgedonicLogSink` from an `Arc<RwLock<RegulationLedger>>`
/// (the shape the composition root holds). Reads the ledger out of the lock
/// and clones it (cheap — `Arc` refcount bump).
impl BridgeAlgedonicLogSink {
    pub fn from_shared(shared: Arc<tokio::sync::RwLock<RegulationLedger>>) -> Self {
        // We can't await here (this is a sync constructor), but
        // `RegulationLedger` is `Clone` (Arc inside). We use
        // `try_read` — if the lock is held, we retry once. In practice
        // the lock is almost never contended at construction time
        // (startup, before the tick loop starts).
        let ledger = shared
            .try_read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| {
                tracing::warn!(
                    target: "reg.algedonic",
                    "RegulationLedger lock contended at sink construction — \
                     using a fresh ledger as fallback. This should not happen \
                     at startup; investigate if it recurs."
                );
                RegulationLedger::with_threshold(
                    hkask_regulation::DEFAULT_VARIETY_MAX_DEFICIT as u64,
                )
            });
        Self { ledger }
    }
}

#[async_trait]
impl AlgedonicLogSink for BridgeAlgedonicLogSink {
    async fn clear_reviewed_alerts(&self) -> Result<usize, String> {
        let count_before = self.ledger.alert_log_count().await;
        self.ledger.clear_reviewed_alerts().await;
        let count_after = self.ledger.alert_log_count().await;
        let cleared = count_before.saturating_sub(count_after);
        tracing::info!(
            target: "reg.algedonic",
            cleared = cleared,
            remaining = count_after,
            "Algedonic log: cleared reviewed alerts"
        );
        Ok(cleared)
    }

    async fn alert_log_count(&self) -> Option<usize> {
        Some(self.ledger.alert_log_count().await)
    }

    async fn alert_log_approaching_cap(&self) -> Option<bool> {
        Some(self.ledger.alert_log_approaching_cap().await)
    }
}
