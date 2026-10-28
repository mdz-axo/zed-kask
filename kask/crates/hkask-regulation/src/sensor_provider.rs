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
    providers: Mutex<Vec<(Option<SignalMetric>, Arc<dyn Sensor>)>>,
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
        self.providers.lock().push((None, provider));
    }

    /// Replace a late-wired provider for one metric instead of retaining stale
    /// sources across model or store rewires.
    pub fn replace(&self, metric: SignalMetric, provider: Arc<dyn Sensor>) {
        let mut providers = self.providers.lock();
        providers.retain(|(registered_metric, _)| *registered_metric != Some(metric));
        providers.push((Some(metric), provider));
    }

    /// expect: "The system provides pluggable metric sensing for the cybernetic regulation loop"
    pub async fn sense_all(&self, source: LoopId) -> Vec<Signal> {
        let providers: Vec<Arc<dyn Sensor>> = self
            .providers
            .lock()
            .iter()
            .map(|(_, provider)| Arc::clone(provider))
            .collect();
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

/// Senses tool reliability from the Regulation runtime's outcome tracker.
///
/// Data source: `RegulationLedger::outcome_breakdown`. Returns the aggregate
/// success rate across domains with current samples, including healthy readings.
/// This closes the feedback loop that was
/// blind to systematic tool failures (e.g. MCP server timeouts looping for
/// minutes without the regulation loop sensing the deviation).
///
/// Returns `None` when no outcomes have been recorded yet, or when every
/// tracked domain is below `TOOL_RELIABILITY_MIN_DOMAIN_SAMPLES` (the
/// legitimate "no data" states) — not a signal with value 1.0, which would
/// mask a broken sensor as "healthy" (the `.rules` `unwrap_or(0)` trap).
pub(crate) struct ToolReliabilitySensor {
    ledger: Arc<tokio::sync::RwLock<super::runtime::RegulationLedger>>,
    set_point: f64,
}

/// Minimum in-window operations a domain must have before its success rate
/// counts toward the aggregate — the same five-operation minimum
/// `RegulationLedger::check_outcome` applies to its outcome alerts. Below
/// it, a domain's rate is small-sample noise: the live-observed
/// `tool_reliability` deviations at 0.5 (one success in two calls), 0.6667
/// (two in three), and 0.75 (three in four) all came from quiet windows
/// where a couple of failures were the entire sample, and each fired a
/// `ToolReliabilityDegraded` escalation. A domain below the floor is
/// excluded from the aggregate; when every domain is below it, the sensor
/// returns no data.
pub(crate) const TOOL_RELIABILITY_MIN_DOMAIN_SAMPLES: u64 = 5;

/// The equal-weighted aggregate success rate across domains meeting the
/// minimum-sample floor. Returns `None` when no domain meets the floor (the
/// no-data state, not 0% — the `.rules` `unwrap_or(0)` trap).
pub(crate) fn aggregate_tool_reliability(
    breakdown: &[super::runtime::DomainOutcomeSnapshot],
) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0;
    for snapshot in breakdown {
        if snapshot.total_operations < TOOL_RELIABILITY_MIN_DOMAIN_SAMPLES {
            continue;
        }
        if let Some(rate) = snapshot.success_rate {
            sum += rate;
            count += 1;
        }
    }
    (count > 0).then(|| sum / count as f64)
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
        let breakdown = {
            let ledger = self.ledger.read().await;
            ledger.outcome_breakdown().await
        };
        // Equal-weighted aggregate across domains meeting the minimum-sample
        // floor (small-sample domains are noise, not signal); a domain with
        // zero operations falls out by the same floor (no data, not 0%
        // success).
        let aggregate = aggregate_tool_reliability(&breakdown)?;
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::ToolReliability,
            aggregate,
            self.set_point,
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// NATIVE-FIRST RATE SENSOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Senses the native-first rate — the fraction of completed substantive
/// agent turns that used zero kask instruments — from the Regulation
/// runtime's rolling turn window.
///
/// Data source: `RegulationLedger::record_agent_turn`, fed by the zed-side
/// agent loop at turn completion (the same seam family as
/// `record_mcp_tool_outcome`). This closes the drift feedback loop's
/// unsensed leg: `ToolReliabilitySensor` senses tool FAILURES, but nothing
/// sensed instrument ABSENCE — a session that completes every substantive
/// turn from native generation alone read as "healthy" because no tool
/// ever failed.
///
/// Returns `None` when no substantive turns have been recorded (the
/// legitimate "no data" state) — not a signal with value 0.0, which would
/// mask a broken sensor as "perfect compliance" (the `.rules`
/// `unwrap_or(0)` trap).
pub(crate) struct NativeFirstRateSensor {
    ledger: Arc<tokio::sync::RwLock<super::runtime::RegulationLedger>>,
    set_point: f64,
}

impl NativeFirstRateSensor {
    pub fn new(
        ledger: Arc<tokio::sync::RwLock<super::runtime::RegulationLedger>>,
        set_point: f64,
    ) -> Self {
        Self { ledger, set_point }
    }
}

#[async_trait::async_trait]
impl Sensor for NativeFirstRateSensor {
    async fn observe(&self) -> Option<Signal> {
        let rate = {
            let ledger = self.ledger.read().await;
            ledger.native_first_rate().await
        };
        let (rate, _samples) = rate?;
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::NativeFirstRate,
            rate,
            self.set_point,
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CONTEXT SERVER HEALTH SENSOR
// ═══════════════════════════════════════════════════════════════════════════════

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
/// same blind-feedback-loop class as inference resilience observation, but
/// for MCP stdio child processes spawned by zed's `ContextServerStore`.
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
/// The inference/context-health pattern adapted for a subprocess whose events
/// cross the process boundary via a health file.
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
/// Without this sensor, 4 memory regulation loops are blind — their policy
/// rules (`TripleCount`, `LowConfidenceCount`, `ConsolidationCandidates`,
/// `MemoryLife`) can never fire because no signal is produced.
/// This is the `.rules` broken-feedback-loop pattern at structural scale.
#[async_trait::async_trait]
pub trait MemoryHealthSource: Send + Sync {
    /// Count of h_mems in the sheddable tiers — turn storage
    /// (`curator:thread:`) plus goal events (`curator:goal:`) — the two tiers
    /// with forgetting valves. The additive-by-design knowledge layer is
    /// excluded (operator ruling on algedonic card 33b0eebb, option (A),
    /// 2026-10-18: the growth ceiling monitors a valve-stall signal on the
    /// tiers that CAN shed, not store size). `None` if the store is
    /// unavailable — the sensor returns `None` (not 0, which would mask a
    /// broken store as "empty but healthy").
    async fn sheddable_h_mem_count(&self) -> Option<usize>;

    /// Count of h_mems at or below the given confidence threshold.
    async fn low_confidence_count(&self, threshold: f64) -> Option<usize>;

    /// Configured memory life in days (the retention half-life parameter).
    async fn memory_life_days(&self) -> f64;
}

/// Senses memory health metrics from the memory store.
///
/// Emits signals for 4 `SignalMetric` variants that previously had policy
/// rules but no sensor:
/// - `TripleCount` — sheddable-tier h_mem count (turn storage + goal events)
///   above the set-point: the memory-store growth breach. The additive-by-design
///   knowledge layer is exempt (operator ruling on algedonic card 33b0eebb,
///   option (A), 2026-10-18 — its growth is distillation working; a size alarm
///   on it fires on normal operation forever). Escalates as
///   `memory_store_growth` — see `set_points::DEFAULT_TRIPLE_COUNT_MAX` for
///   the ceiling's basis and algedonic card c148d75d)
/// - `LowConfidenceCount` — low-confidence h_mem count above the set-point
/// - `ConsolidationCandidates` — same count using the consolidation floor
/// - `MemoryLife` — configured memory life days below the set-point (too short)
///
/// One registered sensor per metric reports both healthy and degraded states.
/// A busy metric must not hide another metric's recovery.
pub(crate) struct MemoryHealthSensor {
    source: Arc<dyn MemoryHealthSource>,
    metric: SignalMetric,
    /// Set-point: max sheddable-tier h_mem count (turn storage + goal events)
    /// before `TripleCount` fires — the memory-store growth ceiling (see
    /// `set_points::DEFAULT_TRIPLE_COUNT_MAX` for the valve-cadence constraint
    /// and measured basis; algedonic card c148d75d).
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
                self.source.sheddable_h_mem_count().await? as f64,
                self.triple_count_max as f64,
            ),
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

// ═════════════════════════════════════════════════════════════════════════
// EVOLUTION REGISTRY HEALTH (§P8.9 step 1)
// ═════════════════════════════════════════════════════════════════════════

/// The experimentation registry could not be read — a broken sensor, never
/// an empty result (the `.rules` `unwrap_or(0)` trap: an unreadable registry
/// must not read as "no stuck experiments").
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum ExperimentationHealthError {
    /// The store's health-snapshot read failed; the payload is the store
    /// error's message.
    #[error("experimentation health snapshot failed: {0}")]
    Snapshot(String),
}

/// Evolution registry health source — the bridge implements this over the
/// evolution store's health snapshot.
#[async_trait::async_trait]
pub trait ExperimentationHealthSource: Send + Sync {
    /// Ids of running experiments that are stuck: unresolved past the stale
    /// set point (D-3), or with their whole declared budget recorded but no
    /// verdict. Empty = healthy. `Err` means the registry could not be read —
    /// a broken sensor, which the caller must `warn!` about, never collapse
    /// into an empty list (the `.rules` `unwrap_or(0)` trap: an unreadable
    /// registry would read as "no stuck experiments").
    async fn stuck_running_experiments(
        &self,
        stale_days: u32,
    ) -> Result<Vec<String>, ExperimentationHealthError>;
}

/// The stale-experiment set point (D-3, operator ruling 2026-09-30): an
/// experiment unresolved after a week is a deviation worth an advisory,
/// not a crisis.
pub const DEFAULT_EXPERIMENTATION_STALE_DAYS: u32 = 7;

/// Senses stuck experiments from the registry's health snapshot
/// (§P8.9 step 1) — the Layer-A afferent pathway for the experimentation program.
/// Without this, the loop reports `signal_count=0` while an experiment sits
/// unresolved forever or a session strands an experiment at its budget
/// ceiling — the blind-feedback-loop trap.
pub(crate) struct ExperimentationHealthSensor {
    source: Arc<dyn ExperimentationHealthSource>,
    stale_days: u32,
}

impl ExperimentationHealthSensor {
    pub fn new(source: Arc<dyn ExperimentationHealthSource>, stale_days: u32) -> Self {
        Self { source, stale_days }
    }
}

#[async_trait::async_trait]
impl Sensor for ExperimentationHealthSensor {
    async fn observe(&self) -> Option<Signal> {
        let stuck = match self.source.stuck_running_experiments(self.stale_days).await {
            Ok(stuck) => stuck,
            Err(error) => {
                // A broken sensor is not "no deviation" — warn so an
                // unreadable registry is distinguishable from a healthy one
                // (the `.rules` failure-signal rule).
                tracing::warn!(
                    target: "hkask.sensor.evolution",
                    error = %error,
                    "ExperimentationHealthSensor: experimentation registry unreadable — returning no signal (not 'no deviation')"
                );
                return None;
            }
        };
        // A real zero proves recovery; a missing reading does not.
        Some(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::ExperimentationStuckExperiments,
            stuck.len() as f64,
            0.0, // set-point: no stuck experiments
        ))
    }
}

#[cfg(test)]
mod tests;
