//! CalibratedEnergyEstimator — Self-regulating per-server gas cost estimator.
//!
//! Wraps [`CompositeEnergyEstimator`] and keeps its per-server table in sync with
//! observed `reg.gas.settled` events via [`DynamicGasTable`] and [`GasReport`].
//! A background calibration task can be spawned with ``Self::spawn_calibration``.
//!
//! This closes the Good Regulator feedback loop (P9): estimates are continuously
//! validated against real settlement data and adjusted by exponential moving average.
//!
//! # Design
//!
//! - `DynamicGasTable` and `CompositeEnergyEstimator` are held behind `std::sync::RwLock`
//!   so the synchronous [`EnergyEstimator::estimate_cost`] can read the current
//!   estimator without crossing an async boundary.
//! - Calibration is async and uses `tokio::sync::Mutex` for `last_calibrated_at`.
//! - Calibration is incremental: each pass processes only events since the last
//!   successful calibration, then rebuilds the `CompositeEnergyEstimator` from the
//!   updated table.

use crate::composite_energy_estimator::CompositeEnergyEstimator;
use crate::dynamic_gas_table::DynamicGasTable;
use crate::energy_estimator::EnergyEstimator;
use crate::gas_report::GasReport;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use hkask_types::InfrastructureError;
use hkask_types::LedgerStoragePort;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span};
use serde_json::Value;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use tracing::{info, warn};

/// Default interval between background calibrations.
///
/// expect: "I can configure the default interval for the background gas calibration loop"
pub const DEFAULT_CALIBRATION_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Default lookback window for the first calibration after construction.
///
pub const DEFAULT_INITIAL_LOOKBACK: ChronoDuration = ChronoDuration::hours(1);

/// Self-regulating energy estimator that refreshes its per-server table from
/// settled Regulation gas events.
///
/// # Public Surface (≤7 items — deep-module discipline)
/// - `CalibratedEnergyEstimator` (struct)
/// - `new()` — construct from an event store
/// - `with_initial_lookback()` — configure first-calibration window
/// - `with_event_sink()` — attach a Regulation event sink for calibration spans
/// - `calibrate()` — run one calibration pass
/// - `spawn_calibration()` — spawn a background calibration task
/// - `current_table()` — diagnostic snapshot of the calibrated table
pub struct CalibratedEnergyEstimator {
    store: Arc<dyn LedgerStoragePort>,
    table: RwLock<DynamicGasTable>,
    estimator: RwLock<CompositeEnergyEstimator>,
    last_calibrated_at: tokio::sync::Mutex<DateTime<Utc>>,
    event_sink: Option<Arc<dyn RegulationSink>>,
    calibration_alive: std::sync::atomic::AtomicBool,
}

