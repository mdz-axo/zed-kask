//! TrustSensor — Binding 2 (trust-internalization program).
//!
//! Senses the trust ledger's window stats and emits a `TrustDeviation`
//! signal when the weighted violation count exceeds zero. The trust
//! ledger lives in BOTH the curator's MemoryStore AND the kata-kanban's
//! HMemStore (Binding 5's substrate — trust events ARE h_mems); this
//! sensor reads from every store it's given (tight coupling: the
//! regulation loop reads directly from the kata-kanban's database too,
//! aggregating trust events from both sources into one signal).
//!
//! D/P: the counts are D (mechanically computed from the ledger's
//! h_mems); the set-point (0.0) and the deviation threshold are P
//! (operator-set, like every other metric's bands). The outage
//! discipline: any store unavailable → `observe()` returns `None`
//! → the loop reads sensor-unavailable, never zero.

use crate::loops::core::LoopId;
use crate::loops::signals::{Signal, SignalMetric};
use crate::sensor_provider::Sensor;
use chrono::{DateTime, Utc};
use hkask_memory::MemoryStore;
use std::sync::Arc;

/// Senses the trust ledger: weighted violation count (the deviation
/// metric) plus the unverified-explicit count (visible, never silent).
///
/// The set-point is 0.0 — any positive weighted-violation count is a
/// deviation. The sensor reads from every `MemoryStore` it holds
/// (tight coupling: both the curator's and the kata-kanban's),
/// aggregating across all stores.
pub(crate) struct TrustSensor {
    /// All stores that carry trust events (tight coupling: both the
    /// curator's MemoryStore and the kata-kanban's, if wired). Each
    /// store is read independently; the counts aggregate.
    stores: Vec<Arc<MemoryStore>>,
    /// The lookback window (hours) for the trust account.
    window_hours: i64,
}

impl TrustSensor {
    pub fn new(stores: Vec<Arc<MemoryStore>>, window_hours: i64) -> Self {
        Self {
            stores,
            window_hours,
        }
    }

    /// Read the trust ledger's window stats from every store.
    ///
    /// Returns `None` when any store is unavailable (the `Sensor`
    /// contract: "None is not evidence of recovery" — a store outage
    /// is a broken feedback loop, not zero events).
    fn trust_window_stats(&self, since: DateTime<Utc>) -> Option<TrustWindowStats> {
        let mut stats = TrustWindowStats::default();
        for memory in &self.stores {
            let h_mems = memory
                .query_deduped_untouched(crate::TRUST_LEDGER_ENTITY)
                .ok()?;

            for h in &h_mems {
                // Filter by the event's ts (inside the value), not the
                // h_mem's observed_at.
                let ts = h
                    .value
                    .get("ts")
                    .and_then(|t| t.as_str())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())?;
                if ts.with_timezone(&Utc) < since {
                    continue;
                }

                let weight = h.value.get("weight").and_then(|w| w.as_i64()).unwrap_or(3);
                let outcome = h
                    .value
                    .get("outcome")
                    .and_then(|o| o.as_str())
                    .unwrap_or("unverified");

                match outcome {
                    "violated" => stats.weighted_violation_count += weight,
                    "unverified" if weight == 3 => stats.unverified_explicit_count += 1,
                    _ => {}
                }
            }
        }
        Some(stats)
    }
}

/// Trust window stats (the D-computed counts the sensor reads).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TrustWindowStats {
    /// The sum of weights of violated instructions in the window.
    /// The deviation metric — set-point 0.0.
    pub weighted_violation_count: i64,
    /// The count of unverified explicit instructions (weight 3) —
    /// visible, never silent, but not a deviation by itself.
    pub unverified_explicit_count: i64,
}

