//! Sensor trait — pluggable metric sensors (Fermi Extractor pattern).
//!
//! Fermi's `Extractor` trait separates domain data extraction from the fitting
//! loop. Sensor applies the same pattern to hKask's regulation loop:
//! each metric gets its own `Sensor` implementation, registered with
//! a `SensorBus`. The `CyberneticsLoop::sense()` method walks the bus
//! instead of containing inline sensing logic.
//!
//! ## Why this lives in hkask-regulation
//!
//! Sensor providers are Regulation regulation infrastructure. They live alongside
//! `CyberneticsLoop`, `StagnationDetector`, and `SetPoints` in `hkask-regulation`,
//! the crate responsible for homeostatic self-regulation.

use super::loops::{LoopId, Signal, SignalMetric};
use parking_lot::Mutex;
use std::sync::Arc;

/// A pluggable sensor that produces one kind of signal metric.
///
/// Each implementation senses a single `SignalMetric` from its data source.
/// Fermi pattern: the `Extractor` trait takes a domain payload and produces
/// a scalar; `Sensor` takes system state and produces an optional
/// `Signal`. If the sensor has nothing to report (metric is healthy),
/// it returns `None`.
#[async_trait::async_trait]
pub(crate) trait Sensor: Send + Sync {
    /// Sense the current state and produce a signal if the metric is
    /// in a reportable state. Returns `None` if nothing to report.
    async fn sense(&self) -> Option<Signal>;
}

/// Sensor bus for a single loop — actively walks sensors each tick.
///
/// Providers are registered at construction time and executed in order.
/// Order doesn't matter — each provider independently decides whether
/// to emit a signal. The bus aggregates their signals into a single
/// `Vec<Signal>` for the loop's `sense()` phase.
pub(crate) struct SensorBus {
    providers: Mutex<Vec<Arc<dyn Sensor>>>,
}

