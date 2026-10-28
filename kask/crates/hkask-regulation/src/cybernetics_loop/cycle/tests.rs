use crate::CyberneticsLoop;
use crate::cybernetics_loop::RolloutImpactCheck;
use crate::cybernetics_loop::test_support::CapturingSink;
use crate::loops::{
    ActionDecision, ActionType, Deviation, DeviationDirection, LoopId, RegulationData,
    RegulatoryAction, RegulatoryActionParams, Signal, SignalMetric,
};
use crate::regulation_policy::RegulationPolicy;
use crate::runtime::RegulationLedger;
use crate::{RolloutEventError, RolloutEventSource, RolloutMetricObservation};
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

/// A recorded `append_impact_verdict` call — exactly what the loop wrote back.
#[derive(Debug, Clone, PartialEq)]
struct RecordedVerdict {
    rollout_id: String,
    metric: String,
    before: f64,
    after: f64,
    improved: bool,
    decision: String,
}

/// A test double for `RolloutEventSource` that returns a configured
/// `metric_observation` result and records `append_impact_verdict`
/// calls so the test can assert exactly what the loop persisted.
///
/// `metric_observation` is configurable (Ok(Some) / Ok(None) / Err)
/// to exercise the three event-substrate outcomes. The no-data and error
/// branches must not record a verdict.
struct MockRolloutEventSource {
    /// `Err` holds only the failure detail; the typed
    /// [`RolloutEventError`] is built at the trait boundary so the mock
    /// does not force `Clone` onto the port's error type.
    before_after: Mutex<Result<Option<(f64, f64)>, String>>, // string-error-ok
    verdicts: Mutex<Vec<RecordedVerdict>>,
    sample_sizes: Mutex<Option<(u64, u64)>>,
}

impl MockRolloutEventSource {
    fn answering(before: f64, after: f64) -> Self {
        Self {
            before_after: Mutex::new(Ok(Some((before, after)))),
            verdicts: Mutex::new(Vec::new()),
            sample_sizes: Mutex::new(None),
        }
    }
    fn with_sample_sizes(self, before: u64, after: u64) -> Self {
        *self.sample_sizes.lock().expect("sample_sizes lock") = Some((before, after));
        self
    }
    fn empty() -> Self {
        Self {
            before_after: Mutex::new(Ok(None)),
            verdicts: Mutex::new(Vec::new()),
            sample_sizes: Mutex::new(None),
        }
    }
    fn failing(error: &str) -> Self {
        Self {
            before_after: Mutex::new(Err(error.to_string())),
            verdicts: Mutex::new(Vec::new()),
            sample_sizes: Mutex::new(None),
        }
    }
    fn recorded(&self) -> Vec<RecordedVerdict> {
        self.verdicts.lock().expect("verdicts lock").clone()
    }
}

impl RolloutEventSource for MockRolloutEventSource {
    fn metric_observation(
        &self,
        _rollout_id: &str,
        _metric: &str,
        _before_position: i64,
    ) -> Result<Option<RolloutMetricObservation>, RolloutEventError> {
        self.before_after
            .lock()
            .expect("before_after lock")
            .clone()
            .map(|values| {
                values.map(|(before, after)| RolloutMetricObservation {
                    before,
                    after,
                    sample_sizes: *self.sample_sizes.lock().expect("sample_sizes lock"),
                })
            })
            .map_err(|detail| RolloutEventError::Query { detail })
    }
    fn append_impact_verdict(
        &self,
        rollout_id: &str,
        metric: &str,
        before: f64,
        after: f64,
        improved: bool,
        decision: &str,
    ) -> Result<(), RolloutEventError> {
        self.verdicts
            .lock()
            .expect("verdicts lock")
            .push(RecordedVerdict {
                rollout_id: rollout_id.to_string(),
                metric: metric.to_string(),
                before,
                after,
                improved,
                decision: decision.to_string(),
            });
        Ok(())
    }
}

fn loop_with_source<S: RolloutEventSource + 'static>(source: Arc<S>) -> CyberneticsLoop {
    let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
    CyberneticsLoop::new(ledger).with_rollout_event_source(source)
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

struct LatchingEscalationSink {
    pending: Mutex<bool>,
    persisted: Mutex<Vec<String>>,
}

impl LatchingEscalationSink {
    fn new() -> Self {
        Self {
            pending: Mutex::new(false),
            persisted: Mutex::new(Vec::new()),
        }
    }

