use crate::CyberneticsLoop;
use crate::cybernetics_loop::test_support::CapturingSink;
use crate::loops::{
    ActionType, Deviation, DeviationDirection, LoopId, RegulationData, RegulatoryAction,
    RegulatoryActionParams, Signal, SignalMetric,
};
use crate::regulation_policy::RegulationPolicy;
use crate::runtime::RegulationLedger;
use hkask_types::WebID;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

/// expect: "Unbound files cannot claim my code is healthy; real tool outcomes still inform regulation." [P1]
#[test]
fn unbound_metrics_cannot_become_quality_observations() {
    const CHILD: &str = "HKASK_UNBOUND_METRICS_TEST_CHILD";
    const COMPLETE: &str = "unbound-metrics-contract-exercised";
    if std::env::var_os(CHILD).is_some() {
        let trace =
            std::path::PathBuf::from(std::env::var_os("HKASK_TRACE_DIR").expect("fixture path"));
        let fixture: serde_json::Value = serde_json::from_slice(
            &std::fs::read(trace.join("unrelated-run/metrics.json"))
                .expect("forged metrics fixture"),
        )
        .expect("fixture JSON");
        assert_eq!(fixture["coverage_pct"], 1.0);
        assert_eq!(fixture["mutation_score"], 1.0);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let regulation =
                CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
            for _ in 0..5 {
                regulation
                    .record_outcome("evidence-control", true, None)
                    .await;
            }
            let signals = regulation.sense().await;
            assert!(
                signals.iter().any(|signal| {
                    signal.metric.as_str() == "tool_reliability" && signal.value == 1.0
                }),
                "genuine outcomes must remain observable"
            );
            assert!(
                signals.iter().all(|signal| {
                    !matches!(signal.metric.as_str(), "test_coverage" | "mutation_score")
                }),
                "unbound file became a quality observation: {signals:?}"
            );
        });
        println!("{COMPLETE}");
        return;
    }

    let directory = std::env::temp_dir().join(format!("hkask-unbound-{}", WebID::new()));
    let run = directory.join("unrelated-run");
    std::fs::create_dir_all(&run).expect("isolated trace directory");
    std::fs::write(
        run.join("metrics.json"),
        r#"{"coverage_pct":1.0,"mutation_score":1.0}"#,
    )
    .expect("unbound metrics fixture");
    // Process-local environment avoids races with other tests/runtime workers.
    let mut child = tokio::process::Command::new(std::env::current_exe().expect("test executable"));
    child
        .args([
            "--exact",
            "cybernetics_loop::cycle::tests::unbound_metrics_cannot_become_quality_observations",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("HKASK_TRACE_DIR", &directory)
        .kill_on_drop(true);
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("parent test runtime")
        .block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(10), child.output()).await
        });
    std::fs::remove_dir_all(&directory).expect("remove isolated fixture");
    let output = result
        .expect("child deadline")
        .expect("child test execution");
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.contains(COMPLETE)),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct ChangingOcrHealthSource {
    reads: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl crate::OcrHealthSource for ChangingOcrHealthSource {
    async fn recent_silent_failures(&self) -> Result<u64, crate::OcrHealthError> {
        let read = self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(if read == 0 { 1 } else { 2 })
    }
}

/// A recording `AlertEscalationSink` — captures the exact strings the
/// loop persisted and auto-resolved so the two message-format sites
/// can be asserted byte-identical.
struct RecordingEscalationSink {
    persisted: Mutex<Vec<String>>,
    /// The error_context JSON each persist carried — the breakdown test
    /// asserts on its contents.
    contexts: Mutex<Vec<String>>,
}

impl RecordingEscalationSink {
    fn new() -> Self {
        Self {
            persisted: Mutex::new(Vec::new()),
            contexts: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl crate::AlertEscalationSink for RecordingEscalationSink {
    async fn try_persist_alert(
        &self,
        output: &str,
        _confidence: f64,
        error_context: &str,
    ) -> Result<crate::AlertDeliveryOutcome, crate::AlertPersistError> {
        self.persisted
            .lock()
            .expect("persisted lock")
            .push(output.to_string());
        self.contexts
            .lock()
            .expect("contexts lock")
            .push(error_context.to_string());
        Ok(crate::AlertDeliveryOutcome::Attempted)
    }
}

/// Records the first card id and reports later deliveries as repeat
/// comments, so repeated conditions do not replay live/archive alerts.
struct ConfirmingEscalationSink {
    persisted: Mutex<Vec<String>>,
}

impl ConfirmingEscalationSink {
    fn new() -> Self {
        Self {
            persisted: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl crate::AlertEscalationSink for ConfirmingEscalationSink {
    async fn try_persist_alert(
        &self,
        output: &str,
        _confidence: f64,
        _error_context: &str,
    ) -> Result<crate::AlertDeliveryOutcome, crate::AlertPersistError> {
        let mut persisted = self.persisted.lock().expect("persisted lock");
        let id = persisted.is_empty().then(|| "test-escalation".to_string());
        persisted.push(output.to_string());
        Ok(crate::AlertDeliveryOutcome::Confirmed(id))
    }
}

/// A computed action is advice until an operator applies it. Routing that
/// advice must not immediately re-read the metric and attribute concurrent
/// movement to an intervention that did not occur.
#[tokio::test]
async fn tick_does_not_verify_unapplied_ocr_advice() {
    let ocr = Arc::new(ChangingOcrHealthSource {
        reads: std::sync::atomic::AtomicUsize::new(0),
    });
    let archive = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let escalation = Arc::new(RecordingEscalationSink::new());
    let mut regulation = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
        .with_ocr_health_source(ocr.clone())
        .with_event_sink(Arc::clone(&archive) as Arc<dyn hkask_types::RegulationSink>);
    regulation.set_alert_escalation_sink(Some(escalation.clone()));

    regulation.tick().await;

    assert_eq!(
        ocr.reads.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the tick may sense the condition once but must not re-sense advice as intervention"
    );
    assert!(
        escalation
            .persisted
            .lock()
            .expect("persisted lock")
            .iter()
            .any(|message| message.starts_with("ocr_silent_failures_exceeded")),
        "the detected OCR condition must still produce an actionable advisory"
    );
    let spans = archive.0.lock().expect("archive lock");
    assert!(
        !spans.iter().any(|(path, _)| path == "impact_verified"),
        "unapplied advice has no impact verdict"
    );
    assert!(
        !spans.iter().any(|(path, _)| path == "action_blocked"),
        "unapplied advice cannot be classified as harmful"
    );
}

/// Call-cap exhaustion is detected BEFORE the per-tick reset —
/// the reset replenishes every cap (remaining = ceiling), so reading
/// after it would never observe remaining == 0 and the E04 exhaustion
/// alert could never fire (the pre-fix defect: the alert was dead
/// code). Controls: the reset still replenishes; a replenished agent
/// does not re-alert without new exhaustion; and the exhaustion alert IS
/// board-reviewable (operator ruling 2026-11-07, dual-axis review T3:
/// fresh cap exhaustion is escalated — the reset alone still earns no
/// card, only a fresh post-reset exhaustion does).
#[tokio::test]
async fn cap_exhaustion_is_detected_before_the_reset_replenishes() {
    let archive = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let escalation = Arc::new(RecordingEscalationSink::new());
    let mut regulation_loop =
        CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
    regulation_loop.set_event_sink(Arc::clone(&archive) as Arc<dyn hkask_types::RegulationSink>);
    regulation_loop.set_alert_escalation_sink(Some(escalation.clone()));

    let agent = WebID::from_persona(b"agent-x");
    {
        let cap_manager = regulation_loop.call_cap_manager.read().await;
        cap_manager.register_call_cap(agent, 2).await;
        // Exhaust between ticks, as metered dispatches would.
        cap_manager.charge_metered(&agent).await;
        cap_manager.charge_metered(&agent).await;
    }

    // No live alerts channel wired — the alert falls to the archive.
    regulation_loop.act(&[]).await;

    {
        let records = archive.0.lock().unwrap_or_else(|e| e.into_inner());
        let exhaustion_records: Vec<_> = records
            .iter()
            .filter(|(path, observation)| {
                path.contains("algedonic")
                    && observation
                        .get("message")
                        .and_then(|m| m.as_str())
                        .is_some_and(|m| m.contains("exhausted"))
            })
            .collect();
        assert_eq!(
            exhaustion_records.len(),
            1,
            "the exhausted agent must produce exactly one alert: {records:#?}"
        );
        assert!(
            exhaustion_records[0]
                .1
                .get("message")
                .and_then(|m| m.as_str())
                .is_some_and(|m| m.contains(&agent.to_string())),
            "the alert must name the exhausted agent"
        );
    }

    // The reset still replenished: the cap is back at its ceiling.
    let statuses = regulation_loop
        .call_cap_manager
        .read()
        .await
        .all_agent_statuses()
        .await;
    let status = statuses
        .iter()
        .find(|(id, _)| *id == agent)
        .expect("registered");
    assert_eq!((status.1.ceiling, status.1.remaining), (2, 2));

    // Control: a second act() with no new charges does not re-alert —
    // replenishment alone is not exhaustion.
    regulation_loop.act(&[]).await;
    let records = archive.0.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(
        records
            .iter()
            .filter(|(path, observation)| {
                path.contains("algedonic")
                    && observation
                        .get("message")
                        .and_then(|m| m.as_str())
                        .is_some_and(|m| m.contains("exhausted"))
            })
            .count(),
        1,
        "a replenished agent must not re-alert"
    );

    // The exhaustion alert IS board-reviewable (T3 ruling): the fresh
    // post-charge exhaustion persisted exactly one review delivery — and
    // the replenished control cycle above produced no second one.
    let persisted = escalation.persisted.lock().expect("persisted");
    assert_eq!(
        persisted.len(),
        1,
        "fresh cap exhaustion creates exactly one review delivery"
    );
    assert!(
        persisted[0].contains(&agent.to_string()),
        "the review delivery names the exhausted agent"
    );
}

/// expect: "Routing advice does not prove that an alert cleared" [P9]
#[tokio::test]
async fn routing_advice_does_not_resolve_alert() {
    let sink = Arc::new(RecordingEscalationSink::new());
    let mut regulation_loop =
        CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
    regulation_loop.set_alert_escalation_sink(Some(sink.clone()));
    let action = RegulatoryAction::with_metric(
        LoopId::Curation,
        ActionType::Escalate,
        RegulatoryActionParams::reason("test_escalation"),
        "test_metric".to_string(),
    );
    regulation_loop.route_action_as_alert(&action).await;
    assert_eq!(sink.persisted.lock().expect("persisted").len(), 1);
}

/// expect: "A repeat is written once to its open card and does not replay the live alert" [P9]
#[tokio::test]
async fn repeat_uses_the_delivery_result_without_a_second_board_read() {
    struct SingleDeliveryRecorder(std::sync::atomic::AtomicUsize);
    #[async_trait::async_trait]
    impl crate::AlertEscalationSink for SingleDeliveryRecorder {
        async fn try_persist_alert(
            &self,
            _output: &str,
            _confidence: f64,
            _context: &str,
        ) -> Result<crate::AlertDeliveryOutcome, crate::AlertPersistError> {
            let call = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(crate::AlertDeliveryOutcome::Confirmed(if call == 0 {
                Some("card-1".to_string())
            } else {
                None
            }))
        }
    }
    let recorder = Arc::new(SingleDeliveryRecorder(std::sync::atomic::AtomicUsize::new(
        0,
    )));
    let archive = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let mut regulation = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
        .with_event_sink(archive.clone() as Arc<dyn hkask_types::RegulationSink>);
    regulation.set_alert_escalation_sink(Some(recorder.clone()));
    let action = RegulatoryAction::with_metric(
        LoopId::Curation,
        ActionType::Escalate,
        RegulatoryActionParams::reason("repeat_delivery_contract"),
        "test_metric".to_string(),
    );
    assert!(regulation.route_action_as_alert(&action).await);
    assert!(regulation.route_action_as_alert(&action).await);
    assert_eq!(recorder.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(
        archive.0.lock().expect("archive").len(),
        1,
        "a repeat comment must not re-emit the archive alert"
    );
}

/// Experimentation-stuck escalations carry the stuck experiment ids in the
/// board card's context — the count in the message cannot name WHICH
/// experiments to unstick (the L25 identity impedance, repaired at the
/// delivery boundary exactly like the tool-reliability breakdown
/// above).
#[tokio::test]
async fn experimentation_alert_carries_stuck_experiment_ids() {
    struct StuckSource(Vec<String>);
    #[async_trait::async_trait]
    impl crate::sensor_provider::ExperimentationHealthSource for StuckSource {
        async fn stuck_running_experiments(
            &self,
            _stale_days: u32,
        ) -> Result<Vec<String>, crate::sensor_provider::ExperimentationHealthError> {
            Ok(self.0.clone())
        }
    }
    let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
    let escalation = Arc::new(RecordingEscalationSink::new());
    let mut regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger));
    regulation_loop.set_alert_escalation_sink(Some(escalation.clone()));
    regulation_loop.set_experimentation_health_source(Arc::new(StuckSource(vec![
        "exp_stale".to_string(),
        "exp_spent".to_string(),
    ])));
    // The sense phase's observation the alert routing looks up; seed it
    // directly so the test exercises the routing, not the bus.
    regulation_loop.observations.lock().insert(
        SignalMetric::ExperimentationStuckExperiments,
        Signal::new(
            LoopId::Cybernetics,
            SignalMetric::ExperimentationStuckExperiments,
            2.0,
            0.0,
        ),
    );

    let action = RegulatoryAction::with_metric(
        LoopId::Curation,
        ActionType::Escalate,
        RegulatoryActionParams::with_data(
            "experimentation_stuck_experiments_exceeded",
            RegulationData::ExperimentationStuckExperimentsExceeded {
                count: 2.0,
                threshold: 0.0,
            },
        ),
        "experimentation_stuck_experiments".into(),
    );
    regulation_loop.route_action_as_alert(&action).await;

    let contexts = escalation.contexts.lock().expect("contexts");
    assert_eq!(
        contexts.len(),
        1,
        "the stuck-experiment alert persists once"
    );
    let context: serde_json::Value =
        serde_json::from_str(&contexts[0]).expect("error_context is JSON");
    let stuck = context
        .get("stuck_experiments")
        .expect("evolution-stuck alerts carry the stuck ids");
    assert!(
        stuck.to_string().contains("exp_stale") && stuck.to_string().contains("exp_spent"),
        "the context names both stuck experiments: {stuck}"
    );
}

/// Tool-reliability alerts carry the per-domain outcome breakdown in
/// the board card's context and the `reg.outcome.tool_domains` span
/// (what the algedonic log serves via
/// `curator_algedonic_log`). Before this, the escalation carried only
/// the aggregate success rate — triage could see THAT tools were
/// failing but never WHICH domain or with what error kinds.
#[tokio::test]
async fn tool_reliability_alert_carries_outcome_breakdown() {
    let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
    {
        let ledger_guard = ledger.read().await;
        for _ in 0..crate::sensor_provider::TOOL_RELIABILITY_MIN_DOMAIN_SAMPLES {
            ledger_guard
                .record_outcome("media", false, Some("internal"))
                .await;
        }
    }
    let archive = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let escalation = Arc::new(RecordingEscalationSink::new());
    let mut regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger));
    regulation_loop.set_event_sink(Arc::clone(&archive) as Arc<dyn hkask_types::RegulationSink>);
    regulation_loop.set_alert_escalation_sink(Some(escalation.clone()));
    // The sense phase records the observation the alert routing looks
    // up; seed it directly so the test exercises the routing, not the bus.
    regulation_loop.observations.lock().insert(
        SignalMetric::ToolReliability,
        Signal::new(
            LoopId::Cybernetics,
            SignalMetric::ToolReliability,
            0.0,
            0.80,
        ),
    );

    let action = RegulatoryAction::with_metric(
        LoopId::Curation,
        ActionType::Escalate,
        RegulatoryActionParams::with_data(
            "tool_reliability_degraded",
            RegulationData::ToolReliabilityDegraded {
                reliability: 0.0,
                threshold: 0.8,
            },
        ),
        "tool_reliability".into(),
    );
    regulation_loop.route_action_as_alert(&action).await;

    let contexts = escalation.contexts.lock().expect("contexts");
    assert_eq!(contexts.len(), 1, "the degradation alert persists once");
    let context: serde_json::Value =
        serde_json::from_str(&contexts[0]).expect("error_context is JSON");
    let breakdown = context
        .get("outcome_breakdown")
        .expect("tool-reliability alerts carry the breakdown");
    assert!(
        breakdown.to_string().contains("media"),
        "the breakdown names the failing domain: {breakdown}"
    );
    assert!(
        breakdown.to_string().contains("internal"),
        "the breakdown carries the error kind: {breakdown}"
    );

    let spans = archive.0.lock().unwrap_or_else(|e| e.into_inner());
    let tool_domains: Vec<_> = spans
        .iter()
        .filter(|(path, _)| path == "reg.outcome.tool_domains")
        .collect();
    assert_eq!(tool_domains.len(), 1, "one breakdown span at alert time");
    assert!(
        tool_domains[0]
            .1
            .get("domains")
            .expect("domains field")
            .to_string()
            .contains("media"),
        "the span names the failing domain"
    );
}

/// dropping actions and leaving the loop open for those metrics. The
/// match is now exhaustive (no `_ =>` arm), so the compiler enforces
/// closure at compile time. This test verifies the behavioral side:
/// each newly-handled arm actually returns Some(action).
#[test]
fn build_regulation_action_produces_action_for_all_new_reasons() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let regulation_loop =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        let policy = RegulationPolicy::default();

        // (metric, direction, value, set_point) for each newly-handled rule.
        // AboveSetPoint: value > set_point. BelowSetPoint: value < set_point.
        use DeviationDirection::*;
        use SignalMetric::*;
        let cases: &[(SignalMetric, DeviationDirection, f64, f64)] = &[
            // Memory-store growth (Escalate, AboveSetPoint — algedonic card
            // c148d75d: the forgetting-valve breach reaches the board)
            (TripleCount, AboveSetPoint, 1.0, 0.0),
            // Category A: Observational (Notify, AboveSetPoint)
            (LowConfidenceCount, AboveSetPoint, 1.0, 0.0),
            (ConsolidationCandidates, AboveSetPoint, 1.0, 0.0),
            (PendingEscalations, AboveSetPoint, 1.0, 0.0),
            // Category B: Meta-regulatory (Escalate, AboveSetPoint)
            (AlgedonicEvents, AboveSetPoint, 1.0, 0.0),
            (AlgedonicLogApproachingCap, AboveSetPoint, 1.0, 0.0),
            (GoalStaleCount, AboveSetPoint, 1.0, 0.0),
            (GoalExpiredCount, AboveSetPoint, 1.0, 0.0),
            (MetacognitionCriticalAlerts, AboveSetPoint, 1.0, 0.0),
            // Category C: Domain-specific
            (MemoryLife, BelowSetPoint, 0.0, 1.0),
            (CircuitBreakerState, AboveSetPoint, 1.0, 0.0),
            (InferenceModelAvailable, BelowSetPoint, 0.0, 1.0),
            (ContextServerHealth, BelowSetPoint, 0.0, 1.0),
            (OcrSilentFailures, AboveSetPoint, 14.0, 0.0),
            (ExperimentationStuckExperiments, AboveSetPoint, 2.0, 0.0),
            // NativeFirstRate exemplar carries the recalibrated default
            // set point (0.75, card 24670321, operator-ratified 2026-11-09)
            (NativeFirstRate, AboveSetPoint, 0.8, 0.75),
        ];

        for &(metric, direction, value, set_point) in cases {
            let signal = Signal::new(LoopId::Cybernetics, metric, value, set_point);
            let deviation = Deviation::from_signal(&signal)
                .unwrap_or_else(|| panic!("{metric:?} should deviate from set_point"));
            assert_eq!(
                deviation.direction, direction,
                "{metric:?} deviation direction mismatch"
            );
            let proposed = policy.decide(&deviation);
            assert!(
                !proposed.is_empty(),
                "{metric:?} {:?} must have a policy rule",
                direction
            );
            for p in proposed {
                let action = regulation_loop.build_regulation_action(&deviation, p).await;
                assert!(
                    action.is_some(),
                    "{metric:?} {:?} build_regulation_action returned None for reason {:?}",
                    direction,
                    p.reason
                );
            }
        }
    });
}

/// Notify is a truthful handled disposition and contributes to response coverage.
#[test]
fn compute_includes_notify_dispositions() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let regulation_loop =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        // LowConfidenceCount AboveSetPoint triggers a Notify rule. (The
        // original fixture was TripleCount; it escalates since algedonic
        // card c148d75d — see
        // growth_breach_escalates_to_the_board_and_recovery_does_not.)
        let signal = Signal::new(
            LoopId::Cybernetics,
            SignalMetric::LowConfidenceCount,
            1.0,
            0.0,
        );
        let deviation = Deviation::from_signal(&signal)
            .expect("LowConfidenceCount 1.0 vs set_point 0.0 should deviate");
        let actions = regulation_loop.compute(&[deviation]).await;
        assert_eq!(actions.len(), 1);
        assert!(
            actions
                .first()
                .is_some_and(|action| action.action_type == ActionType::Notify),
            "the observation must be represented as a handled Notify disposition"
        );
    });
}

