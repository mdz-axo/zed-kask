//! Sensor trait — pluggable metric sensors (Fermi Extractor pattern).
//!
//! Fermi's `Extractor` trait separates domain data extraction from the fitting
//! loop. Sensor applies the same pattern to hKask's regulation loop:
//! each metric gets its own `Sensor` implementation, registered with
//! a `SensorRegistry`. The `CyberneticsLoop::sense()` method walks the registry
//! instead of containing inline sensing logic.
//!
//! ## Why this lives in hkask-regulation
//!
//! Sensor providers are Regulation regulation infrastructure. They live alongside
//! `CyberneticsLoop`, `StagnationDetector`, and `SetPoints` in `hkask-regulation`,
//! the crate responsible for homeostatic self-regulation.
//!
//! ## Unified Sensor Catalog (v0.32.0)
//!
//! The `SensorRegistry` provides a single registration point for sensors
//! across ALL loops, not just Cybernetics. Each loop owns a `SensorRegistry`
//! for its local sensors, but the `SensorRegistry` tracks all of them for
//! monitoring, health checks, and dynamic registration. This eliminates the
//! fragmentation where each loop had inline `sense()` methods that couldn't
//! be discovered or managed from a central point.

use super::types::loops::{LoopId, Signal, SignalMetric};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// A pluggable sensor that produces one kind of signal metric.
///
/// Each implementation senses a single `SignalMetric` from its data source.
/// Fermi pattern: the `Extractor` trait takes a domain payload and produces
/// a scalar; `Sensor` takes system state and produces an optional
/// `Signal`. If the sensor has nothing to report (metric is healthy),
/// it returns `None`.
#[async_trait::async_trait]
pub trait Sensor: Send + Sync {
    /// Sense the current state and produce a signal if the metric is
    /// in a reportable state. Returns `None` if nothing to report.
    async fn sense(&self) -> Option<Signal>;

    /// The metric this sensor produces. Used for catalog indexing and
    /// deduplication. Default implementation returns `None` for backward
    /// compatibility with sensors that produce dynamic metrics.
    fn metric(&self) -> Option<SignalMetric> {
        None
    }

    /// Human-readable name for this sensor. Used in catalog listings and
    /// health checks. Default implementation returns the type name.
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    /// The loop this sensor is registered under. Used by the catalog to
    /// route signals to the correct loop's `sense()` call. Default
    /// implementation returns `None` for backward compatibility.
    fn loop_id(&self) -> Option<LoopId> {
        None
    }
}

/// Sensor bus for a single loop — actively walks sensors each tick.
///
/// Providers are registered at construction time and executed in order.
/// Order doesn't matter — each provider independently decides whether
/// to emit a signal. The bus aggregates their signals into a single
/// `Vec<Signal>` for the loop's `sense()` phase.
pub struct SensorBus {
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

    /// Number of registered providers.
    pub fn len(&self) -> usize {
        self.providers.lock().len()
    }

    /// Whether the registry has no providers.
    pub fn is_empty(&self) -> bool {
        self.providers.lock().is_empty()
    }

    /// List provider names for diagnostics.
    pub fn provider_names(&self) -> Vec<String> {
        self.providers
            .lock()
            .iter()
            .map(|p| p.name().to_string())
            .collect()
    }
}

impl Default for SensorBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry of all sensors across all loops in the system.
///
/// Provides a single registration point for sensors across ALL loops, not
/// just Cybernetics. Each loop owns a `SensorBus` for its local sensors,
/// but the `SensorRegistry` tracks all of them for monitoring, health checks,
/// and dynamic registration.
///
/// This eliminates the fragmentation where each loop had inline `sense()`
/// methods that couldn't be discovered or managed from a central point.
///
/// # Architecture
///
/// ```text
/// SensorRegistry (singleton, system-level)
/// ├── LoopId::Cybernetics → SensorBus
/// ├── LoopId::Inference   → SensorBus
/// ├── LoopId::Episodic    → SensorBus
/// ├── LoopId::Semantic    → SensorBus
/// └── LoopId::Curation    → SensorBus
/// ```
///
/// Each loop's `sense()` method calls `registry.sense_all(loop_id)` instead
/// of containing inline sensing logic. Sensors are registered at startup
/// via `registry.register_for(loop_id, provider)`.
pub struct SensorRegistry {
    /// Per-loop sensor buses. Each loop owns its own bus.
    registries: Mutex<HashMap<LoopId, SensorBus>>,
}

impl SensorRegistry {
    /// Create a new empty catalog.
    pub fn new() -> Self {
        Self {
            registries: Mutex::new(HashMap::new()),
        }
    }

    /// Get or create the sensor bus for a specific loop.
    pub fn bus_for(&self, loop_id: LoopId) -> SensorBus {
        let registries = self.registries.lock();
        registries
            .get(&loop_id)
            .cloned()
            .unwrap_or_else(SensorBus::new)
    }

    /// Register a sensor for a specific loop.
    pub fn register_for(&self, loop_id: LoopId, provider: Arc<dyn Sensor>) {
        let mut registries = self.registries.lock();
        registries.entry(loop_id).or_default().register(provider);
    }