    fn set_pending(&self, pending: bool) {
        *self.pending.lock().expect("pending lock") = pending;
    }
}

#[async_trait::async_trait]
impl crate::AlertEscalationSink for LatchingEscalationSink {
    async fn try_persist_alert(
        &self,
        output: &str,
        _confidence: f64,
        _error_context: &str,
    ) -> Result<crate::AlertDeliveryOutcome, crate::AlertPersistError> {
        self.persisted
            .lock()
            .expect("persisted lock")
            .push(output.to_string());
        if *self.pending.lock().expect("pending lock") {
            Ok(crate::AlertDeliveryOutcome::Confirmed(None))
        } else {
            Ok(crate::AlertDeliveryOutcome::Attempted)
        }
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
/// does not re-alert without new exhaustion; and the reset alone earns
/// no review-card write — the exhaustion alert is transient
/// (`escalated: false`) and does not represent a review condition.
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

    // Control: resetting the cap does not create a review card for the
    // transient alert.
    assert!(
        escalation.persisted.lock().expect("persisted").is_empty(),
        "the transient exhaustion alert must not create a review card"
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

fn rollout_impact_check(rollout_id: &str, metric: &str) -> RolloutImpactCheck {
    RolloutImpactCheck {
        rollout_id: rollout_id.to_string(),
        before_position: 1,
        metric: metric.to_string(),
        read_attempts: 0,
    }
}

async fn verify_checks(
    regulation: &CyberneticsLoop,
    checks: &[RolloutImpactCheck],
) -> Vec<super::ImpactReport> {
    let preparation = regulation.prepare_impact_checks(checks);
    assert!(
        preparation.retry.is_empty(),
        "direct verification must not silently retry"
    );
    assert!(
        preparation.exhausted.is_empty(),
        "direct verification must not silently drop exhausted checks"
    );
    regulation.verify_impact(&preparation.ready).await
}

/// While a plateau card is open, re-detections comment on that card
/// without replaying the span or live alert. The stagnation detector
/// keeps counting; verification closes the card and permits a new one.
#[tokio::test]
async fn plateau_alert_latches_while_pending() {
    let source = Arc::new(MockRolloutEventSource::answering(0.2, 0.2));
    let mut regulation = loop_with_source(source.clone());
    let archive = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let escalation = Arc::new(LatchingEscalationSink::new());
    regulation.set_event_sink(Arc::clone(&archive) as Arc<dyn hkask_types::RegulationSink>);
    regulation.set_alert_escalation_sink(Some(escalation.clone()));

    let action = rollout_impact_check("trace", "tool_reliability");
    // Constant before/after — every cycle is ineffective, so the
    // stagnation detector reaches its threshold and the plateau fires.
    *source.before_after.lock().expect("source") = Ok(Some((0.2, 0.2)));
    let stagnation_threshold = regulation
        .stagnation_detector
        .threshold_for_metric("tool_reliability");
    for _ in 0..stagnation_threshold {
        verify_checks(&regulation, std::slice::from_ref(&action)).await;
    }
    let plateau_spans = || {
        archive
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(path, _)| path == "reg.outcome.plateau_detected")
            .count()
    };
    assert_eq!(
        escalation.persisted.lock().expect("persisted").len(),
        1,
        "the first plateau detection persists one escalation"
    );
    assert_eq!(plateau_spans(), 1, "the first detection emits one span");

    // The operator now has an open card: further detections reach
    // its comment path, not the span or live alert.
    escalation.set_pending(true);
    for _ in 0..3 {
        verify_checks(&regulation, std::slice::from_ref(&action)).await;
    }
    assert_eq!(
        escalation.persisted.lock().expect("persisted").len(),
        4,
        "three repeats must be delivered to the same condition card"
    );
    assert_eq!(
        plateau_spans(),
        1,
        "latched re-detections must not re-emit the plateau span"
    );
    assert!(
        regulation
            .stagnation_detector
            .ineffective_count("tool_reliability", "Notify")
            >= stagnation_threshold,
        "the stagnation detector keeps counting while latched — resolving \
         the escalation re-fires the next detection"
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

/// expect: "Accepted constant degradation/noise accumulates stagnation; only observed progress resets it" [P9]
#[tokio::test]
async fn accepted_noise_does_not_erase_stagnation_or_imply_progress() {
    use crate::loops::core::{LoopMetrics, TriggerOrigin};
    let source = Arc::new(MockRolloutEventSource::answering(0.2, 0.2));
    let regulation = loop_with_source(source.clone());
    let action = rollout_impact_check("trace", "tool_reliability");
    for (index, after) in [0.2, 0.199, 0.2, 0.2].into_iter().enumerate() {
        *source.before_after.lock().expect("source") = Ok(Some((0.2, after)));
        let reports = verify_checks(&regulation, std::slice::from_ref(&action)).await;
        let report = reports.first().expect("verified");
        assert_eq!(report.decision, ActionDecision::Accept);
        assert!(!report.improved);
        assert_eq!(
            regulation
                .stagnation_detector
                .ineffective_count("tool_reliability", ActionType::Notify.as_str()),
            (index + 1) as u32
        );
        assert_eq!(
            LoopMetrics::from_cycle(0, &[], &[], &reports, TriggerOrigin::Scheduled)
                .rollout_progress_score,
            Some(0.0)
        );
    }
    *source.before_after.lock().expect("source") = Ok(Some((0.2, 0.3)));
    let reports = verify_checks(&regulation, std::slice::from_ref(&action)).await;
    assert!(reports.first().expect("report").improved);
    assert_eq!(
        regulation
            .stagnation_detector
            .ineffective_count("tool_reliability", ActionType::Notify.as_str()),
        0
    );
}

/// Worsening thresholds are ratios, not raw metric units. A pass-rate
/// decline from 0.10 to 0.07 is 30% relative worsening and must cross the
/// default 20% block threshold even though the absolute delta is only 0.03.
#[tokio::test]
async fn verify_impact_classifies_relative_worsening() {
    // n large enough that 0.10 → 0.07 clears the sample-size guard.
    let source =
        Arc::new(MockRolloutEventSource::answering(0.10, 0.07).with_sample_sizes(2000, 2000));
    let escalation = Arc::new(RecordingEscalationSink::new());
    let mut regulation = loop_with_source(source);
    regulation.set_alert_escalation_sink(Some(escalation.clone()));

    let reports = verify_checks(
        &regulation,
        &[rollout_impact_check("relative", "pass_rate")],
    )
    .await;

    assert_eq!(
        reports.first().expect("impact report").decision,
        ActionDecision::Block
    );
    let messages = escalation.persisted.lock().expect("persisted lock");
    assert_eq!(messages.len(), 1, "one block alert is persisted");
    assert_eq!(
        messages.first().expect("block alert"),
        "ActionDecision::Block: impact check for pass_rate observed 30.0% relative worsening (threshold: 20.0%)",
        "the alert reports an observation, not unsupported causation"
    );
}

/// A pass-rate drop on too few attempts (3/3 → 1/3) is not Blocked and
/// raises no Critical alert; it is staged for review with the
/// insufficient-sample reason logged. Unknown sample sizes behave the same.
#[tokio::test]
async fn verify_impact_refuses_block_on_insufficient_sample() {
    for source in [
        MockRolloutEventSource::answering(1.0, 1.0 / 3.0).with_sample_sizes(3, 3),
        MockRolloutEventSource::answering(1.0, 1.0 / 3.0),
    ] {
        let escalation = Arc::new(RecordingEscalationSink::new());
        let mut regulation = loop_with_source(Arc::new(source));
        regulation.set_alert_escalation_sink(Some(escalation.clone()));

        let reports =
            verify_checks(&regulation, &[rollout_impact_check("small", "pass_rate")]).await;

        assert_eq!(
            reports.first().expect("impact report").decision,
            ActionDecision::Stage
        );
        let messages = escalation.persisted.lock().expect("persisted lock");
        assert!(
            messages
                .iter()
                .all(|m| !m.contains("ActionDecision::Block")),
            "no block alert on an insufficient sample: {messages:?}"
        );
    }
}

/// S1 + happy path: a store-answered pass_rate regression writes a verdict
/// event labeled with the REAL metric name ("pass_rate"), not a
/// fallback default. Before the fix the write-back passed
/// `metric.as_str()`, which fell through a `from_str_name` default
/// (the since-deleted energy metric) → a self-describing JSON field
/// that lied about its content.
#[test]
fn verify_impact_write_back_records_real_metric_name() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let source = Arc::new(MockRolloutEventSource::answering(0.8, 0.5));
        let regulation_loop = loop_with_source(Arc::clone(&source));
        let reports = verify_checks(
            &regulation_loop,
            &[rollout_impact_check("alpha", "pass_rate")],
        )
        .await;
        assert_eq!(
            reports.len(),
            1,
            "a store-answered check produces one report"
        );
        let verdicts = source.recorded();
        assert_eq!(verdicts.len(), 1, "the impact verdict is written back once");
        let verdict = &verdicts[0];
        assert_eq!(verdict.rollout_id, "alpha");
        // S1: the verdict records the real metric name, not the SignalMetric
        // fallback. Before the fix this asserted "energy_remaining".
        assert_eq!(verdict.metric, "pass_rate");
        assert_eq!(verdict.before, 0.8);
        assert_eq!(verdict.after, 0.5);
        assert!(!verdict.improved, "a pass-rate drop is not an improvement");
        assert!(
            matches!(verdict.decision.as_str(), "Accept" | "Stage" | "Block"),
            "decision is a known ActionDecision variant, got {}",
            verdict.decision
        );
    });
}

/// A submitted impact check the store cannot answer must surface the
/// missing baseline and produce neither a report nor a verdict write-back.
#[test]
fn verify_impact_store_no_data_skips_without_verdict() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let source = Arc::new(MockRolloutEventSource::empty());
        let regulation_loop = loop_with_source(Arc::clone(&source));
        let reports = verify_checks(
            &regulation_loop,
            &[rollout_impact_check("alpha", "pass_rate")],
        )
        .await;
        assert!(reports.is_empty(), "no report when the store has no data");
        assert!(
            source.recorded().is_empty(),
            "no verdict written back when the store has no data"
        );
    });
}