/// algedonic card c148d75d (cadence decision on card 1c02ef3c, metric (a)):
/// the memory-store growth breach escalates through the standard algedonic
/// path — the board card class is `memory_store_growth` (the reason string
/// the delivery path carries into the card) — and a store under the ceiling
/// fires nothing. The sensor reads state (sheddable-tier h_mem count vs
/// `triple_count_max`), never events, so the therapy session that reduces
/// the count is the response, not a new breach: the under-ceiling case is
/// exactly the post-therapy state.
#[tokio::test]
async fn growth_breach_escalates_to_the_board_and_recovery_does_not() {
    let (source, regulation) = loop_with_memory_observations();

    // Over the ceiling (fixture sheddable-tier count 1_000_000 vs the 4_000 default).
    let over = regulation.sense().await;
    let growth_signal = over
        .iter()
        .find(|signal| signal.metric == SignalMetric::TripleCount)
        .expect("an over-ceiling store must be sensed on TripleCount");
    let growth = Deviation::from_signal(growth_signal)
        .expect("an over-ceiling TripleCount signal must deviate");
    assert_eq!(growth.direction, DeviationDirection::AboveSetPoint);
    let actions = regulation.compute(&[growth]).await;
    let escalation = actions
        .iter()
        .find(|action| action.metric_name.as_deref() == Some("triple_count"))
        .expect("the growth breach must produce an action");
    assert_eq!(
        escalation.action_type,
        ActionType::Escalate,
        "the growth breach must escalate (algedonic card class memory_store_growth), not Notify"
    );

    // Under the ceiling (fixture h_mem_count 0) — the post-therapy state.
    source.0.store(1, std::sync::atomic::Ordering::SeqCst);
    let under = regulation.sense().await;
    assert!(
        under
            .iter()
            .all(|signal| signal.metric != SignalMetric::TripleCount
                || Deviation::from_signal(signal).is_none()),
        "an under-ceiling store must not deviate on TripleCount"
    );
}