    /// Sense all signals for a specific loop.
    pub async fn sense_all(&self, loop_id: LoopId) -> Vec<Signal> {
        let registry = {
            let registries = self.registries.lock();
            registries.get(&loop_id).cloned()
        };
        match registry {
            Some(reg) => reg.sense_all(loop_id).await,
            None => Vec::new(),
        }
    }

    /// Total number of sensors across all loops.
    pub fn total_sensors(&self) -> usize {
        self.registries.lock().values().map(|r| r.len()).sum()
    }

    /// List all sensor names grouped by loop.
    pub fn sensor_inventory(&self) -> Vec<(LoopId, Vec<String>)> {
        self.registries
            .lock()
            .iter()
            .map(|(loop_id, registry)| (*loop_id, registry.provider_names()))
            .collect()
    }

    /// Health check: which loops have no sensors registered?
    pub fn loops_without_sensors(&self) -> Vec<LoopId> {
        let registries = self.registries.lock();
        let all_loops = [
            LoopId::Inference,
            LoopId::Episodic,
            LoopId::Semantic,
            LoopId::Curation,
            LoopId::Cybernetics,
        ];
        all_loops
            .iter()
            .filter(|id| {
                !registries.contains_key(id) || registries.get(*id).is_none_or(|r| r.is_empty())
            })
            .copied()
            .collect()
    }
}

impl Default for SensorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SensorBus {
    fn clone(&self) -> Self {
        Self {
            providers: Mutex::new(self.providers.lock().clone()),
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// CONCRETE SENSOR PROVIDERS
// ═════════════════════════════════════════════════════════════════════════════

/// Senses energy budget remaining ratios across all agents.
///
/// Data source: `CallCapManager`. Produces a signal per agent.
pub struct EnergyBudgetSensor {
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
        Some(Signal::new(
            LoopId::Cybernetics, // placeholder — registry backfills
            SignalMetric::EnergyRemaining,
            worst,
            self.set_point,
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::EnergyRemaining)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Cybernetics)
    }
}

/// Senses variety deficit from the Regulation runtime.
///
/// Data source: `RegulationLedger`. Produces a single aggregate signal.
pub struct VarietySensor {
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
        Some(Signal::new(
            LoopId::Cybernetics, // placeholder — registry backfills
            SignalMetric::VarietyDeficit,
            health.overall_deficit as f64,
            self.set_point,
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::VarietyDeficit)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Cybernetics)
    }
}

/// Senses tool reliability across all MCP tools.
pub struct ToolReliabilitySensor {
    tool_stats: Arc<crate::tool_stats::ToolStats>,
    threshold: f64,
}

impl ToolReliabilitySensor {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new(tool_stats: Arc<crate::tool_stats::ToolStats>, threshold: f64) -> Self {
        Self {
            tool_stats,
            threshold,
        }
    }
}

#[async_trait::async_trait]
impl Sensor for ToolReliabilitySensor {
    async fn sense(&self) -> Option<Signal> {
        let alerts = self.tool_stats.reliability_alerts().await;
        if alerts.is_empty() {
            return None;
        }
        let worst = alerts
            .iter()
            .map(|a| a.success_probability)
            .fold(1.0, f64::min);
        Some(Signal::new(
            LoopId::Cybernetics, // placeholder — registry backfills
            SignalMetric::ToolReliability,
            worst,
            self.threshold,
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::ToolReliability)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Cybernetics)
    }
}

/// Error classifying why a trace-run metrics file could not be located.
///
/// Distinguishes I/O failures (the trace dir or a `metrics.json` is unreadable
/// — a broken sensor) from the legitimate "no run has produced metrics yet"
/// case, which is `Ok(None)`. Collapsing these into a single `None` masked
/// DB outages and permission errors as "no deviation," blinding the
/// regulation loop (the `.rules` `unwrap_or(0)` / `.ok()?` trap on sense
/// inputs). See `tool_stats::read_count_field` for the canonical warn-then-
/// fallback pattern this mirrors.
#[derive(Debug, thiserror::Error)]
pub enum MetricsLocateError {
    /// The trace directory itself could not be read (missing, permission
    /// denied, not a directory). The sensor cannot determine whether any run
    /// has metrics — this is a broken sensor, not an empty one.
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
/// Returns `Ok(None)` when the trace directory exists but contains no run
/// with a `metrics.json` (the legitimate "no metrics yet" case). Returns
/// `Err` for I/O failures so the caller can `warn!` and distinguish a broken
/// sensor from an empty one — collapsing the two into `None` made a DB outage
/// indistinguishable from "coverage meets set-point" (F1/F2).
///
/// Shared by `TestCoverageSensor` and `MutationScoreSensor`; extracting this
/// closes the byte-identical duplication and gives one place to enforce the
/// error-classification contract. Public so the error-classification contract
/// can be pinned by integration tests.
pub fn latest_run_metrics(
    trace_dir: &std::path::Path,
) -> Result<Option<std::path::PathBuf>, MetricsLocateError> {
    let entries =
        std::fs::read_dir(trace_dir).map_err(|error| MetricsLocateError::TraceDirInaccessible {
            path: trace_dir.to_path_buf(),
            error,
        })?;
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
/// Data source: the trace filesystem (`HKASK_TRACE_DIR`, default `kask/traces`).
/// Produces a signal only when `coverage_pct` is below the coverage floor.
pub struct TestCoverageSensor {
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

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::TestCoverage)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Cybernetics)
    }
}

