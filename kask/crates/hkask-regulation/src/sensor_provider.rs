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
    /// Total h_mem count (valid h_mems only). `None` if the store is
    /// unavailable — the sensor returns `None` (not 0, which would mask a
    /// broken store as "empty but healthy").
    async fn h_mem_count(&self) -> Option<usize>;

    /// Count of h_mems at or below the given confidence threshold.
    async fn low_confidence_count(&self, threshold: f64) -> Option<usize>;

    /// Configured memory life in days (the retention half-life parameter).
    async fn memory_life_days(&self) -> f64;
}

/// Senses memory health metrics from the memory store.
///
/// Emits signals for 4 `SignalMetric` variants that previously had policy
/// rules but no sensor:
/// - `TripleCount` — h_mem count above the set-point (too many h_mems)
/// - `LowConfidenceCount` — low-confidence h_mem count above the set-point
/// - `ConsolidationCandidates` — same count using the consolidation floor
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
                self.source.h_mem_count().await? as f64,
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::runtime::RegulationLedger;

    struct FixedSensor(f64);

    #[async_trait::async_trait]
    impl Sensor for FixedSensor {
        async fn observe(&self) -> Option<Signal> {
            Some(Signal::new(
                LoopId::Cybernetics,
                SignalMetric::CircuitBreakerState,
                self.0,
                1.0,
            ))
        }
    }

    /// expect: "Rewiring inference replaces stale observations instead of duplicating them"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: two providers are wired successively for the same metric
    /// post: one signal remains and it comes from the latest provider
    #[tokio::test]
    async fn sensor_bus_replaces_late_wired_metric_source() {
        let bus = SensorBus::new();
        bus.replace(
            SignalMetric::CircuitBreakerState,
            Arc::new(FixedSensor(0.0)),
        );
        bus.replace(
            SignalMetric::CircuitBreakerState,
            Arc::new(FixedSensor(1.0)),
        );

        let signals = bus.sense_all(LoopId::Cybernetics).await;
        assert_eq!(signals.len(), 1);
        assert_eq!(signals.first().map(|signal| signal.value), Some(1.0));
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
        // Enough failures to meet the minimum-sample floor, so it is the
        // set-point (not the floor) that silences the sensor here.
        for _ in 0..TOOL_RELIABILITY_MIN_DOMAIN_SAMPLES {
            ledger.record_outcome("media", false, None).await;
        }
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
        // Five failures: the sensor's minimum-sample floor. A single failure
        // is the small-sample noise class the floor excludes, so the 0%
        // reading must come from a sample the floor admits.
        for _ in 0..TOOL_RELIABILITY_MIN_DOMAIN_SAMPLES {
            ledger.record_outcome("media", false, None).await;
        }
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

    /// The minimum-sample floor: a quiet window where one or two failures
    /// are the entire sample produces NO signal — the live-observed
    /// 0.5/0.6667/0.75 deviations that fired `tool_reliability` escalations
    /// during idle periods. This is the small-sample noise class the floor
    /// eliminates.
    #[tokio::test]
    async fn tool_reliability_sensor_excludes_small_sample_domains() {
        let ledger = RegulationLedger::default();
        ledger.record_outcome("media", true, None).await;
        ledger
            .record_outcome("media", false, Some("internal"))
            .await;
        let sensor = ToolReliabilitySensor::new(Arc::new(tokio::sync::RwLock::new(ledger)), 0.80);
        assert!(
            sensor.sense().await.is_none(),
            "a 2-call domain (1 success, 1 failure) is below the 5-sample floor — \
             no signal, not a 0.5 deviation"
        );
    }

    /// Domains below the floor are excluded from the aggregate, not merged
    /// into it: a healthy-volume domain's rate must not be dragged by a
    /// quiet neighbor's small sample.
    #[tokio::test]
    async fn tool_reliability_sensor_aggregates_only_domains_meeting_the_floor() {
        let ledger = RegulationLedger::default();
        // Domain "busy": 5 operations, 3 successes → 0.6 (meets the floor).
        for outcome in [true, true, true, false, false] {
            ledger.record_outcome("busy", outcome, None).await;
        }
        // Domain "quiet": 2 operations, 1 success → 0.5, below the floor —
        // must not dilute the aggregate.
        ledger.record_outcome("quiet", true, None).await;
        ledger
            .record_outcome("quiet", false, Some("internal"))
            .await;
        let sensor = ToolReliabilitySensor::new(Arc::new(tokio::sync::RwLock::new(ledger)), 0.80);
        let signal = sensor
            .sense()
            .await
            .expect("busy domain meets the floor — signal required");
        assert_eq!(
            signal.value, 0.6,
            "only the busy domain's rate aggregates; the quiet domain is excluded"
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
    async fn ocr_health_sensor_filters_healthy_from_deviation_helper() {
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