/// The re-scope boundary (operator ruling on algedonic card 33b0eebb,
/// option (A), 2026-10-18): the growth ceiling counts the SHEDDABLE tiers
/// (turn storage + goal events) — the tiers with forgetting valves, whose
/// growth above a ceiling genuinely signals a stuck valve. An
/// additive-tier-only store above the old ceiling's total (1_000_000
/// knowledge rows, sheddable tiers at 0) must NOT breach — the knowledge
/// layer is additive-by-design (distillation appends lessons, nothing
/// deletes them; recall quality is protected by confidence decay, not
/// deletion). Under the pre-flip sensor (which counted the whole store),
/// this exact state deviated on TripleCount — that is the recurring
/// `memory_store_growth` false positive this re-scope eliminates. The
/// real prefix-scoped exclusion (`sheddable_h_mem_count` against a real
/// `MemoryStore`) is pinned in `kask_bridge`.
#[tokio::test]
async fn additive_only_store_above_the_old_ceiling_does_not_breach() {
    let (source, regulation) = loop_with_memory_observations();

    // Additive-tier-only growth: case 3 — 1_000_000 knowledge rows,
    // sheddable tiers at 0.
    source.0.store(3, std::sync::atomic::Ordering::SeqCst);
    let sensed = regulation.sense().await;
    assert!(
        sensed
            .iter()
            .all(|signal| signal.metric != SignalMetric::TripleCount
                || Deviation::from_signal(signal).is_none()),
        "an additive-tier-only store must not deviate on TripleCount — the \
         knowledge layer's growth is distillation working, not a valve stall"
    );

    // And the positive half still holds: a sheddable-tier breach still fires
    // (case 0 — the breach state; also asserted by
    // growth_breach_escalates_to_the_board_and_recovery_does_not).
    source.0.store(0, std::sync::atomic::Ordering::SeqCst);
    let breach = regulation.sense().await;
    assert!(
        breach
            .iter()
            .any(|signal| signal.metric == SignalMetric::TripleCount
                && Deviation::from_signal(signal).is_some()),
        "a sheddable-tier store above the ceiling must still deviate on TripleCount"
    );
}

/// Pins B2: fidelity matching must use metric_name only, not string
/// fallback on reason. An action without metric_name but with a reason
/// that would have matched under the old fallback (e.g., "low" matching
/// "energy_budget_low") must NOT count as a match. Before the fix, the
/// string fallback produced false positives by conflating direction
/// semantics ("low" matched both "energy_budget_low" and
/// "low_confidence_count").
#[test]
fn from_cycle_fidelity_no_string_fallback() {
    use crate::loops::core::{LoopMetrics, TriggerOrigin};
    let signal = Signal::new(LoopId::Cybernetics, SignalMetric::ToolReliability, 0.1, 0.2);
    let deviation = Deviation::from_signal(&signal).unwrap();
    // An action with no metric_name but a reason that contains "low" —
    // under the old fallback this would have matched the deviation's
    // metric via reason.contains("low").
    let action = RegulatoryAction {
        target: LoopId::Curation,
        action_type: ActionType::Escalate,
        parameters: RegulatoryActionParams::reason("some_unrelated_low_thing"),
        metric_name: None,
    };
    let metrics = LoopMetrics::from_cycle(0, &[deviation], &[action], TriggerOrigin::Scheduled);
    assert_eq!(
        metrics.fidelity_score, 0.0,
        "action without metric_name must not match via string fallback"
    );
}

/// The loop + memory-source fixture shared by the memory tests below
/// (the 2026-10-09 duplication-ratchet diagnosis measured the two
/// tests' identical setup as a net-new intra-file cluster).
fn loop_with_memory_observations() -> (Arc<MemoryObservations>, CyberneticsLoop) {
    let source = Arc::new(MemoryObservations(std::sync::atomic::AtomicUsize::new(0)));
    let mut regulation = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
    regulation.set_memory_health_source(source.clone());
    (source, regulation)
}