/// Senses mutation score from the latest trace run's `metrics.json`.
///
/// Data source: the trace filesystem (`HKASK_TRACE_DIR`, default `kask/traces`).
/// Produces a signal only when `mutation_score` is below the mutation score floor.
pub struct MutationScoreSensor {
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

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::MutationScore)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Cybernetics)
    }
}

/// Senses grounding health from the central verification ledger.
///
/// Data source: `VerificationStore::grounding_trend(Global)`. Produces up to
/// three signals per tick:
///
/// - `GroundingCleanRate` — fraction of grounded delegations with zero
///   nulled fields. Encoded as -1.0 when no grounded delegations exist
///   (absence ≠ 0 — paper Rule 5.3). Signal fires only when the clean rate
///   drops below the floor.
/// - `GroundingCoverageRate` — fraction of delegations with a grounding
///   contract. Encoded as -1.0 when no delegations exist. Signal fires only
///   when the coverage rate drops below the floor.
/// - `GroundingViolationDelta` — change in `delegations_with_nulled` since
///   the last tick. Encoded as 0.0 on the first tick (no baseline). Signal
///   fires only when the delta is positive (new violations).
///
/// A DB outage is NOT collapsed to "no signal" — the sensor logs a `warn!`
/// naming the failure and returns no signals for that tick. The operator
/// can distinguish "not configured" from "configured but broken" (the
/// `.rules` broken-feedback-loop trap).
///
/// The sensor produces at most one signal per metric per tick (the
/// `Sensor::sense` trait returns `Option<Signal>`, so the loop registers
/// three `GroundingSensor` instances — one per metric — via
/// `GroundingSensor::new`). This follows the existing pattern where
/// each `Sensor` implementation produces a single `SignalMetric`.
pub struct GroundingSensor {
    verification_store: Arc<hkask_verification::VerificationStore>,
    metric: GroundingSensorMetric,
    clean_rate_floor: f64,
    coverage_rate_floor: f64,
    /// Previous tick's `delegations_with_nulled` count, for delta computation.
    /// `None` on the first tick (no baseline — delta is 0, not "no change").
    previous_nulled: parking_lot::Mutex<Option<u64>>,
    /// Set when the last `read_trend()` returned `None` (DB outage). On the
    /// next successful read, the violation-delta sensor suppresses the delta
    /// (it spans the outage, not a single tick) and resets this flag.
    was_outage: parking_lot::Mutex<bool>,
    /// Optional external delegation counter (e.g. the swarm ledger). When
    /// present, the liveness-gap sensor computes the true gap: external
    /// delegations minus verification-store records. When absent, the gap
    /// is 0.0 (honest: "no gap detected" because we can't measure it).
    /// Wrapped in a Mutex so the counter can be wired after construction
    /// (the `CyberneticsLoop` is built before the swarm server, which owns
    /// the ledger the counter reads).
    delegation_counter: parking_lot::Mutex<Option<Arc<dyn hkask_verification::DelegationCounter>>>,
}

/// Which grounding metric this sensor instance produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroundingSensorMetric {
    CleanRate,
    CoverageRate,
    ViolationDelta,
    LivenessGap,
}

impl GroundingSensor {
    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub fn new(
        verification_store: Arc<hkask_verification::VerificationStore>,
        metric: GroundingSensorMetric,
        clean_rate_floor: f64,
        coverage_rate_floor: f64,
    ) -> Self {
        Self {
            verification_store,
            metric,
            clean_rate_floor,
            coverage_rate_floor,
            previous_nulled: parking_lot::Mutex::new(None),
            was_outage: parking_lot::Mutex::new(false),
            delegation_counter: parking_lot::Mutex::new(None),
        }
    }

    /// Wire an external delegation counter (e.g. the swarm ledger). When
    /// present, the liveness-gap sensor computes the true gap: external
    /// delegations minus verification-store records. Without this, the
    /// sensor returns 0.0 (honest: "no gap detected" because we can't
    /// measure it).
    pub fn with_delegation_counter(
        self,
        counter: Arc<dyn hkask_verification::DelegationCounter>,
    ) -> Self {
        *self.delegation_counter.lock() = Some(counter);
        self
    }

    /// Wire a delegation counter on an already-constructed sensor. Used
    /// when the counter becomes available after the sensor is registered
    /// (e.g. the swarm ledger is opened in the deferred post-login task,
    /// after the `CyberneticsLoop` is already running).
    pub fn set_delegation_counter(&self, counter: Arc<dyn hkask_verification::DelegationCounter>) {
        *self.delegation_counter.lock() = Some(counter);
    }

