//! Rollout event source bridge (event-substrate phase 6).
//!
//! Implements `hkask_regulation::RolloutEventSource` over the swarm event
//! store (`mcp/swarm/events.db`, the same store the rollout harness writes).
//! Wired into the `CyberneticsLoop` at startup so `verify_impact` can answer
//! "for rollout R, what was the metric before action A and after it?" as a
//! store query.
//!
//! The bridge reads `model_request` events for the rollout and derives the
//! metric values from their payloads. Metrics not derivable from captured
//! events return `None` — absence, not a fabricated zero (the `.rules`
//! broken-feedback-loop trap).

use hkask_regulation::RolloutEventSource;
use hkask_storage::database::driver::DatabaseDriver;
use std::sync::Arc;

/// A `RolloutEventSource` over the swarm event store.
pub struct BridgeRolloutEventSource {
    store: Arc<hkask_event_store::EventStore>,
}

impl BridgeRolloutEventSource {
    /// Open the store at the swarm events path. `Err` when the database
    /// cannot be opened — the caller (startup wiring) logs and continues
    /// unwired rather than failing startup (degraded, not broken).
    pub fn open(events_path: &str) -> Result<Self, String> {
        if let Some(parent) = std::path::Path::new(events_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create event store dir: {e}"))?;
        }
        let manager = r2d2_sqlite::SqliteConnectionManager::file(events_path)
            .with_init(|conn| conn.execute_batch(hkask_storage::WAL_PRAGMA_BATCH));
        let pool = r2d2::Pool::builder()
            .max_size(2)
            .build(manager)
            .map_err(|e| format!("failed to create event store pool: {e}"))?;
        let driver: Arc<dyn DatabaseDriver> = Arc::new(hkask_storage::SqliteDriver::new(pool));
        let store = hkask_event_store::EventStore::from_driver(driver)
            .map_err(|e| format!("failed to init event store: {e}"))?;
        Ok(Self {
            store: Arc::new(store),
        })
    }
}

impl RolloutEventSource for BridgeRolloutEventSource {
    fn metric_before_and_after(
        &self,
        rollout_id: &str,
        metric: &str,
        before_position: i64,
    ) -> Result<Option<(f64, f64)>, String> {
        let events = self
            .store
            .query(&hkask_event_store::EventFilter {
                rollout: Some(rollout_id.to_string()),
                ..hkask_event_store::EventFilter::default()
            })
            .map_err(|e| format!("event store query failed: {e}"))?;
        if events.is_empty() {
            // No events for this rollout — absence, not zero.
            return Ok(None);
        }
        // Derive the metric value from each event's payload. The two metrics
        // the capture path records today: latency (model_request.latency_ms)
        // and token usage (model_request.usage.total_tokens). Anything else
        // has no captured source — None, never fabricated.
        let value_of = |payload: &serde_json::Value| -> Option<f64> {
            match metric {
                "connector_latency" => payload.get("latency_ms").and_then(|v| v.as_f64()),
                "energy_remaining" => payload
                    .get("usage")
                    .and_then(|usage| usage.get("total_tokens"))
                    .and_then(|v| v.as_f64()),
                _ => None,
            }
        };
        let before = events
            .iter()
            .filter(|event| event.position <= before_position)
            .find_map(|event| value_of(&event.payload));
        let after = events
            .iter()
            .filter(|event| event.position > before_position)
            .last()
            .and_then(|event| value_of(&event.payload));
        match (before, after) {
            (Some(before), Some(after)) => Ok(Some((before, after))),
            _ => Ok(None),
        }
    }
}