/// Case-indexed memory-source double: 0 = sheddable tiers at 1_000_000 (the
/// breach state), 1 = sheddable tiers at 0 (healthy, post-therapy), 2 = store
/// unavailable (`None` — must mask as unavailability, never as zero), 3 =
/// additive-tier-only growth (1_000_000 knowledge rows, sheddable tiers at 0 —
/// the re-scope boundary case; the real prefix-scoped exclusion is pinned in
/// `kask_bridge` against a real `MemoryStore`).
struct MemoryObservations(std::sync::atomic::AtomicUsize);
#[async_trait::async_trait]
impl crate::MemoryHealthSource for MemoryObservations {
    async fn sheddable_h_mem_count(&self) -> Option<usize> {
        match self.0.load(std::sync::atomic::Ordering::SeqCst) {
            0 => Some(1_000_000),
            1 | 3 => Some(0),
            _ => None,
        }
    }
    async fn low_confidence_count(&self, _: f64) -> Option<usize> {
        self.sheddable_h_mem_count().await
    }
    async fn memory_life_days(&self) -> f64 {
        if self.0.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            0.0
        } else {
            36_500.0
        }
    }
}

/// expect: "Every memory metric can report recovery independently; unavailable stores do not report zero" [P9]
#[tokio::test]
async fn memory_observations_report_all_metrics_and_recovery() {
    let (source, regulation) = loop_with_memory_observations();
    let metrics = [
        SignalMetric::MemoryLife,
        SignalMetric::TripleCount,
        SignalMetric::LowConfidenceCount,
        SignalMetric::ConsolidationCandidates,
    ];
    let degraded = regulation.sense().await;
    for metric in metrics {
        let signal = degraded
            .iter()
            .find(|signal| signal.metric == metric)
            .expect("every memory metric sensed");
        assert!(Deviation::from_signal(signal).is_some(), "{metric:?}");
    }
    source.0.store(1, std::sync::atomic::Ordering::SeqCst);
    let healthy = regulation.sense().await;
    for metric in metrics {
        let signal = healthy
            .iter()
            .find(|signal| signal.metric == metric)
            .expect("healthy metric observable");
        assert!(Deviation::from_signal(signal).is_none(), "{metric:?}");
        assert!(
            degraded
                .iter()
                .find(|signal| signal.metric == metric)
                .expect("trigger")
                .recovered_by(signal)
        );
    }
    source.0.store(2, std::sync::atomic::Ordering::SeqCst);
    let unavailable = regulation.sense().await;
    assert_eq!(
        unavailable
            .iter()
            .filter(|signal| metrics.contains(&signal.metric))
            .count(),
        1,
        "only configuration remains observable without a store"
    );
}

/// D-sensing (2026-08-30): the algedonic log's population state must be
/// sensed — `AlgedonicEvents`, `PendingEscalations`, and
/// `MetacognitionCriticalAlerts` were policy-only (rules that could never
/// fire) before this. A clean log emits healthy zero observations; a critical
/// outcome alert (0% success → Critical, escalated) emits all three with
/// value 1.0 against set-point 0.0.
#[test]
fn sense_reports_algedonic_log_population() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
        let regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger));

        // Healthy observations distinguish recovery from unavailable sensing.
        let signals = regulation_loop.sense().await;
        for metric in [
            SignalMetric::AlgedonicEvents,
            SignalMetric::PendingEscalations,
            SignalMetric::MetacognitionCriticalAlerts,
        ] {
            let signal = signals
                .iter()
                .find(|signal| signal.metric == metric)
                .unwrap_or_else(|| panic!("{metric:?} must be observed"));
            assert_eq!(signal.value, 0.0);
            assert_eq!(signal.set_point, 0.0);
            assert!(Deviation::from_signal(signal).is_none());
        }

        // An Info-only log must not create a deviation: Info entries
        // are healthy-range diagnostics (a variety check slightly below
        // expected), not review demands. Before the actionable-count
        // fix, any log population — including normal-use Info noise —
        // fired the sensor as a standing escalation.
        {
            let ledger_guard = ledger.read().await;
            ledger_guard
                .increment_variety("variety_info_test", "only_tool")
                .await;
            let log_count = ledger_guard.alert_log_count().await;
            drop(ledger_guard);
            assert!(
                log_count > 0,
                "sanity: the variety check pushed an Info alert"
            );
        }
        let signals = regulation_loop.sense().await;
        let signal = signals
            .iter()
            .find(|signal| signal.metric == SignalMetric::AlgedonicEvents)
            .expect("Info-only log must remain observable");
        assert_eq!(signal.value, 0.0);
        assert_eq!(signal.set_point, 0.0);
        assert!(Deviation::from_signal(signal).is_none());

        // A critical outcome alert: 0% success over 5 operations
        // (the minimum sample `check_outcome` evaluates) → Critical,
        // escalated. `record_outcome` re-checks thresholds on every record,
        // so the alert fires on the fifth failure.
        let ledger_guard = ledger.read().await;
        for _ in 0..5 {
            ledger_guard.record_outcome("sense_test", false, None).await;
        }
        let alert_count = ledger_guard.alert_log_count().await;
        drop(ledger_guard);
        assert!(alert_count > 0, "0% success must produce an alert");

        let signals = regulation_loop.sense().await;
        for (metric, expected_value) in [
            (SignalMetric::AlgedonicEvents, 1.0),
            (SignalMetric::PendingEscalations, 1.0),
            (SignalMetric::MetacognitionCriticalAlerts, 1.0),
        ] {
            let signal = signals
                .iter()
                .find(|s| s.metric == metric)
                .unwrap_or_else(|| panic!("{metric:?} must be sensed"));
            assert_eq!(signal.value, expected_value);
            assert_eq!(signal.set_point, 0.0);
            assert!(Deviation::from_signal(signal).is_some());
        }
    });
}

/// `record_variety` is the dispatch twin of `record_outcome`: one call
/// per governed tool invocation, tool name as the observed state. It
/// feeds the ledger's variety trackers — the VarietySensor's data
/// source. Repeats of the same tool count once (variety is distinct
/// tools, not call volume).
#[test]
fn record_variety_feeds_ledger_trackers() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
        let regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger));
        regulation_loop
            .record_variety("hkask-mcp-media", "gallery_search")
            .await;
        regulation_loop
            .record_variety("hkask-mcp-media", "gallery_search")
            .await;
        regulation_loop
            .record_variety("hkask-mcp-media", "gallery_add_media")
            .await;
        let ledger_guard = ledger.read().await;
        assert_eq!(
            ledger_guard.variety_for_domain("hkask-mcp-media").await,
            2,
            "distinct tool names are the variety — repeats don't count"
        );
    });
}

struct HealthyResilienceSource;

#[async_trait::async_trait]
impl crate::InferenceResilienceSource for HealthyResilienceSource {
    async fn observe_since(
        &self,
        cursor: u64,
    ) -> Result<crate::InferenceObservation, crate::InferenceObservationError> {
        Ok(crate::InferenceObservation {
            snapshot: crate::InferenceSnapshot {
                observed_at: chrono::Utc::now(),
                in_flight: 0,
                max_concurrency: 96,
                recent_timeout_count: 0,
                circuit_state: crate::InferenceCircuitState::Closed,
            },
            interventions: Vec::new(),
            permanent_failures: Vec::new(),
            next_cursor: cursor,
        })
    }
}

/// `InferenceModelAvailable` (2026-08-30): an unwired inference health
/// source means the default model never resolved — unusable inference.
/// The signal must stay silent during the boot grace window (slow
/// registry population is not an outage), fire after it, and clear the
/// moment the source is wired.
#[test]
fn sense_reports_unwired_inference_model_after_grace() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
        let mut regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger));

        // Before the grace period: no signal — the deferred task often
        // wires the model a few seconds after the first ticks.
        let signals = regulation_loop.sense().await;
        assert!(
            !signals
                .iter()
                .any(|s| s.metric == SignalMetric::InferenceModelAvailable),
            "grace window must not report a model outage"
        );

        // After 3 ticks with no wired source: model-unavailable.
        regulation_loop
            .tick_count
            .store(3, std::sync::atomic::Ordering::Relaxed);
        let signals = regulation_loop.sense().await;
        let signal = signals
            .iter()
            .find(|s| s.metric == SignalMetric::InferenceModelAvailable)
            .expect("unwired inference must be sensed after grace");
        assert_eq!(signal.value, 0.0);
        assert_eq!(signal.set_point, 1.0);

        // Once the source is wired (model resolved): no signal.
        regulation_loop.set_inference_resilience_source(Arc::new(HealthyResilienceSource));
        let signals = regulation_loop.sense().await;
        assert!(
            !signals
                .iter()
                .any(|s| s.metric == SignalMetric::InferenceModelAvailable
                    && Deviation::from_signal(s).is_some()),
            "wired source means the model resolved — no outage deviation"
        );
    });
}

struct FailOnceCapturingSink {
    fail_next: std::sync::atomic::AtomicBool,
    persisted: Mutex<Vec<(String, serde_json::Value)>>,
}