    /// Read the trend from the verification ledger. Each sensor instance
    /// is a separate registered sensor, so this query runs once per sensor
    /// per tick — 3x per tick total (once for each metric variant). The
    /// `SensorBus::sense_all` iterates all registered sensors and calls
    /// `sense()` on each. A future optimization could share a cached trend
    /// result across the three instances within a single tick.
    fn read_trend(&self) -> Option<hkask_verification::GroundingTrendReport> {
        match self
            .verification_store
            .grounding_trend(&hkask_verification::TrendScope::Global)
        {
            Ok(report) => Some(report),
            Err(error) => {
                // DB outage — do NOT collapse to "no signal" (the `.rules`
                // broken-feedback-loop trap). Log the failure classification
                // so the operator can distinguish "not configured" from
                // "configured but broken."
                tracing::warn!(
                    target: "hkask.sensor.grounding",
                    error = %error,
                    metric = ?self.metric,
                    "GroundingSensor: verification ledger query failed — returning no signal (not 'no deviation'). Check that the verification DB is accessible and HKASK_VERIFICATION_PASSPHRASE is set."
                );
                None
            }
        }
    }

    /// Produce the `GroundingCleanRate` signal. Fires only when the clean
    /// rate drops below the floor. Encoded as -1.0 when no grounded
    /// delegations exist (absence ≠ 0).
    fn sense_clean_rate(
        &self,
        report: &hkask_verification::GroundingTrendReport,
    ) -> Option<Signal> {
        let clean_rate = report.clean_rate().unwrap_or(-1.0);
        // Only fire when below the floor AND we have measured delegations.
        // A -1.0 (no grounded delegations) is absence, not a violation —
        // the coverage sensor handles the "no contract" case.
        if clean_rate < 0.0 {
            return None;
        }
        if clean_rate >= self.clean_rate_floor {
            return None;
        }
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::GroundingCleanRate,
            clean_rate,
            self.clean_rate_floor,
        ))
    }

    /// Produce the `GroundingCoverageRate` signal. Fires only when the
    /// coverage rate drops below the floor. Encoded as -1.0 when no
    /// delegations exist.
    fn sense_coverage_rate(
        &self,
        report: &hkask_verification::GroundingTrendReport,
    ) -> Option<Signal> {
        let coverage_rate = report.coverage_rate().unwrap_or(-1.0);
        if coverage_rate < 0.0 {
            return None;
        }
        if coverage_rate >= self.coverage_rate_floor {
            return None;
        }
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::GroundingCoverageRate,
            coverage_rate,
            self.coverage_rate_floor,
        ))
    }

    /// Produce the `GroundingViolationDelta` signal. Fires only when the
    /// delta is positive (new nulled fields since the last tick). Zero on
    /// the first tick (no baseline — absence ≠ "no change").
    fn sense_violation_delta(
        &self,
        report: &hkask_verification::GroundingTrendReport,
    ) -> Option<Signal> {
        let current_nulled = report.delegations_with_nulled as u64;
        // Check and reset the outage flag before touching previous_nulled.
        let was_outage = {
            let mut guard = self.was_outage.lock();
            let was = *guard;
            *guard = false;
            was
        };
        let mut previous_guard = self.previous_nulled.lock();
        let previous = previous_guard.replace(current_nulled);
        // After a DB outage, the delta spans the entire outage — not a single
        // tick. Suppress it (update the baseline, return no signal) so the
        // operator isn't misdirected by a giant spike attributed to one tick.
        if was_outage {
            tracing::warn!(
                target: "hkask.sensor.grounding",
                "GroundingSensor: suppressing violation delta after DB outage recovery \
                 (delta spans the outage, not a single tick)"
            );
            return None;
        }
        let delta = match previous {
            Some(prev) => current_nulled as i64 - prev as i64,
            None => 0, // First tick — no baseline (absence ≠ "no change").
        };
        // Only fire when the delta is positive (new violations).
        if delta <= 0 {
            return None;
        }
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::GroundingViolationDelta,
            delta as f64,
            0.0, // Set-point is 0 — any positive delta is a deviation.
        ))
    }

    /// Produce the `GroundingLivenessGap` signal. Returns the count of
    /// delegations without grounding records. When a `DelegationCounter` is
    /// wired, computes the true gap: external delegations minus verification-
    /// store records. When no counter is wired, returns 0.0 (honest: "no gap
    /// detected" because we can't measure it). Returns `None` when the store
    /// is empty (absence ≠ 0 — can't distinguish "no delegations" from "no
    /// records") or when the counter query fails (absence ≠ 0).
    fn sense_liveness_gap(
        &self,
        report: &hkask_verification::GroundingTrendReport,
    ) -> Option<Signal> {
        if report.total_delegations == 0 {
            return None;
        }
        let gap = match self.delegation_counter.lock().as_ref() {
            Some(counter) => {
                let total = counter.delegation_count()?;
                total.saturating_sub(report.total_delegations as u64) as f64
            }
            None => 0.0, // Can't measure — honest: "no gap detected"
        };
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::GroundingLivenessGap,
            gap,
            0.0,
        ))
    }
}