impl CalibratedEnergyEstimator {
    /// Create a calibrated estimator backed by the given event store.
    ///
    /// expect: "I can configure the default interval for the background gas calibration loop"
    /// pre:  store is a valid LedgerStoragePort
    /// post: returns CalibratedEnergyEstimator with default table and no observations
    /// post: first calibration will look back `DEFAULT_INITIAL_LOOKBACK`
    /// post: no event sink attached until `with_event_sink` is called
    pub fn new(store: Arc<dyn LedgerStoragePort>) -> Self {
        let table = DynamicGasTable::new();
        let estimator = CompositeEnergyEstimator::from_dynamic_table(&table);
        Self {
            store,
            table: RwLock::new(table),
            estimator: RwLock::new(estimator),
            last_calibrated_at: tokio::sync::Mutex::new(Utc::now() - DEFAULT_INITIAL_LOOKBACK),
            event_sink: None,
            calibration_alive: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Configure how far back the first calibration pass searches for events.
    ///
    /// expect: "I can override the initial calibration lookback window for bootstrapping from historical data"
    /// expect: "I can create a calibrated energy estimator backed by the event store for self-regulating cost estimation"
    /// pre:  lookback is a positive duration
    /// post: first calibration will search [Utc::now() - lookback, Utc::now()]
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_initial_lookback(mut self, lookback: ChronoDuration) -> Self {
        let now = Utc::now();
        // Update last_calibrated_at so the first pass covers [now - lookback, now].
        self.last_calibrated_at = tokio::sync::Mutex::new(now - lookback);
        self
    }

    /// Attach a Regulation event sink for calibration span emission.
    ///
    /// expect: "I can attach an event sink so calibration adjustments emit Regulation observability spans"
    /// pre:  sink is a valid RegulationSink
    /// post: subsequent successful calibrations that adjust costs emit a span
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_event_sink(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Run one incremental calibration pass.
    ///
    /// expect: "I can override the initial calibration lookback window for bootstrapping from historical data"
    /// expect: "I can create a calibrated energy estimator backed by the event store for self-regulating cost estimation"
    /// pre:  `self.store` is a valid RegulationArchive
    /// post: all settled gas events since the last calibration are fed into
    ///       `DynamicGasTable`; `CompositeEnergyEstimator` is rebuilt from the
    ///       updated table; returns the number of servers whose costs changed
    #[must_use = "result must be used"]
    pub async fn calibrate(&self) -> Result<usize, InfrastructureError> {
        let until = Utc::now();
        let since = {
            let mut last = self.last_calibrated_at.lock().await;
            let s = *last;
            *last = until;
            s
        };

        let mut table = self
            .table
            .write()
            .map_err(|_| InfrastructureError::LockPoisoned)?;

        let report = GasReport::new(Arc::clone(&self.store));
        let adjusted = report.calibrate_table(&mut table, since, until)?;

        let new_estimator = CompositeEnergyEstimator::from_dynamic_table(&table);
        *self
            .estimator
            .write()
            .map_err(|_| InfrastructureError::LockPoisoned)? = new_estimator;

        info!(
            target: "reg.gas.calibration",
            since = %since,
            until = %until,
            adjusted_servers = adjusted,
            "Calibrated energy estimator"
        );

        if adjusted > 0 {
            self.emit_calibration_span(since, until, adjusted, &table.report_table());
        }

        Ok(adjusted)
    }

    fn emit_calibration_span(
        &self,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
        adjusted: usize,
        table: &std::collections::HashMap<String, u64>,
    ) {
        if let Some(ref sink) = self.event_sink {
            let span = Span::new(
                hkask_types::regulation::RegulationSpan::Gas
                    .try_into()
                    .expect("canonical span"),
                "calibrated",
            );
            let event = RegulationRecord::new(
                Self::default_actor(),
                span,
                CyclePhase::Act,
                serde_json::json!({
                    "since": since,
                    "until": until,
                    "adjusted_servers": adjusted,
                    "server_costs": table,
                }),
                0,
            );
            if let Err(e) = sink.persist(&event) {
                warn!(
                    target: "reg.gas.calibration",
                    error = %e,
                    "Failed to persist calibration Regulation span"
                );
            }
        }
    }

    fn default_actor() -> WebID {
        WebID::from_persona_with_namespace(b"calibrated-energy-estimator", "reg-surface")
    }

    /// Spawn a background task that calls `calibrate()` at the given interval.
    ///
    /// Delegates to the shared `spawn_calibration_loop` — see `calibrator` module.
    pub fn spawn_calibration(self: Arc<Self>, interval: Duration) {
        crate::calibrator::spawn_calibration_loop(self, interval);
    }

    /// Check whether the background calibration task is still running.
    #[must_use]
    pub fn calibration_healthy(&self) -> bool {
        self.calibration_alive
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Snapshot of the current calibrated server-cost table.
    ///
    /// Useful for diagnostics and tests.
    ///
    /// expect: "I can override the initial calibration lookback window for bootstrapping from historical data"
    /// expect: "I can create a calibrated energy estimator backed by the event store for self-regulating cost estimation"
    /// post: returns a copy of the internal server_costs map
    #[must_use]
    pub fn current_table(&self) -> std::collections::HashMap<String, u64> {
        self.table
            .read()
            .map_or_else(|_| std::collections::HashMap::new(), |t| t.report_table())
    }
}

impl EnergyEstimator for CalibratedEnergyEstimator {
    fn estimate_cost(&self, server: &str, tool: &str, args: &Value) -> u64 {
        let estimator = self.estimator.read().unwrap_or_else(|poisoned| {
            // If a writer panicked while holding the lock, recovering the data
            // is better than panicking the caller thread.
            poisoned.into_inner()
        });
        estimator.estimate_cost(server, tool, args)
    }
}

#[async_trait::async_trait]
impl crate::calibrator::Calibrator for CalibratedEnergyEstimator {
    async fn run_calibration(&self) -> Result<usize, InfrastructureError> {
        self.calibrate().await
    }

    fn calibration_alive(&self) -> &std::sync::atomic::AtomicBool {
        &self.calibration_alive
    }

    fn calibration_target(&self) -> &'static str {
        "reg.gas.calibration"
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;
    use hkask_storage::RegulationArchive;
    use hkask_types::WebID;
    use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanKind};
    use std::sync::Mutex;

    /// A test event sink that captures the last persisted event.
    struct CaptureSink {
        last_event: Mutex<Option<RegulationRecord>>,
    }

    impl CaptureSink {
        fn new() -> Self {
            Self {
                last_event: Mutex::new(None),
            }
        }
        fn last_event(&self) -> Option<RegulationRecord> {
            self.last_event
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
    }

    impl RegulationSink for CaptureSink {
        fn persist(
            &self,
            event: &RegulationRecord,
        ) -> Result<(), hkask_types::InfrastructureError> {
            *self.last_event.lock().unwrap_or_else(|e| e.into_inner()) = Some(event.clone());
            Ok(())
        }
    }

    fn settled_event(agent: WebID, server: &str, reserved: u64, actual: u64) -> RegulationRecord {
        RegulationRecord::new(
            agent,
            Span::from_kind(SpanKind::GasSettled),
            CyclePhase::Act,
            serde_json::json!({
                "server": server,
                "tool": "test_tool",
                "reserved": reserved,
                "actual": actual,
                "refunded": reserved.saturating_sub(actual),
            }),
            0,
        )
    }

    #[tokio::test]
    async fn calibrate_updates_costs_from_settled_events() {
        let agent = WebID::new();
        let server = "hkask-mcp-media";

        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let event_store = Arc::new(RegulationArchive::from_driver(driver));
        let store: Arc<dyn LedgerStoragePort> =
            Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;

        let estimator = Arc::new(CalibratedEnergyEstimator::new(store));

        // Before calibration, default cost applies.
        let before = estimator.estimate_cost(server, "search", &serde_json::json!({}));
        assert_eq!(before, 100);

        // Persist a settled event where actual is double the reserved cost.
        let event = settled_event(agent, server, 100, 200);
        event_store.persist(&event).unwrap();

        // Calibrate over a window that includes the event.
        let adjusted = estimator.calibrate().await.unwrap();
        assert_eq!(adjusted, 1);

        // After calibration, cost should double from 100 to 200.
        let after = estimator.estimate_cost(server, "search", &serde_json::json!({}));
        assert_eq!(after, 200);
    }

    #[tokio::test]
    async fn calibrate_is_incremental() {
        let agent = WebID::new();

        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let event_store = Arc::new(RegulationArchive::from_driver(driver));
        let store: Arc<dyn LedgerStoragePort> =
            Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;
        let estimator = Arc::new(CalibratedEnergyEstimator::new(store));

        let server_a = "hkask-mcp-media";
        let server_b = "hkask-mcp-research";

        // First calibration window: only server A observed.
        event_store
            .persist(&settled_event(agent, server_a, 100, 200))
            .unwrap();
        assert_eq!(estimator.calibrate().await.unwrap(), 1);
        assert_eq!(
            estimator.estimate_cost(server_a, "search", &serde_json::json!({})),
            200
        );

        // Second calibration window: server B observed for the first time.
        event_store
            .persist(&settled_event(agent, server_b, 50, 100))
            .unwrap();
        assert_eq!(estimator.calibrate().await.unwrap(), 1);
        assert_eq!(
            estimator.estimate_cost(server_b, "search", &serde_json::json!({})),
            100
        );
        // Server A cost remains stable (no new A events in second window).
        assert_eq!(
            estimator.estimate_cost(server_a, "search", &serde_json::json!({})),
            200
        );
    }

    #[test]
    fn with_initial_lookback_changes_first_window() {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let event_store = Arc::new(RegulationArchive::from_driver(driver));
        let store: Arc<dyn LedgerStoragePort> =
            Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;

        let estimator = CalibratedEnergyEstimator::new(store)
            .with_initial_lookback(ChronoDuration::minutes(30));

        // We cannot observe the internal last_calibrated_at, but we can verify
        // the table accessor works and the estimator estimates normally.
        assert!(!estimator.current_table().is_empty());
        assert_eq!(
            estimator.estimate_cost("hkask-mcp-media", "search", &serde_json::json!({})),
            100
        );
    }

    #[tokio::test]
    async fn calibrate_emits_reg_gas_span_when_adjusted() {
        let agent = WebID::new();
        let server = "hkask-mcp-media";

        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let event_store = Arc::new(RegulationArchive::from_driver(driver));
        let sink = Arc::new(CaptureSink::new());

        let store: Arc<dyn LedgerStoragePort> =
            Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;
        let estimator = Arc::new(
            CalibratedEnergyEstimator::new(store)
                .with_event_sink(Arc::clone(&sink) as Arc<dyn RegulationSink>),
        );

        event_store
            .persist(&settled_event(agent, server, 100, 200))
            .unwrap();

        let adjusted = estimator.calibrate().await.unwrap();
        assert_eq!(adjusted, 1);

        let event = sink
            .last_event()
            .expect("calibration span should be emitted");
        assert_eq!(event.span.as_str(), "reg.gas.calibrated");
        assert_eq!(event.phase, CyclePhase::Act);
        assert_eq!(
            event
                .observation
                .get("adjusted_servers")
                .and_then(|v| v.as_u64()),
            Some(1)
        );
    }

    #[tokio::test]
    async fn calibrate_does_not_emit_span_when_not_adjusted() {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let event_store = Arc::new(RegulationArchive::from_driver(driver));
        let sink = Arc::new(CaptureSink::new());

        let store: Arc<dyn LedgerStoragePort> =
            Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;
        let estimator = Arc::new(
            CalibratedEnergyEstimator::new(store)
                .with_event_sink(Arc::clone(&sink) as Arc<dyn RegulationSink>),
        );

        let adjusted = estimator.calibrate().await.unwrap();
        assert_eq!(adjusted, 0);
        assert!(
            sink.last_event().is_none(),
            "no adjustment should not emit a calibration span"
        );
    }
}