impl hkask_types::RegulationSink for FailOnceCapturingSink {
    fn persist(
        &self,
        event: &hkask_types::RegulationRecord,
    ) -> Result<(), hkask_types::InfrastructureError> {
        if self
            .fail_next
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(hkask_types::InfrastructureError::Serialization(
                "sink unavailable".to_string(),
            ));
        }
        self.persisted
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((event.span.path.clone(), event.observation.clone()));
        Ok(())
    }
}

/// The shared observe_since body: clone the observation, keep only
/// receipts past `cursor`, and advance next_cursor to the newest kept
/// id — the filtering both resilience-source stubs below perform.
fn observe_since_filtered(
    observation: &crate::InferenceObservation,
    cursor: u64,
) -> crate::InferenceObservation {
    let mut observation = observation.clone();
    observation
        .interventions
        .retain(|receipt| receipt.id > cursor);
    observation
        .permanent_failures
        .retain(|receipt| receipt.id > cursor);
    observation.next_cursor = observation
        .interventions
        .iter()
        .map(|receipt| receipt.id)
        .chain(
            observation
                .permanent_failures
                .iter()
                .map(|receipt| receipt.id),
        )
        .max()
        .unwrap_or(cursor);
    observation
}

/// One circuit-state intervention receipt (id 1) and no permanent
/// failures — the observation shape of the transient-circuit tests.
fn circuit_intervention_observation(
    now: chrono::DateTime<chrono::Utc>,
    recent_timeout_count: u64,
    circuit_state: crate::InferenceCircuitState,
    kind: crate::InferenceInterventionKind,
) -> crate::InferenceObservation {
    crate::InferenceObservation {
        snapshot: crate::InferenceSnapshot {
            observed_at: now,
            in_flight: 0,
            max_concurrency: 2,
            recent_timeout_count,
            circuit_state,
        },
        interventions: vec![crate::InferenceInterventionReceipt {
            id: 1,
            kind,
            occurred_at: now,
        }],
        permanent_failures: Vec::new(),
        next_cursor: 1,
    }
}

/// Route a circuit-breaker escalation with a pre-seeded
/// circuit_breaker_state observation of `signal_value` — the shared
/// setup of the recovery-signal attachment tests.
async fn escalate_with_circuit_observation(signal_value: f64) -> Arc<RecordingEscalationSink> {
    let escalation = Arc::new(RecordingEscalationSink::new());
    let mut regulation_loop =
        CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
    regulation_loop.set_alert_escalation_sink(Some(escalation.clone()));
    regulation_loop.observations.lock().insert(
        SignalMetric::CircuitBreakerState,
        Signal::new(
            LoopId::Inference,
            SignalMetric::CircuitBreakerState,
            signal_value,
            0.0,
        ),
    );
    let action = RegulatoryAction::with_metric(
        LoopId::Curation,
        ActionType::Escalate,
        RegulatoryActionParams::reason("circuit_breaker_open"),
        SignalMetric::CircuitBreakerState.as_str().to_string(),
    );
    regulation_loop.route_action_as_alert(&action).await;
    escalation
}

/// An in-memory regulation archive plus the `since` instant one second
/// before now — the algedonic-read-budget tests' shared preamble.
fn in_memory_archive() -> (
    Arc<hkask_storage::RegulationArchive>,
    chrono::DateTime<chrono::Utc>,
) {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let archive = Arc::new(
        hkask_storage::RegulationArchive::from_driver(driver).expect("regulation archive"),
    );
    let since = chrono::Utc::now() - chrono::Duration::seconds(1);
    (archive, since)
}

struct StubResilienceSource {
    observation: crate::InferenceObservation,
}

#[async_trait::async_trait]
impl crate::InferenceResilienceSource for StubResilienceSource {
    async fn observe_since(
        &self,
        cursor: u64,
    ) -> Result<crate::InferenceObservation, crate::InferenceObservationError> {
        Ok(observe_since_filtered(&self.observation, cursor))
    }
}

/// The circuit-observation test preamble: a fresh loop over the
/// resilience `source` with a recording escalation sink wired — shared
/// by the circuit-open and permanent-failure tests. The trailing
/// tick/sense call stays per test.
fn circuit_loop(
    source: Arc<dyn crate::InferenceResilienceSource>,
) -> (Arc<RecordingEscalationSink>, CyberneticsLoop) {
    let escalation = Arc::new(RecordingEscalationSink::new());
    let mut regulation = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
    regulation.set_inference_resilience_source(source);
    regulation.set_alert_escalation_sink(Some(
        Arc::clone(&escalation) as Arc<dyn crate::AlertEscalationSink>
    ));
    (escalation, regulation)
}

struct CursorRecordingResilienceSource {
    observation: crate::InferenceObservation,
    cursors: Mutex<Vec<u64>>,
}

#[async_trait::async_trait]
impl crate::InferenceResilienceSource for CursorRecordingResilienceSource {
    async fn observe_since(
        &self,
        cursor: u64,
    ) -> Result<crate::InferenceObservation, crate::InferenceObservationError> {
        self.cursors
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(cursor);
        Ok(observe_since_filtered(&self.observation, cursor))
    }
}

/// expect: "A resilience receipt remains pending until its observation is durably recorded"
/// [P9] Motivating: Homeostatic Self-Regulation
/// pre: the first attempt to persist an observed recovery fails and the second succeeds
/// post: the second observation retries the same cursor and only the third advances past the receipt
#[tokio::test]
async fn failed_inference_receipt_persistence_does_not_advance_cursor() {
    let now = chrono::Utc::now();
    let source = Arc::new(CursorRecordingResilienceSource {
        observation: circuit_intervention_observation(
            now,
            0,
            crate::InferenceCircuitState::Closed,
            crate::InferenceInterventionKind::CircuitClosed,
        ),
        cursors: Mutex::new(Vec::new()),
    });
    let sink = Arc::new(FailOnceCapturingSink {
        fail_next: std::sync::atomic::AtomicBool::new(true),
        persisted: Mutex::new(Vec::new()),
    });
    let mut regulation = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
        .with_event_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>);
    regulation.set_inference_resilience_source(Arc::clone(&source) as Arc<_>);

    regulation.sense().await;
    regulation.sense().await;
    regulation.sense().await;

    assert_eq!(
        *source
            .cursors
            .lock()
            .unwrap_or_else(|error| error.into_inner()),
        vec![0, 0, 1]
    );
    assert_eq!(
        sink.persisted
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len(),
        1
    );
}

/// expect: "A failed half-open receipt stays pending until its escalation is durably handled"
/// [P9] Motivating: Homeostatic Self-Regulation
/// pre: a circuit-reopened receipt is observed while no escalation or archive sink is available
/// post: the next observation retries from the same cursor instead of acknowledging lost evidence
#[tokio::test]
async fn failed_inference_escalation_does_not_advance_cursor() {
    let now = chrono::Utc::now();
    let source = Arc::new(CursorRecordingResilienceSource {
        observation: circuit_intervention_observation(
            now,
            1,
            crate::InferenceCircuitState::Open,
            crate::InferenceInterventionKind::CircuitReopened,
        ),
        cursors: Mutex::new(Vec::new()),
    });
    let mut regulation = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
    regulation.set_inference_resilience_source(Arc::clone(&source) as Arc<_>);

    regulation.sense().await;
    regulation.sense().await;

    assert_eq!(
        *source
            .cursors
            .lock()
            .unwrap_or_else(|error| error.into_inner()),
        vec![0, 0]
    );
}

/// expect: "A recovered inference circuit is recorded as observed recovery, not causal proof"
/// [P9] Motivating: Homeostatic Self-Regulation
/// pre: the resilience source reports a later circuit-closed receipt
/// post: regulation persists the receipt with causal attribution unverified
#[tokio::test]
async fn circuit_close_receipt_records_observed_recovery() {
    let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let now = chrono::Utc::now();
    let source = Arc::new(StubResilienceSource {
        observation: circuit_intervention_observation(
            now,
            0,
            crate::InferenceCircuitState::Closed,
            crate::InferenceInterventionKind::CircuitClosed,
        ),
    });
    let mut regulation = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
        .with_event_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>);
    regulation.set_inference_resilience_source(source);

    regulation.sense().await;

    let spans = sink.0.lock().unwrap_or_else(|error| error.into_inner());
    let recovery = spans
        .iter()
        .find(|(path, _)| path == "reg.inference.observed_recovery")
        .map(|(_, observation)| observation);
    assert_eq!(
        recovery.and_then(|value| value["causal_attribution"].as_str()),
        Some("unverified"),
        "persisted spans: {spans:?}"
    );
}

/// expect: "Initial circuit opening corrects locally without creating an operator alert"
/// [P9] Motivating: Homeostatic Self-Regulation
/// pre: the resilience boundary reports its first circuit-open transition
/// post: regulation records the transition but does not escalate before recovery is attempted
#[tokio::test]
async fn initial_inference_circuit_open_does_not_escalate() {
    let now = chrono::Utc::now();
    let source = Arc::new(StubResilienceSource {
        observation: circuit_intervention_observation(
            now,
            3,
            crate::InferenceCircuitState::Open,
            crate::InferenceInterventionKind::CircuitOpened,
        ),
    });
    let (escalation, regulation) = circuit_loop(source);

    regulation.tick().await;

    assert!(
        escalation
            .persisted
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_empty()
    );
}

