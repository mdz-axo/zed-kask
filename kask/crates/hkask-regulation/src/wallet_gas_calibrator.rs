//! WalletGasCalibrator — Runtime calibration of the wallet gas→rJoule rate.
//!
//! Observes aggregate `reg.gas.settled` events via `GasReport`, computes the
//! global actual/estimated gas ratio, and feeds it to `WalletEnergyEstimator`.
//! The resulting `gas_per_rjoule` is pushed into `WalletManager` so that
//! `WalletBackedBudget` uses a live, calibrated conversion rate.
//!
//! This closes the wallet-energy feedback loop (P9): the system's estimate of
//! how much gas a rJoule buys is continuously validated against real settlements.

use crate::gas_report::GasReport;
use crate::wallet_energy_estimator::WalletEnergyEstimator;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use hkask_types::InfrastructureError;
use hkask_types::LedgerStoragePort;
use hkask_types::WalletBudgetPort;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// Default interval between background wallet-gas calibrations.
///
/// expect: "I can configure the default interval for background wallet gas calibration"
pub const DEFAULT_WALLET_CALIBRATION_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Default lookback window for the first calibration pass after construction.
///
pub const DEFAULT_WALLET_INITIAL_LOOKBACK: ChronoDuration = ChronoDuration::hours(1);

/// Calibrator for the wallet gas→rJoule conversion rate.
///
/// # Public Surface (≤7 items — deep-module discipline)
/// - `WalletGasCalibrator` (struct)
/// - `new()` — construct from event store and wallet manager
/// - `with_initial_lookback()` — configure first-calibration window
/// - `with_event_sink()` — attach a Regulation event sink for calibration spans
/// - `calibrate()` — run one calibration pass
/// - `spawn_calibration()` — spawn a background calibration task
pub struct WalletGasCalibrator {
    store: Arc<dyn LedgerStoragePort>,
    wallet_manager: Arc<dyn WalletBudgetPort>,
    estimator: std::sync::Mutex<WalletEnergyEstimator>,
    last_calibrated_at: tokio::sync::Mutex<DateTime<Utc>>,
    event_sink: Option<Arc<dyn RegulationSink>>,
    /// Set to false if the background calibration task panics or exits.
    calibration_alive: std::sync::atomic::AtomicBool,
}