impl SensorBus {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new() -> Self {
        Self {
            providers: Mutex::new(Vec::new()),
        }
    }

    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn register(&self, provider: Arc<dyn Sensor>) {
        self.providers.lock().push(provider);
    }

    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub async fn sense_all(&self, source: LoopId) -> Vec<Signal> {
        let providers: Vec<Arc<dyn Sensor>> = { self.providers.lock().clone() }; // Lock dropped here — no .await while holding it.
        let mut signals = Vec::new();
        for provider in &providers {
            if let Some(signal) = provider.sense().await {
                signals.push(signal);
            }
        }
        for s in &mut signals {
            s.source = source;
        }
        signals
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// CONCRETE SENSOR PROVIDERS
// ═════════════════════════════════════════════════════════════════════════════

/// Senses energy budget remaining ratios across all agents.
///
/// Data source: `CallCapManager`. Produces a signal per agent.
pub(crate) struct EnergyBudgetSensor {
    cap_manager: Arc<tokio::sync::RwLock<super::energy::CallCapManager>>,
    set_point: f64,
}

impl EnergyBudgetSensor {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new(
        cap_manager: Arc<tokio::sync::RwLock<super::energy::CallCapManager>>,
        set_point: f64,
    ) -> Self {
        Self {
            cap_manager,
            set_point,
        }
    }
}

#[async_trait::async_trait]
impl Sensor for EnergyBudgetSensor {
    async fn sense(&self) -> Option<Signal> {
        let statuses = self.cap_manager.read().await.all_agent_statuses().await;
        // Use the worst remaining ratio as the aggregate signal.
        let worst = statuses
            .iter()
            .map(|(_, s)| s.remaining as f64 / s.ceiling.max(1) as f64)
            .fold(1.0, f64::min);
        // Only emit when energy is below the floor — healthy states produce
        // no signal, matching TestCoverageSensor and ToolReliabilitySensor.
        // Without this gate the sensor emits AboveSetPoint deviations for
        // healthy energy levels, which no policy rule matches, leaving the
        // loop open (gain=0, fidelity=0 every tick).
        if worst >= self.set_point {
            return None;
        }
        Some(Signal::new(
            LoopId::Cybernetics, // placeholder — registry backfills
            SignalMetric::EnergyRemaining,
            worst,
            self.set_point,
        ))
    }
}

/// Senses variety deficit from the Regulation runtime.
///
/// Data source: `RegulationLedger`. Produces a single aggregate signal.
pub(crate) struct VarietySensor {
    ledger: Arc<tokio::sync::RwLock<super::runtime::RegulationLedger>>,
    set_point: f64,
}

impl VarietySensor {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new(
        ledger: Arc<tokio::sync::RwLock<super::runtime::RegulationLedger>>,
        set_point: f64,
    ) -> Self {
        Self { ledger, set_point }
    }
}

#[async_trait::async_trait]
impl Sensor for VarietySensor {
    async fn sense(&self) -> Option<Signal> {
        let ledger = self.ledger.read().await;
        let health = ledger.health().await;
        // Only emit when deficit exceeds the max — healthy states produce no
        // signal, matching TestCoverageSensor and ToolReliabilitySensor.
        // Without this gate the sensor emits BelowSetPoint deviations for
        // healthy variety levels, which no policy rule matches, leaving the
        // loop open (gain=0, fidelity=0 every tick).
        if health.overall_deficit as f64 <= self.set_point {
            return None;
        }
        Some(Signal::new(
            LoopId::Cybernetics, // placeholder — registry backfills
            SignalMetric::VarietyDeficit,
            health.overall_deficit as f64,
            self.set_point,
        ))
    }
}

/// case, which is `Ok(None)`. Collapsing these into a single `None` masked
/// DB outages and permission errors as "no deviation," blinding the
/// regulation loop (the `.rules` `unwrap_or(0)` / `.ok()?` trap on sense
/// inputs). See `tool_stats::read_count_field` for the canonical warn-then-
/// fallback pattern this mirrors.
#[derive(Debug, thiserror::Error)]
pub(crate) enum MetricsLocateError {
    /// The trace directory exists but could not be read (permission denied,
    /// not a directory). The sensor cannot determine whether any run has
    /// metrics — this is a broken sensor, not an empty one.
    ///
    /// Note: a *missing* trace directory is not this variant — it is the
    /// legitimate "no metrics yet" state and is returned as `Ok(None)` by
    /// `latest_run_metrics`. Treating `NotFound` as an error spammed the log
    /// with warnings every loop tick before any trace run had occurred.
    #[error("trace directory unreadable: {path}: {error}")]
    TraceDirInaccessible {
        path: std::path::PathBuf,
        #[source]
        error: std::io::Error,
    },
    /// A `metrics.json` candidate was found but its metadata (specifically
    /// the modification time used to pick the newest run) could not be read.
    /// The file is present but unreadable — a broken sensor.
    #[error("metrics metadata unreadable: {path}: {error}")]
    MetadataUnavailable {
        path: std::path::PathBuf,
        #[source]
        error: std::io::Error,
    },
}

/// Find the run directory whose `metrics.json` was most recently modified.
///
/// Returns `Ok(None)` when the trace directory does not exist or exists but
/// contains no run with a `metrics.json` (the legitimate "no metrics yet"
/// case). Returns `Err` for genuine I/O failures (permission denied, not a
/// directory, metadata unreadable) so the caller can `warn!` and distinguish a
/// broken sensor from an empty one — collapsing the two into `None` made a DB
/// outage indistinguishable from "coverage meets set-point" (F1/F2).
///
/// Shared by `TestCoverageSensor` and `MutationScoreSensor`; extracting this
/// closes the byte-identical duplication and gives one place to enforce the
/// error-classification contract. Public so the error-classification contract
/// can be pinned by integration tests.
pub(crate) fn latest_run_metrics(
    trace_dir: &std::path::Path,
) -> Result<Option<std::path::PathBuf>, MetricsLocateError> {
    let entries = match std::fs::read_dir(trace_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A missing trace directory is the normal "no metrics yet" state, not
            // a broken sensor — return `Ok(None)` so the sensor stays silent.
            // Only genuine I/O failures (permission denied, not a directory, etc.)
            // are classified as `TraceDirInaccessible` to trigger the warn.
            return Ok(None);
        }
        Err(error) => {
            return Err(MetricsLocateError::TraceDirInaccessible {
                path: trace_dir.to_path_buf(),
                error,
            });
        }
    };
    let mut newest: Option<(std::path::PathBuf, std::time::SystemTime)> = None;
    for entry in entries.flatten() {
        let metrics = entry.path().join("metrics.json");
        if !metrics.is_file() {
            continue;
        }
        let modified = std::fs::metadata(&metrics)
            .and_then(|m| m.modified())
            .map_err(|error| MetricsLocateError::MetadataUnavailable {
                path: metrics.clone(),
                error,
            })?;
        match &newest {
            Some((_, best)) if &modified <= best => {}
            _ => newest = Some((metrics, modified)),
        }
    }
    Ok(newest.map(|(p, _)| p))
}

/// Senses test coverage from the latest trace run's `metrics.json`.
///
/// Data source: the trace filesystem (`HKASK_TRACE_DIR`, default `{HKASK_DATA_DIR}/traces`).
/// Produces a signal only when `coverage_pct` is below the coverage floor.
pub(crate) struct TestCoverageSensor {
    trace_dir: std::path::PathBuf,
    set_point: f64,
}

impl TestCoverageSensor {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new(trace_dir: std::path::PathBuf, set_point: f64) -> Self {
        Self {
            trace_dir,
            set_point,
        }
    }
}

#[async_trait::async_trait]
impl Sensor for TestCoverageSensor {
    async fn sense(&self) -> Option<Signal> {
        let path = match latest_run_metrics(&self.trace_dir) {
            Ok(path) => path?,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.sensor.coverage",
                    error = %error,
                    "TestCoverageSensor: trace metrics unreadable — returning no signal (not 'no deviation')"
                );
                return None;
            }
        };
        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.sensor.coverage",
                    path = %path.display(),
                    error = %error,
                    "TestCoverageSensor: metrics.json unreadable — returning no signal (not 'no deviation')"
                );
                return None;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&contents) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.sensor.coverage",
                    path = %path.display(),
                    error = %error,
                    "TestCoverageSensor: metrics.json unparsable — returning no signal (not 'no deviation')"
                );
                return None;
            }
        };
        let coverage = match value.get("coverage_pct").and_then(|v| v.as_f64()) {
            Some(coverage) => coverage,
            None => return None,
        };
        if coverage >= self.set_point {
            return None;
        }
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::TestCoverage,
            coverage,
            self.set_point,
        ))
    }
}