/// expect: "A failed half-open recovery probe escalates once with its real condition"
/// [P9] Motivating: Homeostatic Self-Regulation
/// pre: the local resilience boundary reports that the circuit reopened
/// post: central regulation routes a native circuit-breaker escalation
#[tokio::test]
async fn reopened_inference_circuit_routes_native_escalation() {
    let now = chrono::Utc::now();
    let source = Arc::new(StubResilienceSource {
        observation: circuit_intervention_observation(
            now,
            3,
            crate::InferenceCircuitState::Open,
            crate::InferenceInterventionKind::CircuitReopened,
        ),
    });
    let escalation = Arc::new(ConfirmingEscalationSink::new());
    let event_sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let mut regulation = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
        .with_event_sink(Arc::clone(&event_sink) as Arc<dyn hkask_types::RegulationSink>);
    regulation.set_inference_resilience_source(source);
    regulation.set_alert_escalation_sink(Some(
        Arc::clone(&escalation) as Arc<dyn crate::AlertEscalationSink>
    ));

    regulation.tick().await;

    let persisted = escalation
        .persisted
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    assert_eq!(
        persisted.len(),
        2,
        "the repeated condition reaches the card sink"
    );
    assert_eq!(
        persisted.first().map(String::as_str),
        Some("circuit_breaker_open — regulatory escalation")
    );
    drop(persisted);
    let spans = event_sink
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let loop_quality = spans
        .iter()
        .find(|(path, _)| path == "reg.outcome.loop_quality")
        .map(|(_, observation)| observation);
    assert_eq!(
        loop_quality.and_then(|observation| observation["advisories_computed"].as_u64()),
        Some(1),
        "the deduplicated policy disposition must remain visible in loop-quality telemetry"
    );
}

/// expect: "An escalation whose stored trigger can never be recovered-by is manual-review-only, not a permanently unmeasurable row"
/// [P9] Motivating: Homeostatic Self-Regulation
/// pre: the reopened-circuit escalation routes while the latest
///      circuit_breaker_state observation is the previous pass's healthy
///      snapshot (value == set-point)
/// post: the persisted escalation carries a null recovery signal — the
///       reconcile pass neither warns per tick nor treats the row as
///       recoverable-at-threshold
#[tokio::test]
async fn reopened_circuit_escalation_drops_unmeasurable_recovery_signal() {
    // The stale prior-pass observation the mid-sense routing looks up:
    // healthy (value == set-point), so unmeasurable as a trigger.
    let escalation = escalate_with_circuit_observation(0.0).await;

    let contexts = escalation.contexts.lock().expect("contexts");
    assert_eq!(contexts.len(), 1, "the escalation persists once");
    let context: serde_json::Value =
        serde_json::from_str(&contexts[0]).expect("error_context is JSON");
    assert!(
        context
            .get("recovery_signal")
            .is_some_and(serde_json::Value::is_null),
        "an unmeasurable observation must not anchor recovery: {context}"
    );
}

/// expect: "A measurable deviation observed for the alerted metric anchors recovery at the original threshold"
/// [P9] Motivating: Homeostatic Self-Regulation
/// pre: the latest circuit_breaker_state observation is a real deviation
///      (circuit open: value above set-point)
/// post: the persisted escalation carries that trigger so the reconcile
///       pass can auto-resolve it when the circuit closes
#[tokio::test]
async fn escalation_attaches_measurable_recovery_signal() {
    let escalation = escalate_with_circuit_observation(1.0).await;

    let contexts = escalation.contexts.lock().expect("contexts");
    assert_eq!(contexts.len(), 1, "the escalation persists once");
    let context: serde_json::Value =
        serde_json::from_str(&contexts[0]).expect("error_context is JSON");
    let trigger = context
        .get("recovery_signal")
        .and_then(serde_json::Value::as_object)
        .expect("measurable trigger is attached");
    assert_eq!(trigger["metric"], "circuit_breaker_state");
    assert_eq!(trigger["value"], 1.0);
    assert_eq!(trigger["set_point"], 0.0);
}

/// expect: "Permanent inference failures escalate for operator correction without opening a transient circuit"
/// [P9] Motivating: Homeostatic Self-Regulation
/// pre: the resilience source reports an authorization failure while the circuit is closed
/// post: one native escalation carries the failure class and no unwired-action wording
#[tokio::test]
async fn permanent_inference_failure_routes_native_escalation() {
    let now = chrono::Utc::now();
    let source = Arc::new(StubResilienceSource {
        observation: crate::InferenceObservation {
            snapshot: crate::InferenceSnapshot {
                observed_at: now,
                in_flight: 0,
                max_concurrency: 2,
                recent_timeout_count: 0,
                circuit_state: crate::InferenceCircuitState::Closed,
            },
            interventions: Vec::new(),
            permanent_failures: vec![crate::InferencePermanentFailureReceipt {
                id: 1,
                kind: crate::InferencePermanentFailureKind::Authorization,
                detail: "provider rejected credential".to_string(),
                occurred_at: now,
            }],
            next_cursor: 1,
        },
    });
    let (escalation, regulation) = circuit_loop(source);

    regulation.sense().await;

    let persisted = escalation
        .persisted
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    assert_eq!(persisted.len(), 1);
    assert_eq!(
        persisted.first().map(String::as_str),
        Some(
            "inference_permanent_failure:Authorization — provider rejected credential — regulatory escalation"
        )
    );
}

/// The loop-quality telemetry row names the deviating metric(s), not
/// just counts them — a count-only row left the operator unable to see
/// WHICH signal deviated (observed live through 2026-11-02: the
/// weeks-long `deviations: 1, advisories_computed: 0` signature required
/// board-card archaeology to identify the signal). The array is always
/// present; empty is the explicit zero-deviation reading (the four-way
/// absence discipline).
#[test]
fn loop_quality_telemetry_names_deviating_metrics() {
    let (runtime, ledger, sink) = capturing_runtime();
    runtime.block_on(async {
        ledger.read().await.record_agent_turn(true, true).await; // one substantive native-first turn: rate 1.0 > default 0.50
        let regulation_loop = CyberneticsLoop::new(ledger)
            .with_event_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>);
        regulation_loop.tick().await;

        let spans = sink.0.lock().unwrap_or_else(|e| e.into_inner());
        let (path, observation) = spans
            .iter()
            .find(|(path, _)| path == "reg.outcome.loop_quality")
            .expect("the first-appearance deviation tick emits");
        assert_eq!(path, "reg.outcome.loop_quality");
        assert_eq!(
            observation["deviating_metrics"][0]["metric"],
            serde_json::json!("native_first_rate"),
            "the row names the deviating metric, not just its count"
        );
        assert_eq!(
            observation["deviations"].as_u64().unwrap_or_default(),
            1,
            "path {path}"
        );
    });
}

/// Zero deviations is an explicit empty `deviating_metrics` array — never
/// an absent field (absence would read as instrument failure, the
/// `unwrap_or(0)` trap).
#[test]
fn loop_quality_telemetry_names_no_deviations_explicitly() {
    let (runtime, ledger, sink) = capturing_runtime();
    runtime.block_on(async {
        let regulation_loop = CyberneticsLoop::new(ledger)
            .with_event_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>);
        regulation_loop.tick().await;

        let spans = sink.0.lock().unwrap_or_else(|e| e.into_inner());
        let (_, observation) = spans
            .iter()
            .find(|(path, _)| path == "reg.outcome.loop_quality")
            .expect("the tick-1 idle heartbeat emits");
        assert_eq!(
            observation["deviating_metrics"],
            serde_json::json!([]),
            "zero deviations is an explicit empty array, not an absent field"
        );
    });
}

/// The heartbeat/telemetry test preamble: a fresh tokio runtime, a
/// default ledger, and a capturing event sink — the pieces the tick-loop
/// tests block on. The loop construction stays per test: the resilience
/// wiring differs and is load-bearing (its absence is the signal test's
/// deviation source).
fn capturing_runtime() -> (
    tokio::runtime::Runtime,
    Arc<RwLock<RegulationLedger>>,
    Arc<CapturingSink>,
) {
    (
        tokio::runtime::Runtime::new().expect("tokio runtime"),
        Arc::new(RwLock::new(RegulationLedger::default())),
        Arc::new(CapturingSink(Mutex::new(Vec::new()))),
    )
}