/// A store read error retains the accepted check without fabricating a verdict.
#[test]
fn verify_impact_store_error_retries_without_verdict() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let source = Arc::new(MockRolloutEventSource::failing("store down"));
        let regulation_loop = loop_with_source(Arc::clone(&source));
        let preparation =
            regulation_loop.prepare_impact_checks(&[rollout_impact_check("alpha", "pass_rate")]);
        let reports = regulation_loop.verify_impact(&preparation.ready).await;
        assert_eq!(preparation.retry.len(), 1, "the accepted check is retained");
        assert!(reports.is_empty(), "no report when the store errors");
        assert!(
            source.recorded().is_empty(),
            "no verdict written back on a store error"
        );
    });
}

#[tokio::test]
async fn accepted_impact_check_retries_a_failed_read_then_verifies_once() {
    let source = Arc::new(MockRolloutEventSource::failing("store down"));
    let regulation = loop_with_source(Arc::clone(&source));
    assert_eq!(
        regulation
            .submit_rollout_impact_check("alpha".into(), 1, "pass_rate".into())
            .await,
        crate::RolloutImpactSubmission::Accepted,
    );
    regulation.tick().await;
    assert_eq!(regulation.submitted_rollout_checks.lock().await.len(), 1);
    *source.before_after.lock().expect("source") = Ok(Some((0.8, 0.5)));
    regulation.tick().await;
    assert!(regulation.submitted_rollout_checks.lock().await.is_empty());
    assert_eq!(source.recorded().len(), 1);
}

