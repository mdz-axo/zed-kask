//! TrustSensor — Binding 2 (trust-internalization program).
//!
//! Senses the trust ledger's window and emits a `TrustDeviation`
//! signal when the weighted violation count exceeds zero. The trust
//! ledger lives in BOTH the curator's MemoryStore AND the kata-kanban's
//! own DB (Binding 5's substrate — trust events ARE h_mems); this
//! sensor reads from every store it's given (tight coupling: the
//! regulation loop reads directly from the kata-kanban's database too,
//! aggregating trust events from both sources into one signal).
//!
//! The window's rows decode through the ONE wire home
//! (`hkask_trust::wire`) — the same decode the stores and the bridge's
//! injected trust state use, so the readers cannot drift. A row with an
//! unparseable ts is skipped by the shared window rule (row-level
//! garbage), never read as a store outage.
//!
//! D/P: the counts are D (mechanically computed from the ledger's rows
//! through the shared decode); the set-point (0.0) and the deviation
//! threshold are P (operator-set, like every other metric's bands).
//! The outage discipline: any store unavailable → `observe()` returns
//! `None` → the loop reads sensor-unavailable, never zero.

use crate::loops::core::LoopId;
use crate::loops::signals::{Signal, SignalMetric};
use crate::sensor_provider::Sensor;
use chrono::{DateTime, Utc};
use hkask_memory::MemoryStore;
use hkask_trust::wire;
use hkask_trust::{LedgerEvent, Outcome, Weight};
use std::sync::Arc;

/// Senses the trust ledger: weighted violation count (the deviation
/// metric) plus the unverified-explicit count and the ΔT direction —
/// Binding 2's three observables.
///
/// The set-point is 0.0 — any positive weighted-violation count is a
/// deviation. The sensor reads from every `MemoryStore` it holds
/// (tight coupling: both the curator's and the kata-kanban's), reading
/// the window's events through the shared wire decode.
pub(crate) struct TrustSensor {
    /// All stores that carry trust events (tight coupling: both the
    /// curator's MemoryStore and the kata-kanban's, if wired). Each
    /// store is read independently; the events union.
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

    /// The window's events from every store, decoded by the one wire
    /// home (`hkask_trust::wire`).
    ///
    /// Returns `None` when any store is unavailable (the `Sensor`
    /// contract: "None is not evidence of recovery" — a store outage
    /// is a broken feedback loop, not zero events). A row with an
    /// unparseable ts is skipped by the shared window rule — one
    /// corrupt row is row-level garbage, not a store outage.
    fn trust_window_events(&self, since: DateTime<Utc>) -> Option<Vec<LedgerEvent>> {
        let mut events = Vec::new();
        for memory in &self.stores {
            let h_mems = memory
                .query_deduped_untouched(crate::TRUST_LEDGER_ENTITY)
                .ok()?;
            let rows: Vec<(&str, &serde_json::Value)> = h_mems
                .iter()
                .map(|h| (h.attribute.as_str(), &h.value))
                .collect();
            events.extend(wire::window_from_rows(rows, since));
        }
        Some(events)
    }

    /// The window's observable counts: weighted violation count (the
    /// deviation metric), unverified-explicit count, and the ΔT
    /// direction.
    fn trust_window_stats(&self, since: DateTime<Utc>) -> Option<TrustWindowStats> {
        let events = self.trust_window_events(since)?;
        let mut stats = TrustWindowStats::default();
        for event in &events {
            match event.outcome {
                Outcome::Violated => {
                    stats.weighted_violation_count += i64::from(event.weight.points());
                }
                Outcome::Unverified if event.weight == Weight::Explicit => {
                    stats.unverified_explicit_count += 1;
                }
                _ => {}
            }
        }
        stats.delta_t_direction = TrustDirection::of(&events);
        Some(stats)
    }
}

/// The ΔT direction over the sense window (Binding 2's third
/// observable): fold the window's events in time order and read the
/// second half's contribution — a recovering window (violations early,
/// keeps late) reads Improving, a degrading one reads Degrading, and a
/// window with no recent net movement reads Steady.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum TrustDirection {
    #[default]
    Steady,
    Improving,
    Degrading,
}