/// Idle cycles emit exactly one heartbeat span per hour (tick 1, then
/// every 360 ticks) carrying the all-zero payload plus `heartbeat: true`,
/// `tick_count`, and the alert log's fill state. Without it, a converged
/// loop and a dead ticker are indistinguishable — both produce archive
/// silence.
#[test]
fn idle_loop_emits_hourly_heartbeat_span() {
    let (runtime, ledger, sink) = capturing_runtime();
    runtime.block_on(async {
        let mut regulation_loop = CyberneticsLoop::new(ledger)
            .with_event_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>);
        regulation_loop.set_inference_resilience_source(Arc::new(HealthyResilienceSource));

        // 361 ticks: heartbeat at tick 1, silence through tick 359,
        // heartbeat at tick 360, tick 361 silent again.
        for _ in 0..361 {
            regulation_loop.tick().await;
        }

        let spans = sink.0.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            spans.len(),
            2,
            "361 idle ticks must emit exactly 2 spans (tick 1 + tick 360)"
        );
        for (index, (path, observation)) in spans.iter().enumerate() {
            assert_eq!(path, "reg.outcome.loop_quality");
            assert_eq!(
                observation.get("heartbeat"),
                Some(&serde_json::json!(true)),
                "idle span {index} must be a heartbeat"
            );
            assert_eq!(
                observation
                    .get("advisories_computed")
                    .and_then(|v| v.as_u64()),
                Some(0)
            );
            assert!(observation.get("advice_review_progress_score").is_none());
            // The heartbeat carries the alert log's fill state so the
            // cap trend is visible from any session, not just Curator
            // sessions with the curator_status agent tool.
            assert_eq!(
                observation.get("alert_log_count").and_then(|v| v.as_u64()),
                Some(0),
                "idle span {index} reports an empty alert log"
            );
            assert!(
                observation
                    .get("alert_log_cap")
                    .and_then(|v| v.as_u64())
                    .is_some_and(|cap| cap >= 1),
                "idle span {index} carries the configured cap"
            );
            assert_eq!(
                observation.get("alert_log_approaching_cap"),
                Some(&serde_json::json!(false)),
                "an empty log is not approaching its cap"
            );
        }
        assert_eq!(
            spans[0].1.get("tick_count").and_then(|v| v.as_u64()),
            Some(1),
            "first heartbeat is the boot announcement"
        );
        assert_eq!(
            spans[1].1.get("tick_count").and_then(|v| v.as_u64()),
            Some(360),
            "second heartbeat lands on the hourly boundary"
        );
    });
}

/// Signal-bearing cycles emit the normal telemetry span WITHOUT the
/// heartbeat discriminator — the flag must never leak into real signal
/// events, or triage would read a live deviation as an idle heartbeat.
/// Ticks 1-3 sit inside the model-wiring grace window (idle; tick 1 is
/// the boot heartbeat); ticks 4-5 sense the unwired-model deviation.
#[test]
fn signal_ticks_emit_telemetry_without_heartbeat_flag() {
    let (runtime, ledger, sink) = capturing_runtime();
    runtime.block_on(async {
        // No inference health source: after the 3-tick grace the loop
        // senses the model-unavailable deviation on every tick.
        let regulation_loop = CyberneticsLoop::new(ledger)
            .with_event_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>);

        for _ in 0..5 {
            regulation_loop.tick().await;
        }

        let spans = sink.0.lock().unwrap_or_else(|e| e.into_inner());
        let heartbeats: Vec<&serde_json::Value> = spans
            .iter()
            .filter(|(path, observation)| {
                path == "reg.outcome.loop_quality" && observation.get("heartbeat").is_some()
            })
            .map(|(_, observation)| observation)
            .collect();
        let signal_telemetry: Vec<&serde_json::Value> = spans
            .iter()
            .filter(|(path, observation)| {
                path == "reg.outcome.loop_quality" && observation.get("heartbeat").is_none()
            })
            .map(|(_, observation)| observation)
            .collect();
        assert_eq!(
            heartbeats.len(),
            1,
            "tick 1 is the boot heartbeat; ticks 2-3 are silent (grace, idle)"
        );
        assert_eq!(
            heartbeats[0].get("tick_count").and_then(|v| v.as_u64()),
            Some(1)
        );
        assert!(
            !signal_telemetry.is_empty(),
            "post-grace ticks must emit signal telemetry"
        );
        assert_eq!(
            signal_telemetry[0]
                .get("deviations")
                .and_then(|v| v.as_u64()),
            Some(1),
            "tick 4 (first post-grace tick) senses exactly the model deviation"
        );
        assert!(
            signal_telemetry.iter().all(|observation| {
                observation
                    .get("deviations")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                    >= 1
            }),
            "signal telemetry carries deviations, never the heartbeat flag"
        );
    });
}

/// expect: "Alert reconciliation receives fresh observations but produces no advice-review event" [P9]
#[tokio::test]
async fn alert_reconciliation_does_not_publish_advice_review() {
    struct ReconciliationSink(std::sync::atomic::AtomicUsize);
    #[async_trait::async_trait]
    impl crate::AlertEscalationSink for ReconciliationSink {
        async fn reconcile_conditions(
            &self,
            _observations: &[Signal],
        ) -> Result<(), crate::AlertPersistError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        async fn try_persist_alert(
            &self,
            _output: &str,
            _confidence: f64,
            _error_context: &str,
        ) -> Result<crate::AlertDeliveryOutcome, crate::AlertPersistError> {
            Ok(crate::AlertDeliveryOutcome::Confirmed(None))
        }
    }

    let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
    let event_sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let advice_sink = Arc::new(ReconciliationSink(std::sync::atomic::AtomicUsize::new(0)));
    let mut regulation_loop = CyberneticsLoop::new(ledger)
        .with_event_sink(event_sink.clone() as Arc<dyn hkask_types::RegulationSink>);
    regulation_loop.set_alert_escalation_sink(Some(advice_sink.clone()));

    for _ in 0..5 {
        regulation_loop.tick().await;
    }
    let before = event_sink
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .filter(|(path, _)| path == "reg.outcome.loop_quality")
        .count();
    regulation_loop.tick().await;
    let spans = event_sink
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    assert_eq!(advice_sink.0.load(std::sync::atomic::Ordering::SeqCst), 6);
    assert_eq!(spans.len(), before);
    assert!(spans.iter().all(|(path, observation)| {
        path != "reg.outcome.advice_review_observed"
            && observation.get("interventions_confirmed").is_none()
    }));
}

/// expect: "Telemetry coalescing suppresses only exact steady state; changes and clearing emit immediately" [P9]
#[test]
fn loop_telemetry_coalescer_preserves_transitions_and_reports_suppression() {
    let regulation_loop = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
    let signal = Signal::new(LoopId::Cybernetics, SignalMetric::ToolReliability, 0.2, 0.8);
    let first = Deviation::from_signal(&signal).expect("deviation");
    let first_decision =
        regulation_loop.loop_telemetry_decision(std::slice::from_ref(&first), &[], 4);
    assert!(first_decision.emit);
    assert!(!first_decision.steady_state_heartbeat);

    let duplicate = regulation_loop.loop_telemetry_decision(std::slice::from_ref(&first), &[], 5);
    assert!(!duplicate.emit);

    let mut changed_signal = signal;
    changed_signal.value = 0.3;
    let changed = Deviation::from_signal(&changed_signal).expect("changed deviation");
    let changed_decision =
        regulation_loop.loop_telemetry_decision(std::slice::from_ref(&changed), &[], 6);
    assert!(changed_decision.emit);
    assert_eq!(changed_decision.suppressed_cycles, 1);

    let cleared = regulation_loop.loop_telemetry_decision(&[], &[], 8);
    assert!(cleared.emit);
    assert!(cleared.condition_cleared);
    assert!(!regulation_loop.loop_telemetry_decision(&[], &[], 9).emit);

    assert!(
        regulation_loop
            .loop_telemetry_decision(std::slice::from_ref(&first), &[], 10)
            .emit
    );
    let steady = regulation_loop.loop_telemetry_decision(&[first], &[], 360);
    assert!(steady.emit);
    assert!(steady.steady_state_heartbeat);
    assert_eq!(steady.suppressed_cycles, 1);
}

/// expect: "A persistent condition warns once on the log surface, not per tick" [P9 §9.4]
/// The warn arms' log surface follows the telemetry coalescer's transition
/// discipline: entry warns, an unchanged reading is coalesced, a changed
/// reading re-warns, and leaving deviation re-arms the latch. Observed
/// motive (2026-10-31): the native-first warn repeated byte-identically on
/// every 10-second tick for six minutes while its 11-sample window sat
/// frozen between turns — the escalation path dedups onto the open board
/// card, but the log line had no latch.
#[test]
fn condition_warn_latch_classifies_transitions() {
    let regulation_loop = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
    let signal = Signal::new(LoopId::Cybernetics, SignalMetric::NativeFirstRate, 0.7, 0.5);
    assert!(
        regulation_loop.condition_warn_transition(&signal),
        "a condition's first occurrence warns"
    );
    assert!(
        !regulation_loop.condition_warn_transition(&signal),
        "an unchanged persistent condition is coalesced"
    );
    let mut changed = signal.clone();
    changed.value = 0.8;
    assert!(
        regulation_loop.condition_warn_transition(&changed),
        "a changed reading re-warns immediately"
    );
    // Clearing re-arms: the announcement drops the latch, so a recurrence
    // warns again rather than being silenced forever.
    regulation_loop.report_cleared_conditions(&[]);
    assert!(
        regulation_loop.condition_warn_transition(&signal),
        "a recurrence after clearing warns again"
    );
    assert!(
        !regulation_loop.condition_warn_transition(&signal),
        "and its persistence is coalesced again"
    );
}