#[tokio::test]
async fn accepted_impact_check_exhausts_bounded_read_retries() {
    let source = Arc::new(MockRolloutEventSource::failing("store down"));
    let escalation = Arc::new(RecordingEscalationSink::new());
    let mut regulation = loop_with_source(Arc::clone(&source));
    // Wire a healthy inference source so the third tick does not also
    // fire the unwired-model grace alert — this test's board traffic is
    // the exhaustion escalation alone.
    regulation.set_inference_resilience_source(Arc::new(HealthyResilienceSource));
    regulation.set_alert_escalation_sink(Some(escalation.clone()));
    assert_eq!(
        regulation
            .submit_rollout_impact_check("alpha".into(), 1, "pass_rate".into())
            .await,
        crate::RolloutImpactSubmission::Accepted,
    );
    for _ in 0..2 {
        regulation.tick().await;
        assert_eq!(regulation.submitted_rollout_checks.lock().await.len(), 1);
    }
    regulation.tick().await;
    assert!(regulation.submitted_rollout_checks.lock().await.is_empty());
    assert!(
        source.recorded().is_empty(),
        "no false verdict on exhausted reads"
    );
    let messages = escalation.persisted.lock().expect("persisted lock");
    assert_eq!(
        messages.len(),
        1,
        "the exhausted check escalates to the review board"
    );
    assert!(
        messages[0].contains("alpha"),
        "the escalation names the unverified rollout: {}",
        messages[0]
    );
}