/// Senses tool reliability from the Regulation runtime's outcome tracker.
///
/// Data source: `RegulationLedger::outcome_success_rate`. Produces a signal
/// only when the aggregate success rate across all tracked domains drops
/// below the reliability threshold. This closes the feedback loop that was
/// blind to systematic tool failures (e.g. MCP server timeouts looping for
/// minutes without the regulation loop sensing the deviation).
///
/// Returns `None` when no outcomes have been recorded yet (the legitimate
/// "no data" state) — not a signal with value 1.0, which would mask a
/// broken sensor as "healthy" (the `.rules` `unwrap_or(0)` trap).
pub(crate) struct ToolReliabilitySensor {
    ledger: Arc<tokio::sync::RwLock<super::runtime::RegulationLedger>>,
    set_point: f64,
}

impl ToolReliabilitySensor {
    pub fn new(
        ledger: Arc<tokio::sync::RwLock<super::runtime::RegulationLedger>>,
        set_point: f64,
    ) -> Self {
        Self { ledger, set_point }
    }
}

#[async_trait::async_trait]
impl Sensor for ToolReliabilitySensor {
    async fn sense(&self) -> Option<Signal> {
        let ledger = self.ledger.read().await;
        // Aggregate success rate across all tracked domains. Each domain's
        // success rate is weighted equally (not by call count) so a single
        // high-volume domain doesn't dominate the signal. A domain with
        // zero operations is excluded (no data, not 0% success).
        let domains = ledger.tracked_outcome_domains().await;
        let mut sum = 0.0;
        let mut count = 0;
        for domain in &domains {
            if let Some(rate) = ledger.outcome_success_rate(domain).await {
                sum += rate;
                count += 1;
            }
        }
        if count == 0 {
            return None;
        }
        let aggregate = sum / count as f64;
        if aggregate >= self.set_point {
            return None;
        }
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::ToolReliability,
            aggregate,
            self.set_point,
        ))
    }
}

