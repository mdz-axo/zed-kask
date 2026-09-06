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
/// `Signal`. Healthy observations are returned too; `None` means unavailable.
/// The compare phase, not the sensor, decides whether action is needed.
#[async_trait::async_trait]
pub(crate) trait Sensor: Send + Sync {
    /// Read the current state, including health. None is not evidence of recovery.
    async fn observe(&self) -> Option<Signal>;

    #[cfg(test)]
    async fn sense(&self) -> Option<Signal> {
        self.observe()
            .await
            .filter(|signal| super::loops::Deviation::from_signal(signal).is_some())
    }
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
            if let Some(signal) = provider.observe().await {
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
    async fn observe(&self) -> Option<Signal> {
        let statuses = self.cap_manager.read().await.all_agent_statuses().await;
        if statuses.is_empty() {
            return None;
        }
        // Use the worst remaining ratio as the aggregate signal.
        let worst = statuses
            .iter()
            .map(|(_, s)| s.remaining as f64 / s.ceiling.max(1) as f64)
            .fold(1.0, f64::min);
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
    async fn observe(&self) -> Option<Signal> {
        let ledger = self.ledger.read().await;
        let health = ledger.health().await;
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
/// Returns the latest measured coverage, including values above the floor.
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
    async fn observe(&self) -> Option<Signal> {
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
/// Data source: `RegulationLedger::outcome_success_rate`. Returns the aggregate
/// success rate across domains with current samples, including healthy readings.
/// This closes the feedback loop that was
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
    async fn observe(&self) -> Option<Signal> {
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
/// Returns the latest measured mutation score, including values above the floor.
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
    async fn observe(&self) -> Option<Signal> {
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
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::MutationScore,
            score,
            self.set_point,
        ))
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// INFERENCE HEALTH SENSOR (closes the blind-feedback-loop gap)
// ═════════════════════════════════════════════════════════════════════════════

/// A snapshot of inference health, read by [`InferenceHealthSensor`] from the
/// inference dispatch layer.
///
/// This trait lives in `hkask-regulation` (not `kask_bridge`) because the
/// dependency direction is `kask_bridge → hkask-regulation` — the regulation
/// crate cannot depend on the bridge. The bridge implements this trait and
/// passes an `Arc<dyn InferenceHealthSource>` to
/// `CyberneticsLoop::with_inference_health_source`.
///
/// Without this sensor, the cybernetics loop reports `signal_count=0` during
/// an inference timeout storm because its existing sensors read ledger/DB
/// state, not inference dispatch state. The loop's `signal_count=0` is the
/// silent witness of a broken feedback loop (the `.rules` `unwrap_or(0)` trap:
/// a missing sense input reads as "no deviation").
#[async_trait::async_trait]
pub trait InferenceHealthSource: Send + Sync {
    /// Number of inference calls currently in-flight (acquired a permit but
    /// not yet completed). `0` when no calls are active.
    async fn in_flight(&self) -> usize;

    /// Configured maximum concurrent inference calls (`max_concurrency`).
    async fn max_concurrency(&self) -> usize;

    /// Number of inference calls that timed out in the recent window
    /// (e.g. last 5 minutes). `0` when no timeouts have been observed.
    async fn recent_timeout_count(&self) -> u64;
}

/// Senses inference health from the inference dispatch layer.
///
/// Emits `SignalMetric::InferenceAvailable` with value `0.0` when the
/// inference layer is saturated (in_flight >= max_concurrency) or when recent
/// timeouts exceed a threshold. The set-point is `1.0` (fully available); any
/// deviation below `1.0` means the inference layer is degraded.
///
/// This closes the feedback loop that was blind to the 300s timeout storm:
/// the cybernetics loop now senses inference saturation and can act on it
/// (throttle, escalate) instead of reporting `signal_count=0` while inference
/// burns 96 concurrent slots.
pub(crate) struct InferenceHealthSensor {
    source: Arc<dyn InferenceHealthSource>,
    /// Timeout count above which the sensor reports inference as unavailable.
    /// Default 3 — a single timeout is transient, 3+ in the recent window is
    /// a storm.
    timeout_threshold: u64,
}

impl InferenceHealthSensor {
    pub fn new(source: Arc<dyn InferenceHealthSource>, timeout_threshold: u64) -> Self {
        Self {
            source,
            timeout_threshold,
        }
    }
}

#[async_trait::async_trait]
impl Sensor for InferenceHealthSensor {
    async fn observe(&self) -> Option<Signal> {
        let in_flight = self.source.in_flight().await;
        let max_concurrency = self.source.max_concurrency().await;
        let recent_timeouts = self.source.recent_timeout_count().await;

        // No data yet — the port hasn't been wired or no calls have been made.
        // Return None (not a signal with value 1.0, which would mask a broken
        // sensor as "healthy" — the `.rules` `unwrap_or(0)` trap).
        if max_concurrency == 0 {
            return None;
        }

        // Compute availability ratio. 1.0 = fully available (no in-flight
        // saturation, no recent timeouts). 0.0 = saturated or storming.
        let saturation_ratio = in_flight as f64 / max_concurrency as f64;
        let availability = if recent_timeouts >= self.timeout_threshold {
            // Storm detected — report 0.0 regardless of saturation.
            0.0
        } else if saturation_ratio >= 1.0 {
            // Saturated but not storming — report the headroom fraction.
            // When in_flight == max_concurrency, availability is 0.0.
            0.0
        } else {
            // Healthy — report 1.0 (no deviation).
            1.0
        };

        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::InferenceAvailable,
            availability,
            1.0, // set-point: fully available
        ))
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// CONTEXT SERVER HEALTH SENSOR
// ═════════════════════════════════════════════════════════════════════════════

/// Source of context-server health metrics for the cybernetics loop.
///
/// The bridge implements this trait and passes an
/// `Arc<dyn ContextServerHealthSource>` to
/// `CyberneticsLoop::set_context_server_health_source`.
///
/// Without this sensor, the cybernetics loop reports `signal_count=0`
/// while every MCP context server is stuck in `Starting` (spawned but
/// `initialize` never completing) or `Error`. The loop's existing sensors
/// read ledger/DB state, not context-server process state. This is the
/// same blind-feedback-loop class as `InferenceHealthSource` but for the
/// MCP stdio child processes spawned by zed's `ContextServerStore`.
#[async_trait::async_trait]
pub trait ContextServerHealthSource: Send + Sync {
    /// Number of registered context servers currently in a healthy state
    /// (`Running`). `0` when no servers are registered or none are healthy.
    async fn healthy_count(&self) -> usize;

    /// Total number of registered context servers (all states).
    /// `0` when no servers are registered.
    async fn total_count(&self) -> usize;
}

/// Senses context-server health from the per-project `ContextServerStore`.
///
/// Emits `SignalMetric::ContextServerHealth` with value `0.0` when any
/// registered server is stuck in `Starting` or `Error`. The set-point is
/// `1.0` (all registered servers Running); any deviation below `1.0` means
/// the context-server fleet is degraded.
///
/// This closes the blind-feedback-loop gap that caused `signal_count=0`
/// during the 600s `initialize` timeout storm: the cybernetics loop now
/// senses context-server health and can act on it (escalate, notify)
/// instead of reporting "no deviation" while every MCP server is hung.
pub(crate) struct ContextServerHealthSensor {
    source: Arc<dyn ContextServerHealthSource>,
}

impl ContextServerHealthSensor {
    pub fn new(source: Arc<dyn ContextServerHealthSource>) -> Self {
        Self { source }
    }
}

#[async_trait::async_trait]
impl Sensor for ContextServerHealthSensor {
    async fn observe(&self) -> Option<Signal> {
        let total = self.source.total_count().await;
        // No servers registered — nothing to report. Return None (not a
        // signal with value 1.0, which would mask a broken source as
        // "healthy" — the `.rules` `unwrap_or(0)` trap).
        if total == 0 {
            return None;
        }
        let healthy = self.source.healthy_count().await;

        // Health ratio: fraction of registered servers in a healthy state.
        // 1.0 = all Running, 0.0 = none Running.
        let health_ratio = healthy as f64 / total as f64;

        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::ContextServerHealth,
            health_ratio,
            1.0, // set-point: all registered servers Running
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OCR HEALTH SENSOR
// ═══════════════════════════════════════════════════════════════════════

/// Source of OCR silent-failure counts for the cybernetics loop.
///
/// The bridge implements this trait over the corpus MCP server's
/// cross-process health file and passes an `Arc<dyn OcrHealthSource>` to
/// `CyberneticsLoop::with_ocr_health_source`.
///
/// Without this sensor, the cybernetics loop reports `signal_count=0`
/// during an OCR silent-failure storm (a dead-but-responsive OCR endpoint
/// returning HTTP 200 with empty content on every Complex page) because
/// the `reg.pipeline.ocr.silent_failure` warns live in the corpus
/// subprocess's tracing — the loop's existing sensors read ledger/DB state
/// in the zed main process. This is the same blind-feedback-loop class as
/// `InferenceHealthSource`/`ContextServerHealthSource` but for a subprocess
/// whose events cross the process boundary via a health file.
/// The OCR health file is present but cannot be read or parsed — a broken
/// sensor, not a missing one (a missing file is the legitimate "no OCR has
/// run yet" state and surfaces as `Ok(0)`).
#[derive(Debug, thiserror::Error)]
pub enum OcrHealthError {
    #[error("OCR health file unreadable at {path}: {source}")]
    Unreadable {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("OCR health snapshot unparseable at {path}: {source}")]
    Unparseable {
        path: std::path::PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[async_trait::async_trait]
pub trait OcrHealthSource: Send + Sync {
    /// OCR silent failures (empty LLM output on a page) observed in the
    /// recent window. `Ok(0)` = none observed (or no health file yet — the
    /// legitimate "no OCR has run" state). `Err` = the health file is
    /// present but unreadable — a broken sensor, which the caller must
    /// `warn!` about, never collapse into `Ok(0)` (the `.rules`
    /// `unwrap_or(0)` trap: an unreadable file would read as "no
    /// deviation").
    async fn recent_silent_failures(&self) -> Result<u64, OcrHealthError>;
}

/// Senses OCR silent failures from the corpus server's health file.
///
/// Emits `SignalMetric::OcrSilentFailures` with the recent-window count when
/// any silent failures have been observed. The set-point is `0.0` — any
/// positive count is a deviation (an endpoint that returns empty content
/// for a page with text is failing, even once).
pub(crate) struct OcrHealthSensor {
    source: Arc<dyn OcrHealthSource>,
}

impl OcrHealthSensor {
    pub fn new(source: Arc<dyn OcrHealthSource>) -> Self {
        Self { source }
    }
}

#[async_trait::async_trait]
impl Sensor for OcrHealthSensor {
    async fn observe(&self) -> Option<Signal> {
        let count = match self.source.recent_silent_failures().await {
            Ok(count) => count,
            Err(error) => {
                // A broken sensor is not "no deviation" — warn so an
                // unreadable health file is distinguishable from a healthy
                // OCR pipeline (the `.rules` failure-signal rule).
                tracing::warn!(
                    target: "hkask.sensor.ocr",
                    error = %error,
                    "OcrHealthSensor: OCR health file unreadable — returning no signal (not 'no deviation')"
                );
                return None;
            }
        };
        // A real zero proves recovery; a missing reading does not.
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::OcrSilentFailures,
            count as f64,
            0.0, // set-point: no silent failures
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════
// MEMORY HEALTH SENSOR
// ═══════════════════════════════════════════════════════════════════════════

/// Source of memory health metrics for the cybernetics loop.
///
/// The regulation crate cannot depend on `hkask-memory` (it would create a
/// cycle), so the bridge implements this trait and passes an
/// `Arc<dyn MemoryHealthSource>` to `CyberneticsLoop::set_memory_health_source`.
///
/// Without this sensor, 5 memory regulation loops are blind — their policy
/// rules (`TripleCount`, `LowConfidenceCount`, `ConsolidationCandidates`,
/// `StorageUsage`, `MemoryLife`) can never fire because no signal is produced.
/// This is the `.rules` broken-feedback-loop pattern at structural scale.
#[async_trait::async_trait]
pub trait MemoryHealthSource: Send + Sync {
    /// Total h_mem count (valid h_mems only). `None` if the store is
    /// unavailable — the sensor returns `None` (not 0, which would mask a
    /// broken store as "empty but healthy").
    async fn h_mem_count(&self) -> Option<usize>;

    /// Count of h_mems at or below the given confidence threshold.
    async fn low_confidence_count(&self, threshold: f64) -> Option<usize>;

    /// Configured storage budget (max h_mems before consolidation prunes).
    async fn storage_budget(&self) -> usize;

    /// Configured memory life in days (the retention half-life parameter).
    async fn memory_life_days(&self) -> f64;
}

/// Senses memory health metrics from the memory store.
///
/// Emits signals for 5 `SignalMetric` variants that previously had policy
/// rules but no sensor:
/// - `TripleCount` — h_mem count above the set-point (too many h_mems)
/// - `LowConfidenceCount` — low-confidence h_mem count above the set-point
/// - `ConsolidationCandidates` — same count using the consolidation floor
/// - `StorageUsage` — h_mem count / storage budget ratio above the set-point
/// - `MemoryLife` — configured memory life days below the set-point (too short)
///
/// One registered sensor per metric reports both healthy and degraded states.
/// A busy metric must not hide another metric's recovery.
pub(crate) struct MemoryHealthSensor {
    source: Arc<dyn MemoryHealthSource>,
    metric: SignalMetric,
    /// Set-point: max h_mem count before `TripleCount` fires.
    triple_count_max: usize,
    /// Set-point: max low-confidence h_mem count before `LowConfidenceCount` fires.
    low_confidence_max: usize,
    /// Confidence threshold for `LowConfidenceCount`.
    low_confidence_threshold: f64,
    /// Confidence floor for `ConsolidationCandidates` (typically lower than
    /// `low_confidence_threshold` — these are deletion candidates, not just
    /// low-confidence).
    consolidation_floor: f64,
    /// Set-point: max consolidation candidates before `ConsolidationCandidates` fires.
    consolidation_candidates_max: usize,
    /// Set-point: storage usage ratio (h_mem_count / storage_budget) above
    /// which `StorageUsage` fires. 0.0–1.0.
    storage_usage_max_ratio: f64,
    /// Set-point: minimum memory life in days. Below this, `MemoryLife` fires.
    memory_life_min_days: f64,
}

impl MemoryHealthSensor {
    pub fn new(
        source: Arc<dyn MemoryHealthSource>,
        metric: SignalMetric,
        points: &crate::SetPoints,
    ) -> Self {
        Self {
            source,
            metric,
            triple_count_max: points.triple_count_max,
            low_confidence_max: points.low_confidence_max,
            low_confidence_threshold: points.low_confidence_threshold,
            consolidation_floor: points.consolidation_floor,
            consolidation_candidates_max: points.consolidation_candidates_max,
            storage_usage_max_ratio: points.storage_usage_max_ratio,
            memory_life_min_days: points.memory_life_min_days,
        }
    }
}

#[async_trait::async_trait]
impl Sensor for MemoryHealthSensor {
    async fn observe(&self) -> Option<Signal> {
        let (value, set_point) = match self.metric {
            // Configuration is observable even when the store is unavailable.
            SignalMetric::MemoryLife => (
                self.source.memory_life_days().await,
                self.memory_life_min_days,
            ),
            SignalMetric::TripleCount => (
                self.source.h_mem_count().await? as f64,
                self.triple_count_max as f64,
            ),
            SignalMetric::StorageUsage => {
                let budget = self.source.storage_budget().await;
                if budget == 0 {
                    return None;
                }
                (
                    self.source.h_mem_count().await? as f64 / budget as f64,
                    self.storage_usage_max_ratio,
                )
            }
            SignalMetric::LowConfidenceCount => (
                self.source
                    .low_confidence_count(self.low_confidence_threshold)
                    .await? as f64,
                self.low_confidence_max as f64,
            ),
            SignalMetric::ConsolidationCandidates => (
                self.source
                    .low_confidence_count(self.consolidation_floor)
                    .await? as f64,
                self.consolidation_candidates_max as f64,
            ),
            _ => {
                tracing::warn!(target: "reg.sensor", "Non-memory metric registered as memory sensor");
                return None;
            }
        };
        Some(Signal::new(
            LoopId::Cybernetics,
            self.metric,
            value,
            set_point,
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

    /// The variety feed is the tool-dispatch twin of the outcome feed: one
    /// increment per governed tool call, tool name as the observed state. An
    /// active domain exercising fewer distinct tools than expected emits a
    /// signal whose value is the live summed gap (a level — it can clear
    /// when the agent broadens its tool use). This is the rut detector.
    #[tokio::test]
    async fn variety_sensor_emits_live_deficit_for_active_rut() {
        let ledger = RegulationLedger::default();
        // One distinct tool on each of two active domains: per-domain gap
        // 3 − 1 = 2 (DEFAULT_EXPECTED_VARIETY is 3), summed to 4.
        ledger
            .increment_variety("hkask-mcp-media", "gallery_search")
            .await;
        ledger
            .increment_variety("hkask-mcp-companies", "stock_quote")
            .await;
        let sensor = VarietySensor::new(Arc::new(tokio::sync::RwLock::new(ledger)), 1.0);
        let signal = sensor.sense().await.expect("active rut must emit a signal");
        assert_eq!(signal.value, 4.0, "deficit is the summed live gap");
        assert_eq!(signal.set_point, 1.0);
    }

    /// Pins Fix 1: VarietySensor must return None when variety deficit is
    /// healthy (deficit <= set_point). Without the gate the sensor emits
    /// BelowSetPoint deviations for healthy variety levels, which no policy
    /// rule matches, leaving the loop open.
    #[tokio::test]
    async fn variety_sensor_returns_none_when_healthy() {
        let ledger = Arc::new(tokio::sync::RwLock::new(RegulationLedger::default()));
        let sensor = VarietySensor::new(ledger, 100.0);
        assert!(
            sensor.sense().await.is_none(),
            "healthy variety (deficit=0 <= set_point=100) returns None"
        );
    }

    // ── InferenceHealthSensor: closes the blind-feedback-loop gap ──────
    //
    // The cybernetics loop reported `signal_count=0` during the 300s
    // timeout storm because its existing sensors read ledger/DB state, not
    // inference dispatch state. The InferenceHealthSensor reads in-flight
    // count and recent timeouts from the inference port, emitting
    // SignalMetric::InferenceAvailable when the layer is saturated or
    // storming. These tests pin the sensor's behavior so a regression
    // (e.g. removing the saturation gate, or collapsing the timeout storm
    // check to a silent None) is caught.

    /// A mock `InferenceHealthSource` for testing the sensor in isolation.
    struct MockInferenceHealth {
        in_flight: usize,
        max_concurrency: usize,
        recent_timeouts: u64,
    }

    #[async_trait::async_trait]
    impl InferenceHealthSource for MockInferenceHealth {
        async fn in_flight(&self) -> usize {
            self.in_flight
        }
        async fn max_concurrency(&self) -> usize {
            self.max_concurrency
        }
        async fn recent_timeout_count(&self) -> u64 {
            self.recent_timeouts
        }
    }

    /// Healthy inference (no in-flight, no timeouts) returns None — the
    /// sensor stays silent when there's no deviation, matching the other
    /// sensors.
    #[tokio::test]
    async fn inference_health_sensor_returns_none_when_healthy() {
        let source = Arc::new(MockInferenceHealth {
            in_flight: 0,
            max_concurrency: 96,
            recent_timeouts: 0,
        });
        let sensor = InferenceHealthSensor::new(source, 3);
        assert!(
            sensor.sense().await.is_none(),
            "healthy inference (no in-flight, no timeouts) returns None"
        );
    }

    /// Saturated inference (in_flight >= max_concurrency) emits a signal
    /// with value 0.0 — the layer is fully saturated.
    #[tokio::test]
    async fn inference_health_sensor_emits_on_saturation() {
        let source = Arc::new(MockInferenceHealth {
            in_flight: 96,
            max_concurrency: 96,
            recent_timeouts: 0,
        });
        let sensor = InferenceHealthSensor::new(source, 3);
        let signal = sensor
            .sense()
            .await
            .expect("saturated inference must emit a signal");
        assert_eq!(signal.metric, SignalMetric::InferenceAvailable);
        assert_eq!(
            signal.value, 0.0,
            "saturated inference has availability 0.0"
        );
        assert_eq!(signal.set_point, 1.0);
    }

    /// Timeout storm (recent_timeouts >= threshold) emits a signal with
    /// value 0.0 — the layer is storming even if not fully saturated.
    #[tokio::test]
    async fn inference_health_sensor_emits_on_timeout_storm() {
        let source = Arc::new(MockInferenceHealth {
            in_flight: 2,
            max_concurrency: 96,
            recent_timeouts: 5,
        });
        let sensor = InferenceHealthSensor::new(source, 3);
        let signal = sensor
            .sense()
            .await
            .expect("timeout storm must emit a signal");
        assert_eq!(signal.metric, SignalMetric::InferenceAvailable);
        assert_eq!(signal.value, 0.0, "timeout storm has availability 0.0");
    }

    /// `max_concurrency == 0` returns None — the port hasn't been wired
    /// or no calls have been made. This is NOT a signal with value 1.0,
    /// which would mask a broken sensor as "healthy" (the `.rules`
    /// `unwrap_or(0)` trap).
    #[tokio::test]
    async fn inference_health_sensor_returns_none_when_max_concurrency_zero() {
        let source = Arc::new(MockInferenceHealth {
            in_flight: 0,
            max_concurrency: 0,
            recent_timeouts: 0,
        });
        let sensor = InferenceHealthSensor::new(source, 3);
        assert!(
            sensor.sense().await.is_none(),
            "max_concurrency=0 means no data — return None, not a signal masking a broken sensor"
        );
    }

    // ── ToolReliabilitySensor: pins the boundary semantics behind the
    // live-observed "tool_reliability_degraded — value 0 exceeds threshold 0"
    // escalation ──────────────────────────────────────────────────────────
    //
    // Three properties must hold for the alert to be trustworthy:
    // 1. A zero set-point disables the sensor (no alert can carry threshold 0
    //    from a configured floor) — and `SetPoints::validate` now rejects 0.0
    //    outright, so this is defense in depth.
    // 2. No tracked domains = no data, not 0% success — the alert can only
    //    fire after real tool calls, so the live alert was a TRUE positive
    //    (0% success in a real domain), not a startup false positive.
    // 3. When it does fire at 0% success against the 0.80 floor, the
    //    extracted (value, threshold) pair preserves the floor's magnitude —
    //    (0, 80) percent, never the truncated (0, 0).

    /// Property 1: a 0.0 set-point stays silent even with failing domains.
    #[tokio::test]
    async fn tool_reliability_sensor_returns_none_when_set_point_zero() {
        let ledger = RegulationLedger::default();
        ledger.record_outcome("media", false, None).await;
        let sensor = ToolReliabilitySensor::new(Arc::new(tokio::sync::RwLock::new(ledger)), 0.0);
        assert!(
            sensor.sense().await.is_none(),
            "set_point=0.0 makes every aggregate >= set_point — silent, not a (0, 0) alert"
        );
    }

    /// Property 2: no tracked outcomes is the legitimate no-data state.
    #[tokio::test]
    async fn tool_reliability_sensor_returns_none_with_no_tracked_domains() {
        let sensor = ToolReliabilitySensor::new(
            Arc::new(tokio::sync::RwLock::new(RegulationLedger::default())),
            0.80,
        );
        assert!(
            sensor.sense().await.is_none(),
            "no tracked domains = no data — None, not a 0%-success signal"
        );
    }

    /// Property 3: end-to-end — 0% success vs the 0.80 floor fires, and the
    /// extracted pair preserves the threshold's magnitude.
    #[tokio::test]
    async fn tool_reliability_alert_pair_preserves_threshold_magnitude() {
        let ledger = RegulationLedger::default();
        ledger.record_outcome("media", false, None).await;
        let sensor = ToolReliabilitySensor::new(Arc::new(tokio::sync::RwLock::new(ledger)), 0.80);
        let signal = sensor
            .sense()
            .await
            .expect("0% aggregate success must emit a signal");
        assert_eq!(signal.value, 0.0);
        assert_eq!(signal.set_point, 0.80);
        let data = crate::loops::RegulationData::ToolReliabilityDegraded {
            reliability: signal.value,
            threshold: signal.set_point,
        };
        assert_eq!(
            crate::regulation_policy::extract_deficit_threshold(&data),
            Some((0, 80)),
            "0% reliability vs the 0.80 floor must extract as (0, 80) percent — \
             the truncated (0, 0) is the live-observed false-positive appearance"
        );
    }

    // ── OcrHealthSensor: closes the subprocess-tracing blind-feedback gap ──

    struct MockOcrHealth {
        recent_count: u64,
        broken: bool,
    }

    #[async_trait::async_trait]
    impl OcrHealthSource for MockOcrHealth {
        async fn recent_silent_failures(&self) -> Result<u64, OcrHealthError> {
            if self.broken {
                Err(OcrHealthError::Unreadable {
                    path: std::path::PathBuf::from("mock-health.json"),
                    source: std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "health file unreadable",
                    ),
                })
            } else {
                Ok(self.recent_count)
            }
        }
    }

    #[tokio::test]
    async fn ocr_health_sensor_returns_none_when_healthy() {
        let sensor = OcrHealthSensor::new(Arc::new(MockOcrHealth {
            recent_count: 0,
            broken: false,
        }));
        assert!(
            sensor.sense().await.is_none(),
            "zero silent failures is healthy"
        );
    }

    #[tokio::test]
    async fn ocr_health_sensor_emits_on_silent_failure_storm() {
        let sensor = OcrHealthSensor::new(Arc::new(MockOcrHealth {
            recent_count: 14,
            broken: false,
        }));
        let signal = sensor
            .sense()
            .await
            .expect("a silent-failure storm must emit a signal");
        assert_eq!(signal.metric, SignalMetric::OcrSilentFailures);
        assert_eq!(signal.value, 14.0);
        assert_eq!(signal.set_point, 0.0);
    }

    /// A broken source (unreadable health file) must produce NO signal —
    /// never a fabricated 0, which would read as "no deviation" (the
    /// `.rules` `unwrap_or(0)` trap on sense inputs).
    #[tokio::test]
    async fn ocr_health_sensor_returns_none_on_broken_source() {
        let sensor = OcrHealthSensor::new(Arc::new(MockOcrHealth {
            recent_count: 14,
            broken: true,
        }));
        assert!(sensor.sense().await.is_none());
    }
}