#[tokio::test]
async fn retained_impact_checks_count_against_the_admission_bound() {
    let source = Arc::new(MockRolloutEventSource::failing("store down"));
    let regulation = loop_with_source(source);
    assert_eq!(
        regulation
            .submit_rollout_impact_check("first".into(), 1, "pass_rate".into())
            .await,
        crate::RolloutImpactSubmission::Accepted,
    );
    regulation.tick().await;
    for index in 0..63 {
        assert_eq!(
            regulation
                .submit_rollout_impact_check(format!("next-{index}"), 1, "pass_rate".into())
                .await,
            crate::RolloutImpactSubmission::Accepted,
        );
    }
    assert_eq!(
        regulation
            .submit_rollout_impact_check("blocked".into(), 1, "pass_rate".into())
            .await,
        crate::RolloutImpactSubmission::QueueFull { capacity: 64 },
    );
    regulation.tick().await;
    let queue = regulation.submitted_rollout_checks.lock().await;
    assert_eq!(queue.len(), 64);
    assert_eq!(
        queue[0].rollout_id, "first",
        "older accepted work is never displaced"
    );
}

/// Pins Fix 2: every RegulationReason that has a policy rule must produce
/// Some(action) from build_regulation_action — not None via a catch-all.
/// Before the fix, 18 of 30 reasons fell through `_ => None`, silently
/// dropping actions and leaving the loop open for those metrics. The
/// match is now exhaustive (no `_ =>` arm), so the compiler enforces
/// closure at compile time. This test verifies the behavioral side:
/// each newly-handled arm actually returns Some(action).
#[test]
fn build_regulation_action_produces_action_for_all_new_reasons() {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let regulation_loop = loop_with_source(Arc::new(MockRolloutEventSource::empty()));
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
            (NativeFirstRate, AboveSetPoint, 0.7, 0.5),
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
        let regulation_loop = loop_with_source(Arc::new(MockRolloutEventSource::empty()));
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
    let metrics =
        LoopMetrics::from_cycle(0, &[deviation], &[action], &[], TriggerOrigin::Scheduled);
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
            assert_eq!(
                observation
                    .get("rollout_impact_reports")
                    .and_then(|v| v.as_u64()),
                Some(0)
            );
            assert_eq!(
                observation.get("rollout_progress_score"),
                Some(&serde_json::Value::Null)
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

/// expect: "Telemetry coalescing suppresses only exact steady state; changes, clearing, and measured transitions emit immediately" [P9]
#[test]
fn loop_telemetry_coalescer_preserves_transitions_and_reports_suppression() {
    let regulation_loop = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
    let signal = Signal::new(LoopId::Cybernetics, SignalMetric::ToolReliability, 0.2, 0.8);
    let first = Deviation::from_signal(&signal).expect("deviation");
    let first_decision =
        regulation_loop.loop_telemetry_decision(std::slice::from_ref(&first), &[], false, 4);
    assert!(first_decision.emit);
    assert!(!first_decision.steady_state_heartbeat);

    let duplicate =
        regulation_loop.loop_telemetry_decision(std::slice::from_ref(&first), &[], false, 5);
    assert!(!duplicate.emit);

    let mut changed_signal = signal;
    changed_signal.value = 0.3;
    let changed = Deviation::from_signal(&changed_signal).expect("changed deviation");
    let changed_decision =
        regulation_loop.loop_telemetry_decision(std::slice::from_ref(&changed), &[], false, 6);
    assert!(changed_decision.emit);
    assert_eq!(changed_decision.suppressed_cycles, 1);

    let forced_measurement =
        regulation_loop.loop_telemetry_decision(std::slice::from_ref(&changed), &[], true, 7);
    assert!(
        forced_measurement.emit,
        "measured transitions are never suppressed"
    );

    let cleared = regulation_loop.loop_telemetry_decision(&[], &[], false, 8);
    assert!(cleared.emit);
    assert!(cleared.condition_cleared);
    assert!(
        !regulation_loop
            .loop_telemetry_decision(&[], &[], false, 9)
            .emit
    );

    assert!(
        regulation_loop
            .loop_telemetry_decision(std::slice::from_ref(&first), &[], false, 10)
            .emit
    );
    let steady = regulation_loop.loop_telemetry_decision(&[first], &[], false, 360);
    assert!(steady.emit);
    assert!(steady.steady_state_heartbeat);
    assert_eq!(steady.suppressed_cycles, 1);
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
