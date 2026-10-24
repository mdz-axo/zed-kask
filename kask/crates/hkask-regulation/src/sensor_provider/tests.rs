use super::*;

use crate::runtime::RegulationLedger;

/// No recorded turns is the no-data state: the sensor must stay silent, not
/// emit a 0.0 rate that would read as perfect instrument compliance.
#[tokio::test]
async fn native_first_rate_sensor_returns_none_with_no_recorded_turns() {
    let sensor = NativeFirstRateSensor::new(
        std::sync::Arc::new(tokio::sync::RwLock::new(RegulationLedger::default())),
        0.50,
    );
    assert!(
        sensor.sense().await.is_none(),
        "no recorded turns = no data — None, not a 0.0 rate"
    );
}

/// The rate is over SUBSTANTIVE turns only: non-substantive turns (short
/// prompts) are excluded by the ledger, so acknowledgments cannot dilute
/// the drift reading. The production `observe` path emits whenever data
/// exists (healthy readings flow; the compare phase decides) — pinned
/// alongside the test-only deviating `sense` view.
#[tokio::test]
async fn native_first_rate_sensor_reports_the_rate_over_substantive_turns() {
    let ledger = RegulationLedger::default();
    ledger.record_agent_turn(true, true).await; // substantive, no instruments
    ledger.record_agent_turn(true, false).await; // substantive, instrumented
    ledger.record_agent_turn(false, true).await; // non-substantive: not recorded
    let sensor =
        NativeFirstRateSensor::new(std::sync::Arc::new(tokio::sync::RwLock::new(ledger)), 0.25);
    // Production view (`observe`): emits whenever data exists — the compare
    // phase, not the sensor, decides whether action is needed.
    let observed = sensor
        .observe()
        .await
        .expect("observe emits whenever data exists");
    assert_eq!(observed.metric, SignalMetric::NativeFirstRate);
    assert_eq!(observed.source, LoopId::Cybernetics);
    assert!(
        (observed.value - 0.5).abs() < 1e-9,
        "1 of 2 substantive turns native-first"
    );
    assert_eq!(observed.set_point, 0.25);
    // Test-only deviating view (`sense`): 0.5 exceeds the 0.25 set point.
    assert!(
        sensor.sense().await.is_some(),
        "0.5 exceeds the 0.25 set point — a deviation in the test view"
    );
}

/// A reading AT the set point produces no deviation: `observe` still
/// emits (healthy readings flow to the compare phase), while the test-only
/// `sense` view filters it — the same equality contract the tool_reliability
/// set-point-zero test pins. A reading strictly below the set point is a
/// BelowSetPoint deviation under the generic comparator (no policy rule
/// matches this metric in that direction), so equality is the case that
/// pins.
#[tokio::test]
async fn native_first_rate_reading_at_set_point_is_not_a_deviation() {
    let ledger = RegulationLedger::default();
    ledger.record_agent_turn(true, true).await; // substantive, no instruments
    ledger.record_agent_turn(true, false).await; // substantive, instrumented
    let sensor =
        NativeFirstRateSensor::new(std::sync::Arc::new(tokio::sync::RwLock::new(ledger)), 0.50);
    let observed = sensor
        .observe()
        .await
        .expect("observe emits whenever data exists");
    assert!(
        (observed.value - 0.5).abs() < 1e-9,
        "1 of 2 substantive turns native-first"
    );
    assert_eq!(observed.set_point, 0.50);
    assert!(
        sensor.sense().await.is_none(),
        "0.5 does not exceed the 0.50 set point — no deviation in the test view"
    );
}

/// The window is capped and keeps recency: the oldest records drop out, so
/// the rate reflects recent behavior, not the whole session.
#[tokio::test]
async fn native_first_window_caps_and_keeps_recency() {
    let ledger = RegulationLedger::default();
    // First 5 turns instrumented; then 200 native-first. The window cap is
    // 200, so the 5 instrumented records drop out and the rate reads 1.0 —
    // recency, not history.
    for _ in 0..5 {
        ledger.record_agent_turn(true, false).await;
    }
    for _ in 0..200 {
        ledger.record_agent_turn(true, true).await;
    }
    let (rate, samples) = ledger.native_first_rate().await.expect("window non-empty");
    assert_eq!(samples, 200, "the window is capped at 200 records");
    assert!(
        (rate - 1.0).abs() < 1e-9,
        "the 5 oldest instrumented turns dropped out"
    );
}

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
        .increment_variety("hkask-mcp-scenarios", "scenario_quantify")
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
                source: std::io::Error::new(std::io::ErrorKind::Other, "health file unreadable"),
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

struct MockExperimentationSource {
    stuck: Result<Vec<String>, ExperimentationHealthError>,
}

#[async_trait::async_trait]
impl ExperimentationHealthSource for MockExperimentationSource {
    async fn stuck_running_experiments(
        &self,
        _stale_days: u32,
    ) -> Result<Vec<String>, ExperimentationHealthError> {
        self.stuck.clone()
    }
}

/// §P8.9 step 1: stuck experiments (stale past the D-3 set point, or
/// budget spent with no verdict) surface as a count signal above the
/// zero set-point; the deviation derives downstream.
#[tokio::test]
async fn evolution_sensor_signals_stuck_experiments() {
    let sensor = ExperimentationHealthSensor::new(
        Arc::new(MockExperimentationSource {
            stuck: Ok(vec!["exp_stale".to_string(), "exp_spent".to_string()]),
        }),
        DEFAULT_EXPERIMENTATION_STALE_DAYS,
    );
    let signal = sensor.observe().await.expect("signal");
    assert_eq!(signal.metric, SignalMetric::ExperimentationStuckExperiments);
    assert_eq!(signal.value, 2.0);
    assert_eq!(signal.set_point, 0.0);
    let deviation = crate::loops::Deviation::from_signal(&signal);
    assert!(deviation.is_some(), "a positive stuck count is a deviation");
}

#[tokio::test]
async fn evolution_sensor_zero_when_healthy() {
    let sensor = ExperimentationHealthSensor::new(
        Arc::new(MockExperimentationSource {
            stuck: Ok(Vec::new()),
        }),
        DEFAULT_EXPERIMENTATION_STALE_DAYS,
    );
    let signal = sensor.observe().await.expect("signal");
    assert_eq!(signal.value, 0.0);
    assert!(
        crate::loops::Deviation::from_signal(&signal).is_none(),
        "zero stuck at set-point zero is the homeostatic state"
    );
}

#[tokio::test]
async fn experimentation_sensor_returns_none_on_broken_source() {
    let sensor = ExperimentationHealthSensor::new(
        Arc::new(MockExperimentationSource {
            stuck: Err(ExperimentationHealthError::Snapshot(
                "registry unreadable".to_string(),
            )),
        }),
        DEFAULT_EXPERIMENTATION_STALE_DAYS,
    );
    assert!(sensor.observe().await.is_none());
}