#[async_trait::async_trait]
impl Sensor for TrustSensor {
    async fn observe(&self) -> Option<Signal> {
        let since = Utc::now() - chrono::Duration::hours(self.window_hours);
        let stats = self.trust_window_stats(since)?;

        // The deviation value: weighted violation count. The set-point
        // is 0.0 — any positive count is a deviation.
        let value = stats.weighted_violation_count as f64;

        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::TrustDeviation,
            value,
            0.0, // set-point: any positive weighted violation is a deviation
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_memory_store() -> Arc<MemoryStore> {
        let driver = hkask_storage::SqliteDriver::in_memory_driver();
        let h_mem_store =
            hkask_storage::HMemStore::from_driver(driver.clone()).expect("in-memory h_mem store");
        let embedding_store = hkask_storage::EmbeddingStore::from_driver(driver, 0)
            .expect("in-memory embedding store");
        Arc::new(MemoryStore::new(h_mem_store, embedding_store))
    }

    fn trust_h_mem(
        entity_attr: &str,
        weight: i64,
        outcome: &str,
        ts_hours_ago: i64,
    ) -> hkask_storage::HMem {
        hkask_storage::HMem::new(
            crate::TRUST_LEDGER_ENTITY,
            entity_attr,
            serde_json::json!({
                "text": "test instruction",
                "source": "test",
                "weight": weight,
                "outcome": outcome,
                "ts": (chrono::Utc::now() - chrono::Duration::hours(ts_hours_ago)).to_rfc3339(),
                "oracle_note": null,
            }),
            hkask_types::WebID::new(),
        )
    }

    /// A store with no trust events → the sensor reads 0.0 (healthy,
    /// not unavailable).
    #[tokio::test]
    async fn empty_ledger_reads_zero() {
        let memory = test_memory_store();
        let sensor = TrustSensor::new(vec![memory], 24);
        let signal = sensor.observe().await;
        assert!(signal.is_some());
        assert_eq!(signal.unwrap().value, 0.0);
    }

    /// A violated explicit instruction → the sensor reads weight 3
    /// (the deviation metric).
    #[tokio::test]
    async fn violated_explicit_reads_three() {
        let memory = test_memory_store();
        memory
            .store(trust_h_mem("test-event-1", 3, "violated", 0))
            .expect("store");

        let sensor = TrustSensor::new(vec![memory], 24);
        let signal = sensor.observe().await.expect("signal");
        assert_eq!(signal.value, 3.0);
    }

    /// Unverified explicit instructions are visible but not a deviation
    /// (the sensor still reads 0.0).
    #[tokio::test]
    async fn unverified_explicit_is_not_a_deviation() {
        let memory = test_memory_store();
        memory
            .store(trust_h_mem("test-event-2", 3, "unverified", 0))
            .expect("store");

        let sensor = TrustSensor::new(vec![memory], 24);
        let signal = sensor.observe().await.expect("signal");
        assert_eq!(signal.value, 0.0);
    }

    /// Events outside the window are not counted.
    #[tokio::test]
    async fn old_events_age_out() {
        let memory = test_memory_store();
        memory
            .store(trust_h_mem("test-event-3", 3, "violated", 48))
            .expect("store");

        // Window is 24 hours — the 48-hour-old event is outside.
        let sensor = TrustSensor::new(vec![memory], 24);
        let signal = sensor.observe().await.expect("signal");
        assert_eq!(signal.value, 0.0);
    }

    /// Tight coupling: the sensor aggregates trust events from
    /// multiple stores (the curator's AND the kata-kanban's). A
    /// violation in the kata-kanban's store is visible to the
    /// regulation loop.
    #[tokio::test]
    async fn multi_store_aggregates_both_stores() {
        let curator_store = test_memory_store();
        let kanban_store = test_memory_store();

        curator_store
            .store(trust_h_mem("curator-event-1", 2, "violated", 0))
            .expect("store");
        kanban_store
            .store(trust_h_mem("kanban-event-1", 3, "violated", 0))
            .expect("store");

        let sensor = TrustSensor::new(vec![curator_store, kanban_store], 24);
        let signal = sensor.observe().await.expect("signal");
        assert_eq!(
            signal.value, 5.0,
            "the weighted violation count aggregates across both stores: 2 + 3"
        );
    }
}