#[async_trait::async_trait]
impl Sensor for GroundingSensor {
    async fn sense(&self) -> Option<Signal> {
        let report = match self.read_trend() {
            Some(r) => r,
            None => {
                *self.was_outage.lock() = true;
                return None;
            }
        };
        match self.metric {
            GroundingSensorMetric::CleanRate => self.sense_clean_rate(&report),
            GroundingSensorMetric::CoverageRate => self.sense_coverage_rate(&report),
            GroundingSensorMetric::ViolationDelta => self.sense_violation_delta(&report),
            GroundingSensorMetric::LivenessGap => self.sense_liveness_gap(&report),
        }
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(match self.metric {
            GroundingSensorMetric::CleanRate => SignalMetric::GroundingCleanRate,
            GroundingSensorMetric::CoverageRate => SignalMetric::GroundingCoverageRate,
            GroundingSensorMetric::ViolationDelta => SignalMetric::GroundingViolationDelta,
            GroundingSensorMetric::LivenessGap => SignalMetric::GroundingLivenessGap,
        })
    }

    fn name(&self) -> &str {
        match self.metric {
            GroundingSensorMetric::CleanRate => "GroundingSensor(CleanRate)",
            GroundingSensorMetric::CoverageRate => "GroundingSensor(CoverageRate)",
            GroundingSensorMetric::ViolationDelta => "GroundingSensor(ViolationDelta)",
            GroundingSensorMetric::LivenessGap => "GroundingSensor(LivenessGap)",
        }
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Cybernetics)
    }
}

#[cfg(test)]
mod grounding_sensor_tests {
    use super::*;
    use hkask_verification::VerificationStore;

    /// Build an in-memory store with `n_clean` clean delegations and
    /// `n_nulled` delegations-with-nulled for the `"task"` agent_type.
    fn store_with_delegations(n_clean: usize, n_nulled: usize) -> Arc<VerificationStore> {
        let store = Arc::new(VerificationStore::in_memory());
        let clean_output = serde_json::json!({
            "deliverable_path": "/src/lib.rs",
            "summary": "did the work",
            "approach": "directly",
        });
        let clean_tools = vec![serde_json::json!({
            "tool": "zed/edit_file",
            "ok": true,
            "result": {"path": "/src/lib.rs"},
        })];
        for _ in 0..n_clean {
            store.enforce_for_agent(
                "kanban_task_spawn",
                "task_agent",
                "task",
                &clean_output,
                &clean_tools,
                &clean_output.to_string(),
                &[],
            );
        }
        let nulled_output = serde_json::json!({
            "deliverable_path": "/src/fabricated.rs",
            "summary": "did the work",
            "approach": "directly",
        });
        let nulled_tools: Vec<serde_json::Value> = vec![];
        for _ in 0..n_nulled {
            store.enforce_for_agent(
                "kanban_task_spawn",
                "task_agent",
                "task",
                &nulled_output,
                &nulled_tools,
                &nulled_output.to_string(),
                &[],
            );
        }
        store
    }

    #[tokio::test]
    async fn clean_rate_signal_fires_when_below_floor() {
        let store = store_with_delegations(1, 1);
        let sensor = GroundingSensor::new(store, GroundingSensorMetric::CleanRate, 0.8, 0.5);
        let signal = sensor.sense().await.expect("signal must fire below floor");
        assert_eq!(signal.metric, SignalMetric::GroundingCleanRate);
        assert!((signal.value - 0.5).abs() < 1e-9);
        assert!((signal.set_point - 0.8).abs() < 1e-9);
    }

    #[tokio::test]
    async fn clean_rate_signal_does_not_fire_when_above_floor() {
        let store = store_with_delegations(9, 1);
        let sensor = GroundingSensor::new(store, GroundingSensorMetric::CleanRate, 0.8, 0.5);
        assert!(sensor.sense().await.is_none(), "no signal above floor");
    }

    #[tokio::test]
    async fn clean_rate_signal_absence_is_not_zero() {
        let store = Arc::new(VerificationStore::in_memory());
        let sensor = GroundingSensor::new(store, GroundingSensorMetric::CleanRate, 0.8, 0.5);
        assert!(
            sensor.sense().await.is_none(),
            "absence must not fire a clean_rate signal (paper Rule 5.3)"
        );
    }

