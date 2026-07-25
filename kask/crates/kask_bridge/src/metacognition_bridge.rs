//! Bridge between `hkask_regulation::MetacognitionLoop` and the agent's
//! `MetacognitionProvider` trait.
//!
//! The composition root constructs a `MetacognitionLoop` and wraps it in a
//! `BridgeMetacognitionProvider` so the `CuratorStatusTool` can read health
//! snapshots from the agent's tool surface.

use std::sync::Arc;

use gpui::Task;
use hkask_regulation::MetacognitionLoop;
use serde_json::json;

/// Adapter that implements `agent::MetacognitionProvider` over a
/// `MetacognitionLoop`.
pub struct BridgeMetacognitionProvider {
    loop_: Arc<MetacognitionLoop>,
}

impl BridgeMetacognitionProvider {
    pub fn new(loop_: Arc<MetacognitionLoop>) -> Self {
        Self { loop_ }
    }
}

impl agent::MetacognitionProvider for BridgeMetacognitionProvider {
    fn health_snapshot_json(&self) -> Task<Option<serde_json::Value>> {
        let loop_ = self.loop_.clone();
        // The MetacognitionLoop's `last_snapshot()` is async, but it just
        // reads from an RwLock — it's effectively instant. We use
        // `Task::ready` with the result of a blocking read.
        //
        // Since `last_snapshot()` returns a future that completes immediately
        // (it's just an RwLock read), we can use `Task::ready` by first
        // computing the value synchronously.
        let snapshot = loop_.last_snapshot_blocking();
        let result = snapshot.map(|s| {
            json!({
                "timestamp": s.timestamp.to_rfc3339(),
                "variety_deficit": s.variety_deficit,
                "critical_alerts": s.critical_alerts,
                "regulation_effectiveness": s.regulation_effectiveness,
                "healthy": s.ledger_health.healthy,
                "total_cycles": s.regulation_health.total_cycles,
                "accepted": s.regulation_health.accepted,
                "staged": s.regulation_health.staged,
                "blocked": s.regulation_health.blocked,
            })
        });
        Task::ready(result)
    }
}