/// Both condition-warn arms (tool reliability, native-first deviation)
/// route their warns through the §9.4 latch: the first action
/// build consumes the transition, so the identical next cycle coalesces
/// instead of re-warning. Driven through `build_regulation_action` so the
/// pin covers the arm wiring itself, not just the helper.
#[test]
fn warn_arms_route_through_the_condition_latch() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let regulation_loop = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        let policy = RegulationPolicy::default();
        for (metric, value, set_point) in [
            (SignalMetric::ToolReliability, 0.2, 0.8),
            (SignalMetric::NativeFirstRate, 0.7, 0.5),
        ] {
            let signal = Signal::new(LoopId::Cybernetics, metric, value, set_point);
            let deviation = Deviation::from_signal(&signal)
                .unwrap_or_else(|| panic!("{metric:?} must deviate from its set point"));
            for proposed in policy.decide(&deviation) {
                regulation_loop
                    .build_regulation_action(&deviation, proposed)
                    .await
                    .unwrap_or_else(|| panic!("{metric:?} warn arm still builds its action"));
            }
            assert!(
                !regulation_loop.condition_warn_transition(&signal),
                "{metric:?}: the first action build consumed the transition, so the identical next cycle coalesces"
            );
        }
    });
}

fn fallback_alert_action() -> RegulatoryAction {
    RegulatoryAction::with_metric(
        LoopId::Curation,
        ActionType::Escalate,
        RegulatoryActionParams::reason("persistent_fallback_condition"),
        "inference_model_available".to_string(),
    )
}

/// expect: "Fallback dedup clears with the condition so a later recurrence emits again" [P9]
#[tokio::test]
async fn fallback_alert_condition_clearing_allows_recurrence() {
    let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let regulation_loop = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
        .with_event_sink(sink.clone() as Arc<dyn hkask_types::RegulationSink>);
    let action = fallback_alert_action();

    regulation_loop.act(std::slice::from_ref(&action)).await;
    regulation_loop.act(std::slice::from_ref(&action)).await;
    regulation_loop.act(&[]).await;
    regulation_loop.act(std::slice::from_ref(&action)).await;

    let alerts = sink
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .filter(|(path, _)| path == "reg.variety.algedonic_alert")
        .count();
    assert_eq!(alerts, 2, "first occurrence and post-clear recurrence emit");
}

/// expect: "Failed fallback persistence does not arm dedup and retries the same condition" [P9]
#[tokio::test]
async fn fallback_alert_persistence_failure_remains_retryable() {
    let sink = Arc::new(FailOnceCapturingSink {
        fail_next: std::sync::atomic::AtomicBool::new(true),
        persisted: Mutex::new(Vec::new()),
    });
    let regulation_loop = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
        .with_event_sink(sink.clone() as Arc<dyn hkask_types::RegulationSink>);
    let action = fallback_alert_action();

    assert!(!regulation_loop.route_action_as_alert(&action).await);
    assert!(regulation_loop.route_action_as_alert(&action).await);
    assert!(regulation_loop.route_action_as_alert(&action).await);
    assert_eq!(
        sink.persisted
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len(),
        1,
        "one successful retention arms dedup; the failed attempt does not"
    );
}

/// expect: "Board-unavailable fallback preserves one alert per persistent condition instead of displacing earlier informative events" [P9]
#[tokio::test(start_paused = true)]
async fn persistent_fallback_alerts_do_not_displace_prior_informative_events() {
    let (archive, since) = in_memory_archive();
    let informative = hkask_types::RegulationRecord::new(
        WebID::from_persona(b"regulation"),
        hkask_types::event::Span::from_kind(hkask_types::event::SpanKind::ToolOutcomeBreakdown),
        hkask_types::event::CyclePhase::Act,
        serde_json::json!({"informative_before_persistent_condition": true}),
        0,
    );
    hkask_types::RegulationSink::persist(&*archive, &informative)
        .expect("persist informative event");
    let regulation_loop = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
        .with_event_sink(archive.clone() as Arc<dyn hkask_types::RegulationSink>);

    // No board sink is wired: this exercises the archive fallback,
    // not the production queue's pending-condition deduplication.
    for _ in 0..520 {
        regulation_loop.tick().await;
    }

    let visible = archive
        .query_recent_algedonic(since, 500)
        .expect("recent algedonic log");
    assert!(
        visible
            .iter()
            .any(|event| { event.observation["informative_before_persistent_condition"] == true }),
        "one fallback condition must not consume the newest-first read budget"
    );
}

/// expect: "Identical persistent signal cycles cannot consume the algedonic read budget and hide a later novel event" [P9]
#[tokio::test(start_paused = true)]
async fn unchanged_signal_telemetry_does_not_displace_later_novel_events() {
    struct PendingConditionSink;
    #[async_trait::async_trait]
    impl crate::AlertEscalationSink for PendingConditionSink {
        async fn try_persist_alert(
            &self,
            _output: &str,
            _confidence: f64,
            _error_context: &str,
        ) -> Result<crate::AlertDeliveryOutcome, crate::AlertPersistError> {
            Ok(crate::AlertDeliveryOutcome::Confirmed(None))
        }
    }

    let (archive, since) = in_memory_archive();
    let mut regulation_loop =
        CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
            .with_event_sink(archive.clone() as Arc<dyn hkask_types::RegulationSink>);
    regulation_loop.set_alert_escalation_sink(Some(Arc::new(PendingConditionSink)));

    // After the three-tick startup grace, the unwired-model deviation is
    // identical on every tick. More than the curator's 500-row read budget
    // must not hide a later, informative event.
    for _ in 0..520 {
        regulation_loop.tick().await;
    }
    let novel = hkask_types::RegulationRecord::new(
        WebID::from_persona(b"regulation"),
        hkask_types::event::Span::from_kind(hkask_types::event::SpanKind::ToolOutcomeBreakdown),
        hkask_types::event::CyclePhase::Act,
        serde_json::json!({"novel_after_persistent_condition": true}),
        0,
    );
    hkask_types::RegulationSink::persist(&*archive, &novel).expect("persist novel event");

    let visible = archive.query_algedonic(since, 500).expect("algedonic log");
    assert!(
        visible
            .iter()
            .any(|event| { event.observation["novel_after_persistent_condition"] == true }),
        "steady-state telemetry must leave room for a later novel event within the 500-row read budget"
    );
    let steady_heartbeat = visible
        .iter()
        .find(|event| event.observation["steady_state_heartbeat"] == true)
        .expect("persistent-condition heartbeat");
    assert!(
        steady_heartbeat.observation["suppressed_steady_state_cycles"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "suppression must be observable on the persistent-condition heartbeat"
    );
}

/// The `AlgedonicEvents` set point reads the loop's `SetPoints` field —
/// `algedonic_events_max` — instead of the hardcoded 0.0 literal (algedonic
/// card f69162f4, option (B), operator ruling 2026-10-19: the knob exists,
/// the policy is unchanged until the operator sets it). A configured
/// tolerance below the observed count is a deviation; a tolerance at or
/// above it is not.
#[test]
fn algedonic_events_set_point_reads_the_configured_tolerance() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let ledger = Arc::new(RwLock::new(RegulationLedger::default()));

        // One actionable (Warning-or-Critical) alert in the log: a 0% success
        // Critical over 5 operations — the same fixture shape the
        // sense_reports_algedonic_log_population pin uses.
        {
            let ledger_guard = ledger.read().await;
            for _ in 0..5 {
                ledger_guard.record_outcome("algedonic_tolerance_test", false, None).await;
            }
            let log_count = ledger_guard.alert_log_count().await;
            drop(ledger_guard);
            assert!(
                log_count > 0,
                "sanity: the 0% fixture produced an actionable alert"
            );
        }

        // The default loop deviates on 1 actionable alert (set point 0 — the
        // unchanged policy).
        let default_loop = CyberneticsLoop::new(Arc::clone(&ledger));
        let default_signals = default_loop.sense().await;
        let signal = default_signals
            .iter()
            .find(|s| s.metric == SignalMetric::AlgedonicEvents)
            .expect("AlgedonicEvents must be sensed");
        assert_eq!(signal.set_point, 0.0);
        assert!(
            Deviation::from_signal(signal).is_some(),
            "default tolerance 0 — one actionable alert still deviates (option (B) alters no behavior)"
        );

        // A configured tolerance at or above the observed count must not
        // deviate: the knob calibrates the metric without a recompile.
        let mut points = crate::set_points::SetPoints::default();
        points.algedonic_events_max = 1;
        let calibrated_loop = CyberneticsLoop::with_set_points(Arc::clone(&ledger), points);
        let calibrated_signals = calibrated_loop.sense().await;
        let signal = calibrated_signals
            .iter()
            .find(|s| s.metric == SignalMetric::AlgedonicEvents)
            .expect("AlgedonicEvents must be sensed");
        assert_eq!(signal.set_point, 1.0);
        assert_eq!(signal.value, 1.0);
        assert!(
            Deviation::from_signal(signal).is_none(),
            "a configured tolerance at the observed count must not deviate — the knob calibrates without a recompile"
        );
    });
}