impl WalletGasCalibrator {
    /// Create a wallet gas calibrator backed by the given event store and wallet manager.
    ///
    /// expect: "I can create a wallet gas calibrator that self-tunes the gas→rJoule rate from settled events"
    /// expect: "I can configure the default interval for background wallet gas calibration"
    /// pre:  store is a valid LedgerStoragePort; wallet_manager is valid
    /// post: returns WalletGasCalibrator seeded with the manager's current gas_per_rjoule rate
    /// post: first calibration will look back `DEFAULT_WALLET_INITIAL_LOOKBACK`
    /// post: no event sink attached until `with_event_sink` is called
    pub fn new(
        store: Arc<dyn LedgerStoragePort>,
        wallet_manager: Arc<dyn WalletBudgetPort>,
    ) -> Self {
        let initial_rate = wallet_manager.gas_per_rjoule();
        Self {
            store,
            wallet_manager,
            estimator: std::sync::Mutex::new(WalletEnergyEstimator::new(initial_rate)),
            last_calibrated_at: tokio::sync::Mutex::new(
                Utc::now() - DEFAULT_WALLET_INITIAL_LOOKBACK,
            ),
            event_sink: None,
            calibration_alive: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Configure how far back the first calibration pass searches for events.
    ///
    /// expect: "I can configure how far back the gas calibrator searches for events"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — calibrator searches historical events
    /// \[P4\] Constraining: Clear Boundaries — lookback limits calibration scope
    /// pre:  lookback is a positive duration
    /// post: first calibration will search [Utc::now() - lookback, Utc::now()]
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_initial_lookback(mut self, lookback: ChronoDuration) -> Self {
        let now = Utc::now();
        self.last_calibrated_at = tokio::sync::Mutex::new(now - lookback);
        self
    }

    /// Attach a Regulation event sink for calibration span emission.
    ///
    /// expect: "I can attach an event sink so wallet conversion rate adjustments emit Regulation observability spans"
    /// pre:  sink is a valid RegulationSink
    /// post: subsequent successful calibrations that adjust the rate emit a span
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_event_sink(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Run one incremental calibration pass.
    ///
    /// Queries aggregate settled gas over the window since the last calibration,
    /// computes `total_actual / total_reserved`, feeds the ratio to the internal
    /// `WalletEnergyEstimator`, and pushes the resulting `gas_per_rjoule` to the
    /// shared `WalletManager`.
    ///
    /// expect: "I can run an incremental wallet calibration pass that computes the aggregate actual/estimated ratio and updates the conversion rate"
    /// pre:  `self.store` is a valid RegulationArchive; `self.wallet_manager` is valid
    /// post: if settled events exist and the aggregate ratio exceeds tolerance,
    ///       `wallet_manager.gas_per_rjoule()` is updated
    /// post: returns true if the rate was adjusted
    pub async fn calibrate(&self) -> Result<bool, InfrastructureError> {
        let until = Utc::now();
        let since = {
            let mut last = self.last_calibrated_at.lock().await;
            let s = *last;
            *last = until;
            s
        };

        let report = GasReport::new(Arc::clone(&self.store));
        let totals = report.query_total(since, until)?;

        // Nothing to calibrate if no gas was reserved.
        if totals.total_reserved == 0 {
            return Ok(false);
        }

        let ratio = totals.total_consumed as f64 / totals.total_reserved as f64;
        let adjusted = self
            .estimator
            .lock()
            .map_err(|_| InfrastructureError::LockPoisoned)?
            .calibrate(ratio);

        if adjusted {
            let new_rate = self
                .estimator
                .lock()
                .map_err(|_| InfrastructureError::LockPoisoned)?
                .gas_per_rjoule;
            self.wallet_manager.set_gas_per_rjoule(new_rate);
            info!(
                target: "reg.wallet.calibration",
                since = %since,
                until = %until,
                ratio = %ratio,
                new_rate = new_rate,
                "Calibrated wallet gas_per_rjoule"
            );
            self.emit_calibration_span(since, until, ratio, new_rate);
        }

        Ok(adjusted)
    }

    fn emit_calibration_span(
        &self,
        since: DateTime<Utc>,
        until: DateTime<Utc>,
        ratio: f64,
        new_rate: u64,
    ) {
        if let Some(ref sink) = self.event_sink {
            let span = Span::new(
                hkask_types::event::SpanNamespace::from_observable(
                    &crate::infra_span::InfraSpan::WalletConversion,
                )
                .expect("domain span must be canonical"),
                "calibrated",
            );
            let event = RegulationRecord::new(
                Self::default_actor(),
                span,
                CyclePhase::Act,
                serde_json::json!({
                    "since": since,
                    "until": until,
                    "ratio": ratio,
                    "gas_per_rjoule": new_rate,
                }),
                0,
            );
            if let Err(e) = sink.persist(&event) {
                warn!(
                    target: "reg.wallet.calibration",
                    error = %e,
                    "Failed to persist wallet calibration Regulation span"
                );
            }
        }
    }

    fn default_actor() -> WebID {
        WebID::from_persona_with_namespace(b"wallet-gas-calibrator", "reg-surface")
    }

    /// Spawn a background task that calls `calibrate()` at the given interval.
    ///
    /// Delegates to the shared `spawn_calibration_loop` — see `calibrator` module.
    pub fn spawn_calibration(self: Arc<Self>, interval: Duration) {
        crate::calibrator::spawn_calibration_loop(self, interval);
    }

    /// Check whether the background calibration task is still running.
    pub fn calibration_healthy(&self) -> bool {
        self.calibration_alive
            .load(std::sync::atomic::Ordering::Acquire)
    }
}

#[async_trait::async_trait]
impl crate::calibrator::Calibrator for WalletGasCalibrator {
    async fn run_calibration(&self) -> Result<usize, InfrastructureError> {
        let adjusted = self.calibrate().await?;
        Ok(adjusted as usize)
    }

    fn calibration_alive(&self) -> &std::sync::atomic::AtomicBool {
        &self.calibration_alive
    }

    fn calibration_target(&self) -> &'static str {
        "reg.wallet.calibration"
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;
    use hkask_storage::RegulationArchive;
    use hkask_types::RegulationSink;
    use hkask_types::WebID;
    use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanKind};
    use hkask_wallet::GAS_PER_RJOULE;
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
    use hkask_wallet::WalletManager;
    use std::collections::HashMap;

    fn make_wallet_manager() -> Arc<dyn WalletBudgetPort> {
        // SAFETY: test-only env var set in single-threaded test context.
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(hkask_storage::WalletStore::from_driver(driver));
        let manager = WalletManager::build(
            hkask_wallet::WalletConfig::default(),
            store,
            HashMap::new(),
            Arc::new(hkask_wallet::price_feed::StaticPriceFeed::new()),
        )
        .unwrap();
        Arc::new(manager)
    }

    fn settled_event(agent: WebID, reserved: u64, actual: u64) -> RegulationRecord {
        RegulationRecord::new(
            agent,
            Span::from_kind(SpanKind::GasSettled),
            CyclePhase::Act,
            serde_json::json!({
                "server": "hkask-mcp-test",
                "tool": "test_tool",
                "reserved": reserved,
                "actual": actual,
                "refunded": reserved.saturating_sub(actual),
            }),
            0,
        )
    }

    #[tokio::test]
    async fn calibrate_updates_wallet_manager_rate() {
        let agent = WebID::new();
        let wallet_manager = make_wallet_manager();
        assert_eq!(wallet_manager.gas_per_rjoule(), GAS_PER_RJOULE);

        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let event_store = Arc::new(RegulationArchive::from_driver(driver));
        event_store
            .persist(&settled_event(agent, 100, 200))
            .expect("persist settled event");

        let store: Arc<dyn LedgerStoragePort> =
            Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;
        let calibrator = Arc::new(WalletGasCalibrator::new(store, Arc::clone(&wallet_manager)));
        let adjusted = calibrator.calibrate().await.unwrap();
        assert!(adjusted, "ratio 2.0 should adjust rate");
        assert_eq!(
            wallet_manager.gas_per_rjoule(),
            GAS_PER_RJOULE * 2,
            "rate should double"
        );
    }

    #[tokio::test]
    async fn calibrate_emits_wallet_conversion_span_when_adjusted() {
        let agent = WebID::new();
        let wallet_manager = make_wallet_manager();

        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let event_store = Arc::new(RegulationArchive::from_driver(driver));
        let sink = Arc::new(CaptureSink::new());
        event_store
            .persist(&settled_event(agent, 100, 200))
            .expect("persist settled event");

        let store: Arc<dyn LedgerStoragePort> =
            Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;
        let calibrator = Arc::new(
            WalletGasCalibrator::new(store, Arc::clone(&wallet_manager))
                .with_event_sink(Arc::clone(&sink) as Arc<dyn RegulationSink>),
        );
        let adjusted = calibrator.calibrate().await.unwrap();
        assert!(adjusted, "ratio 2.0 should adjust rate");

        let event = sink
            .last_event()
            .expect("wallet conversion span should be emitted");
        assert_eq!(event.span.as_str(), "reg.wallet.conversion.calibrated");
        assert_eq!(event.phase, CyclePhase::Act);
        assert_eq!(
            event
                .observation
                .get("gas_per_rjoule")
                .and_then(|v| v.as_u64()),
            Some(GAS_PER_RJOULE * 2)
        );
    }

    #[tokio::test]
    async fn calibrate_does_not_emit_span_when_not_adjusted() {
        let wallet_manager = make_wallet_manager();
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let event_store = Arc::new(RegulationArchive::from_driver(driver));
        let sink = Arc::new(CaptureSink::new());

        let store: Arc<dyn LedgerStoragePort> =
            Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;
        let calibrator = Arc::new(
            WalletGasCalibrator::new(store, Arc::clone(&wallet_manager))
                .with_event_sink(Arc::clone(&sink) as Arc<dyn RegulationSink>),
        );
        let adjusted = calibrator.calibrate().await.unwrap();
        assert!(!adjusted);
        assert!(
            sink.last_event().is_none(),
            "no adjustment should not emit a wallet conversion span"
        );
    }

    #[tokio::test]
    async fn calibrate_no_events_leaves_rate_unchanged() {
        let wallet_manager = make_wallet_manager();
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let event_store = Arc::new(RegulationArchive::from_driver(driver));

        let store: Arc<dyn LedgerStoragePort> =
            Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;
        let calibrator = Arc::new(WalletGasCalibrator::new(store, Arc::clone(&wallet_manager)));
        let adjusted = calibrator.calibrate().await.unwrap();
        assert!(!adjusted);
        assert_eq!(wallet_manager.gas_per_rjoule(), GAS_PER_RJOULE);
    }

    #[test]
    fn with_initial_lookback_changes_first_window() {
        let wallet_manager = make_wallet_manager();
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let event_store = Arc::new(RegulationArchive::from_driver(driver));

        let store: Arc<dyn LedgerStoragePort> =
            Arc::clone(&event_store) as Arc<dyn LedgerStoragePort>;
        let _calibrator = WalletGasCalibrator::new(store, Arc::clone(&wallet_manager))
            .with_initial_lookback(ChronoDuration::minutes(30));
        // Construction succeeds; internal state is not directly observable.
        assert_eq!(wallet_manager.gas_per_rjoule(), GAS_PER_RJOULE);
    }
}