    #[tokio::test]
    async fn coverage_rate_signal_fires_when_below_floor() {
        let store = Arc::new(VerificationStore::in_memory());
        let clean_output = serde_json::json!({"summary": "x"});
        store.enforce_for_agent(
            "kanban_task_spawn",
            "task_agent",
            "task",
            &clean_output,
            &[],
            &clean_output.to_string(),
            &[],
        );
        store.enforce_for_agent(
            "swarm_delegate_local",
            "custom_agent",
            "unknown_agent_type",
            &clean_output,
            &[],
            &clean_output.to_string(),
            &[],
        );
        let sensor = GroundingSensor::new(store, GroundingSensorMetric::CoverageRate, 0.8, 0.6);
        let signal = sensor.sense().await.expect("signal must fire below floor");
        assert_eq!(signal.metric, SignalMetric::GroundingCoverageRate);
        assert!((signal.value - 0.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn coverage_rate_signal_absence_is_not_zero() {
        let store = Arc::new(VerificationStore::in_memory());
        let sensor = GroundingSensor::new(store, GroundingSensorMetric::CoverageRate, 0.8, 0.5);
        assert!(
            sensor.sense().await.is_none(),
            "absence must not fire a coverage_rate signal (paper Rule 5.3)"
        );
    }

    #[tokio::test]
    async fn violation_delta_fires_on_increase() {
        let store = store_with_delegations(0, 3);
        let store_for_additions = store.clone();
        let sensor = GroundingSensor::new(store, GroundingSensorMetric::ViolationDelta, 0.8, 0.5);
        assert!(sensor.sense().await.is_none(), "first tick: no baseline");
        let nulled_output = serde_json::json!({
            "deliverable_path": "/src/fabricated2.rs",
            "summary": "did the work",
            "approach": "directly",
        });
        store_for_additions.enforce_for_agent(
            "kanban_task_spawn",
            "task_agent",
            "task",
            &nulled_output,
            &[],
            &nulled_output.to_string(),
            &[],
        );
        let signal = sensor.sense().await.expect("delta must fire on increase");
        assert_eq!(signal.metric, SignalMetric::GroundingViolationDelta);
        assert!((signal.value - 1.0).abs() < 1e-9);
        assert!((signal.set_point - 0.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn violation_delta_does_not_fire_on_decrease() {
        let store = store_with_delegations(0, 3);
        let sensor = GroundingSensor::new(store, GroundingSensorMetric::ViolationDelta, 0.8, 0.5);
        assert!(sensor.sense().await.is_none(), "first tick: no baseline");
        assert!(
            sensor.sense().await.is_none(),
            "second tick: delta 0 (stable) — no signal"
        );
    }

    #[tokio::test]
    async fn violation_delta_suppressed_after_outage_recovery() {
        // After a DB outage (read_trend returns None), the first successful
        // read suppresses the delta (it spans the outage, not a single tick).
        // The second successful read resumes normal operation.
        //
        // The in-memory store always succeeds, so we simulate the outage by
        // directly setting the `was_outage` flag — the same flag `sense()`
        // sets when `read_trend()` returns None.
        let store = store_with_delegations(0, 3);
        let store_for_additions = store.clone();
        let sensor = GroundingSensor::new(store, GroundingSensorMetric::ViolationDelta, 0.8, 0.5);

        // was_outage must be initialized to false.
        assert!(!*sensor.was_outage.lock(), "was_outage must start false");

        // First tick: establishes the baseline (no signal — absence ≠ change).
        assert!(sensor.sense().await.is_none(), "first tick: no baseline");

        // Simulate a DB outage: `sense()` would set was_outage = true when
        // read_trend() returns None. We set it directly to emulate that path.
        *sensor.was_outage.lock() = true;

        // Add a nulled delegation during the "outage" — the delta would be +1
        // if it were not suppressed.
        let nulled_output = serde_json::json!({
            "deliverable_path": "/src/fabricated_outage.rs",
            "summary": "did the work",
            "approach": "directly",
        });
        store_for_additions.enforce_for_agent(
            "kanban_task_spawn",
            "task_agent",
            "task",
            &nulled_output,
            &[],
            &nulled_output.to_string(),
            &[],
        );

        // First read after recovery: delta is suppressed (spans the outage).
        assert!(
            sensor.sense().await.is_none(),
            "delta after outage recovery must be suppressed"
        );
        assert!(
            !*sensor.was_outage.lock(),
            "was_outage must reset after recovery"
        );

        // Add another nulled delegation — this delta is a real single-tick jump.
        store_for_additions.enforce_for_agent(
            "kanban_task_spawn",
            "task_agent",
            "task",
            &nulled_output,
            &[],
            &nulled_output.to_string(),
            &[],
        );

        // Second read after recovery: normal operation resumes — delta fires.
        let signal = sensor
            .sense()
            .await
            .expect("delta must fire on real increase after recovery");
        assert_eq!(signal.metric, SignalMetric::GroundingViolationDelta);
        assert!((signal.value - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn sensor_metric_and_loop_id_are_correct() {
        let store = Arc::new(VerificationStore::in_memory());
        for metric in [
            GroundingSensorMetric::CleanRate,
            GroundingSensorMetric::CoverageRate,
            GroundingSensorMetric::ViolationDelta,
            GroundingSensorMetric::LivenessGap,
        ] {
            let sensor = GroundingSensor::new(store.clone(), metric, 0.8, 0.5);
            assert!(sensor.metric().is_some(), "metric must be Some");
            assert_eq!(sensor.loop_id(), Some(LoopId::Cybernetics));
        }
    }

    #[test]
    fn liveness_gap_returns_none_when_store_empty() {
        let store = Arc::new(VerificationStore::in_memory());
        let sensor =
            GroundingSensor::new(store.clone(), GroundingSensorMetric::LivenessGap, 0.0, 0.0);
        let report = store
            .grounding_trend(&hkask_verification::TrendScope::Global)
            .unwrap();
        let signal = sensor.sense_liveness_gap(&report);
        assert!(signal.is_none(), "empty store: absence ≠ 0");
    }

    #[test]
    fn liveness_gap_returns_zero_when_records_exist() {
        let store = Arc::new(VerificationStore::in_memory());
        // Seed one clean delegation.
        let output = serde_json::json!({"deliverable_path": "/src/main.rs", "summary": "done"});
        let tool_calls = vec![
            serde_json::json!({"tool": "zed/write_file", "ok": true, "result": {"path": "/src/main.rs"}}),
        ];
        store.enforce_for_agent(
            "kanban_task_spawn",
            "test_agent",
            "task",
            &output,
            &tool_calls,
            "",
            &[],
        );
        let sensor =
            GroundingSensor::new(store.clone(), GroundingSensorMetric::LivenessGap, 0.0, 0.0);
        let report = store
            .grounding_trend(&hkask_verification::TrendScope::Global)
            .unwrap();
        let signal = sensor.sense_liveness_gap(&report);
        assert!(signal.is_some());
        assert_eq!(
            signal.unwrap().value,
            0.0,
            "no gap detected (infrastructure only)"
        );
    }

    /// A mock `DelegationCounter` that returns a fixed count. Used by the
    /// liveness-gap tests to verify the true-gap computation.
    struct MockDelegationCounter {
        count: Option<u64>,
    }

    impl hkask_verification::DelegationCounter for MockDelegationCounter {
        fn delegation_count(&self) -> Option<u64> {
            self.count
        }
    }

    #[test]
    fn liveness_gap_detects_missing_records_when_counter_wired() {
        // When the counter reports more delegations than the verification
        // store has records, the gap is the difference. This catches
        // delegations that skipped `enforce_for_agent`.
        let store = Arc::new(VerificationStore::in_memory());
        // Seed 8 clean delegations.
        let output = serde_json::json!({"deliverable_path": "/src/main.rs", "summary": "done"});
        let tool_calls = vec![
            serde_json::json!({"tool": "zed/write_file", "ok": true, "result": {"path": "/src/main.rs"}}),
        ];
        for _ in 0..8 {
            store.enforce_for_agent(
                "kanban_task_spawn",
                "test_agent",
                "task",
                &output,
                &tool_calls,
                "",
                &[],
            );
        }
        // The swarm ledger reports 10 delegations — 2 skipped grounding.
        let counter = Arc::new(MockDelegationCounter { count: Some(10) });
        let sensor =
            GroundingSensor::new(store.clone(), GroundingSensorMetric::LivenessGap, 0.0, 0.0)
                .with_delegation_counter(counter);
        let report = store
            .grounding_trend(&hkask_verification::TrendScope::Global)
            .unwrap();
        let signal = sensor.sense_liveness_gap(&report);
        let signal = signal.expect("signal must fire when records exist");
        assert_eq!(signal.value, 2.0, "gap = 10 (ledger) - 8 (store) = 2");
    }

    #[test]
    fn liveness_gap_returns_zero_when_counter_absent() {
        // Without a counter wired, the sensor returns 0.0 — honest: "no
        // gap detected" because we can't measure it.
        let store = Arc::new(VerificationStore::in_memory());
        let output = serde_json::json!({"deliverable_path": "/src/main.rs", "summary": "done"});
        let tool_calls = vec![
            serde_json::json!({"tool": "zed/write_file", "ok": true, "result": {"path": "/src/main.rs"}}),
        ];
        store.enforce_for_agent(
            "kanban_task_spawn",
            "test_agent",
            "task",
            &output,
            &tool_calls,
            "",
            &[],
        );
        let sensor =
            GroundingSensor::new(store.clone(), GroundingSensorMetric::LivenessGap, 0.0, 0.0);
        let report = store
            .grounding_trend(&hkask_verification::TrendScope::Global)
            .unwrap();
        let signal = sensor.sense_liveness_gap(&report);
        let signal = signal.expect("signal must fire when records exist");
        assert_eq!(
            signal.value, 0.0,
            "no counter wired — honest: no gap detected"
        );
    }

    #[test]
    fn liveness_gap_returns_none_when_counter_fails() {
        // When the counter query fails (returns None), the sensor returns
        // None — absence ≠ 0 (a failed read is not a measured zero).
        let store = Arc::new(VerificationStore::in_memory());
        let output = serde_json::json!({"deliverable_path": "/src/main.rs", "summary": "done"});
        let tool_calls = vec![
            serde_json::json!({"tool": "zed/write_file", "ok": true, "result": {"path": "/src/main.rs"}}),
        ];
        store.enforce_for_agent(
            "kanban_task_spawn",
            "test_agent",
            "task",
            &output,
            &tool_calls,
            "",
            &[],
        );
        // Counter query fails — returns None.
        let counter = Arc::new(MockDelegationCounter { count: None });
        let sensor =
            GroundingSensor::new(store.clone(), GroundingSensorMetric::LivenessGap, 0.0, 0.0)
                .with_delegation_counter(counter);
        let report = store
            .grounding_trend(&hkask_verification::TrendScope::Global)
            .unwrap();
        let signal = sensor.sense_liveness_gap(&report);
        assert!(
            signal.is_none(),
            "counter query failed — absence ≠ 0 (failed read is not a measured zero)"
        );
    }

    // ── Proptests ──

    /// Generate a random delegation action: a mix of clean, nulled, and
    /// coverage-gap delegations. Used by the proptests to verify the sensor
    /// never panics and produces consistent signals.
    fn arb_delegation_sequence()
    -> impl proptest::prelude::Strategy<Value = Vec<(usize, usize, usize)>> {
        // (n_clean_task, n_nulled_task, n_research_coverage_gap)
        proptest::prelude::prop::collection::vec((0usize..20, 0usize..20, 0usize..20), 1..5)
    }

    /// Build a store from a delegation sequence: each element is
    /// (n_clean, n_nulled, n_coverage_gap). All delegations are enforced
    /// on the same store.
    fn store_from_sequence(sequence: &[(usize, usize, usize)]) -> Arc<VerificationStore> {
        let store = Arc::new(VerificationStore::in_memory());
        let clean_output = serde_json::json!({
            "deliverable_path": "/src/lib.rs",
            "summary": "did the work",
            "approach": "directly",
        });
        let clean_tools = vec![serde_json::json!({
            "tool": "zed/edit_file",
            "ok": true,
            "result": {"path": "/src/lib.rs"},
        })];
        let nulled_output = serde_json::json!({
            "deliverable_path": "/src/fabricated.rs",
            "summary": "did the work",
            "approach": "directly",
        });
        let gap_output = serde_json::json!({"summary": "research result"});
        for &(n_clean, n_nulled, n_gap) in sequence {
            for _ in 0..n_clean {
                store.enforce_for_agent(
                    "kanban_task_spawn",
                    "task_agent",
                    "task",
                    &clean_output,
                    &clean_tools,
                    &clean_output.to_string(),
                    &[],
                );
            }
            for _ in 0..n_nulled {
                store.enforce_for_agent(
                    "kanban_task_spawn",
                    "task_agent",
                    "task",
                    &nulled_output,
                    &[],
                    &nulled_output.to_string(),
                    &[],
                );
            }
            for _ in 0..n_gap {
                store.enforce_for_agent(
                    "swarm_delegate_local",
                    "researcher",
                    "research",
                    &gap_output,
                    &[],
                    &gap_output.to_string(),
                    &[],
                );
            }
        }
        store
    }

    proptest::proptest! {
        /// The sensor must never panic across random delegation sequences.
        /// Covers the no-delegations, all-clean, all-nulled, and mixed cases.
        #[test]
        fn grounding_sensor_never_panics(sequence in arb_delegation_sequence()) {
            let store = store_from_sequence(&sequence);
            for metric in [
                GroundingSensorMetric::CleanRate,
                GroundingSensorMetric::CoverageRate,
                GroundingSensorMetric::ViolationDelta,
                GroundingSensorMetric::LivenessGap,
            ] {
                let sensor = GroundingSensor::new(store.clone(), metric, 0.8, 0.5);
                // block_on is fine here — this is a sync proptest, not a tokio test.
                // The sensor's sense() is async only because the trait requires it;
                // the actual work is sync (HMemStore queries are blocking).
                let rt = tokio::runtime::Runtime::new().unwrap();
                let _ = rt.block_on(sensor.sense());
            }
        }

        /// The clean_rate signal value must match GroundingTrendReport::clean_rate()
        /// for any delegation sequence. The sensor is a thin wrapper — its
        /// signal value must equal the report's computed rate (or be absent
        /// when the rate is None or above the floor).
        #[test]
        fn clean_rate_signal_matches_report(sequence in arb_delegation_sequence()) {
            let store = store_from_sequence(&sequence);
            let report = store
                .grounding_trend(&hkask_verification::TrendScope::Global)
                .expect("trend query must succeed on in-memory store");
            let sensor = GroundingSensor::new(
                store, GroundingSensorMetric::CleanRate, 0.8, 0.5,
            );
            let rt = tokio::runtime::Runtime::new().unwrap();
            let signal = rt.block_on(sensor.sense());
            match (signal, report.clean_rate()) {
                (None, _) => {
                    // No signal: either clean_rate is None (no grounded
                    // delegations) or clean_rate >= floor.
                    let cr = report.clean_rate();
                    assert!(
                        cr.is_none() || cr.unwrap() >= 0.8,
                        "signal absent but clean_rate {:?} is below floor 0.8",
                        cr
                    );
                }
                (Some(sig), Some(expected)) => {
                    assert!((sig.value - expected).abs() < 1e-9,
                        "signal value {} != report clean_rate {}",
                        sig.value, expected);
                    assert!(expected < 0.8, "signal fired but clean_rate above floor");
                }
                (Some(_), None) => {
                    panic!("signal fired but report clean_rate is None (no grounded delegations)");
                }
            }
        }

        /// The violation delta must never fire on the first tick (no baseline).
        /// On the second tick, the delta must equal current - previous and
        /// must be non-negative when fired.
        #[test]
        fn violation_delta_never_fires_on_first_tick(sequence in arb_delegation_sequence()) {
            let store = store_from_sequence(&sequence);
            let sensor = GroundingSensor::new(
                store, GroundingSensorMetric::ViolationDelta, 0.8, 0.5,
            );
            let rt = tokio::runtime::Runtime::new().unwrap();
            // First tick: no baseline → must be None.
            let first = rt.block_on(sensor.sense());
            assert!(first.is_none(), "first tick must not fire (no baseline)");
            // Second tick: same store, same delegations → delta is 0 → None.
            let second = rt.block_on(sensor.sense());
            assert!(second.is_none(), "second tick with no new delegations must not fire (delta 0)");
        }
    }
}