/// Senses mutation score from the latest trace run's `metrics.json`.
///
/// Data source: the trace filesystem (`HKASK_TRACE_DIR`, default `{HKASK_DATA_DIR}/traces`).
/// Produces a signal only when `mutation_score` is below the mutation score floor.
pub(crate) struct MutationScoreSensor {
    trace_dir: std::path::PathBuf,
    set_point: f64,
}

impl MutationScoreSensor {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new(trace_dir: std::path::PathBuf, set_point: f64) -> Self {
        Self {
            trace_dir,
            set_point,
        }
    }
}

#[async_trait::async_trait]
impl Sensor for MutationScoreSensor {
    async fn sense(&self) -> Option<Signal> {
        let path = match latest_run_metrics(&self.trace_dir) {
            Ok(path) => path?,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.sensor.mutation",
                    error = %error,
                    "MutationScoreSensor: trace metrics unreadable — returning no signal (not 'no deviation')"
                );
                return None;
            }
        };
        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.sensor.mutation",
                    path = %path.display(),
                    error = %error,
                    "MutationScoreSensor: metrics.json unreadable — returning no signal (not 'no deviation')"
                );
                return None;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&contents) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.sensor.mutation",
                    path = %path.display(),
                    error = %error,
                    "MutationScoreSensor: metrics.json unparsable — returning no signal (not 'no deviation')"
                );
                return None;
            }
        };
        let score = match value.get("mutation_score").and_then(|v| v.as_f64()) {
            Some(score) => score,
            None => return None,
        };
        if score >= self.set_point {
            return None;
        }
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::MutationScore,
            score,
            self.set_point,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::energy::CallCapManager;
    use crate::runtime::RegulationLedger;

    /// Pins Fix 1: EnergyBudgetSensor must return None when energy is healthy
    /// (worst remaining ratio >= set_point). Without the gate the sensor
    /// emits AboveSetPoint deviations for healthy energy levels, which no
    /// policy rule matches, leaving the regulation loop open
    /// (gain=0, fidelity=0 every tick).
    #[tokio::test]
    async fn energy_budget_sensor_returns_none_when_healthy() {
        let cap_manager = Arc::new(tokio::sync::RwLock::new(CallCapManager::new()));
        let sensor = EnergyBudgetSensor::new(cap_manager, 0.2);
        assert!(
            sensor.sense().await.is_none(),
            "healthy energy (no agents -> worst=1.0 >= set_point=0.2) returns None"
        );
    }

    /// Pins Fix 1: VarietySensor must return None when variety deficit is
    /// healthy (deficit <= set_point). Without the gate the sensor emits
    /// BelowSetPoint deviations for healthy variety levels, which no policy
    /// rule matches, leaving the regulation loop open.
    #[tokio::test]
    async fn variety_sensor_returns_none_when_healthy() {
        let ledger = Arc::new(tokio::sync::RwLock::new(RegulationLedger::default()));
        let sensor = VarietySensor::new(ledger, 100.0);
        assert!(
            sensor.sense().await.is_none(),
            "healthy variety (deficit=0 <= set_point=100) returns None"
        );
    }
}