impl TrustDirection {
    fn of(events: &[LedgerEvent]) -> Self {
        let mut ordered: Vec<&LedgerEvent> = events.iter().collect();
        ordered.sort_by_key(|e| e.ts);
        let mid = ordered.len() / 2;
        let delta: i32 = ordered[mid..]
            .iter()
            .filter_map(|e| e.fold_event())
            .map(|event| event.delta())
            .sum();
        if delta > 0 {
            TrustDirection::Improving
        } else if delta < 0 {
            TrustDirection::Degrading
        } else {
            TrustDirection::Steady
        }
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
    /// The account's direction over the window (Binding 2's third
    /// observable): improving, steady, or degrading.
    pub delta_t_direction: TrustDirection,
}

#[async_trait::async_trait]
impl Sensor for TrustSensor {
    async fn observe(&self) -> Option<Signal> {
        let since = Utc::now() - chrono::Duration::hours(self.window_hours);
        let stats = self.trust_window_stats(since)?;

        // The deviation value: weighted violation count. The set-point
        // is 0.0 — any positive count is a deviation.
        let value = stats.weighted_violation_count as f64;

        // The three observables, logged at debug — the sensor's
        // observation surface beyond the deviation metric (the counts
        // and the direction stay visible, never silent).
        tracing::debug!(
            target: "reg.sensor.trust",
            weighted_violation_count = stats.weighted_violation_count,
            unverified_explicit_count = stats.unverified_explicit_count,
            ?stats.delta_t_direction,
            "trust window observed"
        );

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

    /// A row with an unparseable ts is skipped by the shared window
    /// rule — row-level garbage, never a store outage. The former
    /// inline decode made ONE corrupt row read as the whole account
    /// unavailable; the one decode home restored the shared skip rule.
    #[tokio::test]
    async fn corrupt_ts_row_is_skipped_not_unavailable() {
        let memory = test_memory_store();
        memory
            .store(trust_h_mem("healthy-event", 3, "violated", 0))
            .expect("store");
        memory
            .store(hkask_storage::HMem::new(
                crate::TRUST_LEDGER_ENTITY,
                "corrupt-row",
                serde_json::json!({
                    "text": "corrupt row",
                    "source": "test",
                    "weight": 3,
                    "outcome": "violated",
                    "ts": "not-a-timestamp",
                    "oracle_note": null,
                }),
                hkask_types::WebID::new(),
            ))
            .expect("store corrupt row");

        let sensor = TrustSensor::new(vec![memory], 24);
        let signal = sensor
            .observe()
            .await
            .expect("the healthy row still reads — one corrupt row is not an outage");
        assert_eq!(signal.value, 3.0);
    }

    /// ΔT direction (Binding 2's third observable): violations early
    /// and keeps late read Improving — the recovering signature.
    #[tokio::test]
    async fn direction_reads_improving_when_keeps_follow_violations() {
        let memory = test_memory_store();
        memory
            .store(trust_h_mem("early-violation", 2, "violated", 10))
            .expect("store");
        memory
            .store(trust_h_mem("late-keep", 2, "kept", 1))
            .expect("store");

        let sensor = TrustSensor::new(vec![memory], 24);
        let since = Utc::now() - chrono::Duration::hours(24);
        let stats = sensor.trust_window_stats(since).expect("stats");
        assert_eq!(stats.weighted_violation_count, 2);
        assert_eq!(stats.delta_t_direction, TrustDirection::Improving);
    }

    /// ΔT direction: keeps early and violations late read Degrading.
    #[tokio::test]
    async fn direction_reads_degrading_when_violations_follow_keeps() {
        let memory = test_memory_store();
        memory
            .store(trust_h_mem("early-keep", 2, "kept", 10))
            .expect("store");
        memory
            .store(trust_h_mem("late-violation", 2, "violated", 1))
            .expect("store");

        let sensor = TrustSensor::new(vec![memory], 24);
        let since = Utc::now() - chrono::Duration::hours(24);
        let stats = sensor.trust_window_stats(since).expect("stats");
        assert_eq!(stats.delta_t_direction, TrustDirection::Degrading);
    }

    /// An empty window reads Steady — no recent net movement.
    #[tokio::test]
    async fn empty_window_reads_steady() {
        let memory = test_memory_store();
        let sensor = TrustSensor::new(vec![memory], 24);
        let since = Utc::now() - chrono::Duration::hours(24);
        let stats = sensor.trust_window_stats(since).expect("stats");
        assert_eq!(stats.delta_t_direction, TrustDirection::Steady);
    }
}
