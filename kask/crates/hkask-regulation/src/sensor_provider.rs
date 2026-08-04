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

    /// Find the run directory whose `metrics.json` was most recently modified.
    /// Returns `None` if the trace dir is missing or no run has a `metrics.json`.
    fn latest_metrics_path(&self) -> Option<std::path::PathBuf> {
        let entries = std::fs::read_dir(&self.trace_dir).ok()?;
        let mut newest: Option<(std::path::PathBuf, std::time::SystemTime)> = None;
        for entry in entries.flatten() {
            let metrics = entry.path().join("metrics.json");
            if !metrics.is_file() {
                continue;
            }
            let modified = std::fs::metadata(&metrics)
                .and_then(|m| m.modified())
                .ok()?;
            match &newest {
                Some((_, best)) if &modified <= best => {}
                _ => newest = Some((metrics, modified)),
            }
        }
        newest.map(|(p, _)| p)
    }
}

#[async_trait::async_trait]
impl Sensor for TestCoverageSensor {
    async fn sense(&self) -> Option<Signal> {
        let path = self.latest_metrics_path()?;
        let contents = std::fs::read_to_string(&path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&contents).ok()?;
        let coverage = value.get("coverage_pct")?.as_f64()?;
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

    /// Find the run directory whose `metrics.json` was most recently modified.
    /// Returns `None` if the trace dir is missing or no run has a `metrics.json`.
    fn latest_metrics_path(&self) -> Option<std::path::PathBuf> {
        let entries = std::fs::read_dir(&self.trace_dir).ok()?;
        let mut newest: Option<(std::path::PathBuf, std::time::SystemTime)> = None;
        for entry in entries.flatten() {
            let metrics = entry.path().join("metrics.json");
            if !metrics.is_file() {
                continue;
            }
            let modified = std::fs::metadata(&metrics)
                .and_then(|m| m.modified())
                .ok()?;
            match &newest {
                Some((_, best)) if &modified <= best => {}
                _ => newest = Some((metrics, modified)),
            }
        }
        newest.map(|(p, _)| p)
    }
}

#[async_trait::async_trait]
impl Sensor for MutationScoreSensor {
    async fn sense(&self) -> Option<Signal> {
        let path = self.latest_metrics_path()?;
        let contents = std::fs::read_to_string(&path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&contents).ok()?;
        let score = value.get("mutation_score")?.as_f64()?;
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
