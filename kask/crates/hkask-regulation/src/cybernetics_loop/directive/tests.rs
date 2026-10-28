use super::*;
use crate::CyberneticsLoop;
use crate::cybernetics_loop::test_support::CapturingSink;
use crate::runtime::RegulationLedger;
use hkask_types::curator::EscalationSeverity;
use std::sync::{Arc, Mutex};
use tokio::sync::{RwLock, mpsc};

/// Sink whose persist always fails — the acknowledgment-failure control.
struct FailingSink;

impl hkask_types::RegulationSink for FailingSink {
    fn persist(
        &self,
        _event: &hkask_types::RegulationRecord,
    ) -> Result<(), hkask_types::InfrastructureError> {
        Err(hkask_types::InfrastructureError::Serialization(
            "sink unavailable".to_string(),
        ))
    }
}

fn directive_acks(sink: &CapturingSink) -> Vec<serde_json::Value> {
    sink.0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|(path, _)| path.contains("directive_acknowledged"))
        .map(|(_, observation)| observation.clone())
        .collect()
}

fn outcome_of(observation: &serde_json::Value) -> &str {
    observation
        .get("outcome")
        .and_then(|outcome| outcome.as_str())
        .unwrap_or("<missing>")
}

fn ack_for<'a>(acks: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
    acks.iter()
        .find(|observation| {
            observation.get("directive_type").and_then(|t| t.as_str()) == Some(name)
        })
        .unwrap_or_else(|| panic!("no acknowledgment for {name}: {acks:#?}"))
}

async fn loop_with_sink(
    sink: Arc<dyn hkask_types::RegulationSink>,
) -> (CyberneticsLoop, mpsc::UnboundedSender<CuratorDirective>) {
    let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
    let (tx, rx) = mpsc::unbounded_channel();
    let regulation_loop = CyberneticsLoop::new(ledger)
        .with_event_sink(sink)
        .with_curator_directive_channel(rx);
    (regulation_loop, tx)
}

async fn cap_status(loop_: &CyberneticsLoop, agent: WebID) -> (u32, u32) {
    loop_
        .call_cap_manager
        .read()
        .await
        .all_agent_statuses()
        .await
        .into_iter()
        .find(|(id, _)| *id == agent)
        .map(|(_, status)| (status.ceiling, status.remaining))
        .expect("agent cap registered")
}

/// Every directive variant's acknowledgment reports what actually
/// happened. Real effects are "applied" (with the state change asserted),
/// persisted requests are "recorded" exactly once, log-only variants are
/// "log_only", and variants with no handler are "unsupported". Pre-fix,
/// every non-dampened directive persisted "applied" regardless of effect,
/// and the evolve request produced a second, false "applied" record on
/// top of its honest "recorded" one.
///
/// Two loops: the metacognitive override cooldown (120s, all overrides
/// suppressed after any metacognitive directive passes dedup) would
/// dampen the cap batches after batch 1's SeekMoreEvidence — existing
/// dampener behavior, not the behavior under test.
#[tokio::test]
async fn directive_acknowledgments_report_actual_outcomes() {
    // Loop A — the variants with no call-cap effect.
    let sink_a = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let (loop_a, tx_a) =
        loop_with_sink(Arc::clone(&sink_a) as Arc<dyn hkask_types::RegulationSink>).await;

    let agent = WebID::from_persona(b"agent-a");
    for directive in [
        CuratorDirective::CalibrateThreshold {
            domain: "inference".to_string(),
            new_threshold: 42,
        },
        CuratorDirective::UpdateCapabilities {
            agent,
            additions: vec!["tool_x".to_string()],
            removals: vec![],
        },
        CuratorDirective::SeekMoreEvidence {
            context: "decision".to_string(),
            channel: "llm_confidence".to_string(),
            confidence: "0.5".to_string(),
        },
        CuratorDirective::EscalateDomain {
            domain: "storage".to_string(),
            severity: EscalationSeverity::Warning,
            evidence: "variety deficit".to_string(),
        },
        CuratorDirective::EvolveMcpToolSchema {
            server_name: "hkask-mcp-scenarios".to_string(),
            tool_name: "dcf_valuation".to_string(),
            evolution_type: SchemaEvolutionType::AddField,
            field_name: "discount_rate".to_string(),
            new_type: Some("f64".to_string()),
            rationale: "skill-use reports omit the rate".to_string(),
            evidence: "skill dcf step 3".to_string(),
        },
    ] {
        tx_a.send(directive).expect("send directive");
    }
    loop_a.process_inbox().await;

    let acks = directive_acks(&sink_a);
    assert_eq!(
        acks.len(),
        5,
        "exactly one acknowledgment per directive — the evolve request must \
         not also produce a generic applied record: {acks:#?}"
    );

    // Real effect → applied.
    assert_eq!(outcome_of(ack_for(&acks, "calibrate_threshold")), "applied");
    // Log-only variants → log_only (no capability-mutation or
    // evidence-seeking handler exists — inventing one is forbidden).
    assert_eq!(
        outcome_of(ack_for(&acks, "update_capabilities")),
        "log_only",
        "update_capabilities has no effect handler — must not be acknowledged as applied"
    );
    assert_eq!(
        outcome_of(ack_for(&acks, "seek_more_evidence")),
        "log_only",
        "seek_more_evidence has no effect handler — must not be acknowledged as applied"
    );
    // No escalation sink wired in this loop → missing_sink (the escalation sink is wired
    // the delivery path; the no-sink state is surfaced, not claimed
    // as delivered).
    assert_eq!(
        outcome_of(ack_for(&acks, "escalate_domain")),
        "missing_sink",
        "escalate_domain with no sink wired must surface missing_sink, not claim delivery"
    );
    // Persisted request → recorded, exactly once, with the payload.
    let evolve = ack_for(&acks, "evolve_mcp_tool_schema");
    assert_eq!(outcome_of(evolve), "recorded");
    assert_eq!(
        evolve.get("field_name").and_then(|f| f.as_str()),
        Some("discount_rate"),
        "the recorded request must carry the evolution payload"
    );

    // Loop B — the call-cap variants, each batch with a real state
    // assertion. Credit saturates at the ceiling, so calls are consumed
    // first to make the replenish observable.
    let sink_b = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let (loop_b, tx_b) =
        loop_with_sink(Arc::clone(&sink_b) as Arc<dyn hkask_types::RegulationSink>).await;
    let cap_manager = loop_b.call_cap_manager.read().await;
    cap_manager.register_call_cap(agent, 10).await;
    for _ in 0..8 {
        cap_manager.charge_metered(&agent).await;
    }
    drop(cap_manager);
    assert_eq!(cap_status(&loop_b, agent).await, (10, 2));

    // Replenish: remaining calls must increase.
    tx_b.send(CuratorDirective::ReplenishBudget {
        agent,
        amount: 3,
        priority: None,
    })
    .expect("send replenish");
    loop_b.process_inbox().await;
    let acks_b = directive_acks(&sink_b);
    assert_eq!(outcome_of(ack_for(&acks_b, "replenish_budget")), "applied");
    assert_eq!(
        cap_status(&loop_b, agent).await,
        (10, 5),
        "the replenish must actually credit 3 calls (2 + 3, under the ceiling)"
    );

    // Override: real state must change.
    tx_b.send(CuratorDirective::OverrideEnergyBudget {
        agent,
        new_budget: 5,
    })
    .expect("send override");
    loop_b.process_inbox().await;
    let acks_b = directive_acks(&sink_b);
    assert_eq!(
        outcome_of(ack_for(&acks_b, "override_energy_budget")),
        "applied"
    );
    assert_eq!(
        cap_status(&loop_b, agent).await,
        (5, 5),
        "the override must actually install ceiling 5"
    );

    // Clear override: original ceiling restored.
    tx_b.send(CuratorDirective::ClearOverride { agent })
        .expect("send clear");
    loop_b.process_inbox().await;
    let acks_b = directive_acks(&sink_b);
    assert_eq!(outcome_of(ack_for(&acks_b, "clear_override")), "applied");
    assert_eq!(
        cap_status(&loop_b, agent).await,
        (10, 10),
        "clearing the override must restore the original ceiling 10"
    );

    // Exactly one acknowledgment per directive on each loop.
    assert_eq!(directive_acks(&sink_a).len(), 5);
    assert_eq!(directive_acks(&sink_b).len(), 3);
}

/// Control: a dampened (repeated) directive produces no
/// acknowledgment — dampening is not application.
#[tokio::test]
async fn dampened_directive_is_not_acknowledged() {
    let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let (regulation_loop, tx) =
        loop_with_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>).await;

    for _ in 0..2 {
        tx.send(CuratorDirective::CalibrateThreshold {
            domain: "inference".to_string(),
            new_threshold: 42,
        })
        .expect("send directive");
    }
    regulation_loop.process_inbox().await;

    let acks = directive_acks(&sink);
    assert_eq!(
        acks.len(),
        1,
        "the repeated directive is dampened and must not be acknowledged: {acks:#?}"
    );
}

/// Control: an acknowledgment-persist failure is surfaced (warn) and
/// does not panic or wedge the inbox — but it never upgrades the
/// directive's outcome.
#[tokio::test]
async fn persist_failure_does_not_wedge_the_inbox() {
    let (regulation_loop, tx) =
        loop_with_sink(Arc::new(FailingSink) as Arc<dyn hkask_types::RegulationSink>).await;

    tx.send(CuratorDirective::CalibrateThreshold {
        domain: "inference".to_string(),
        new_threshold: 42,
    })
    .expect("send directive");
    regulation_loop.process_inbox().await;

    // The inbox drained (the send below would block otherwise is not
    // assertable on an unbounded channel, but processing completing
    // without panic is the contract).
    tx.send(CuratorDirective::CalibrateThreshold {
        domain: "storage".to_string(),
        new_threshold: 7,
    })
    .expect("send after failure");
    regulation_loop.process_inbox().await;
}

/// Escalation sink that records what it received and reports a scripted
/// `try_persist_alert` outcome — the escalation delivery seam under test.
struct ScriptedEscalationSink {
    received: Mutex<Vec<(String, f64, String)>>,
    result: Result<crate::AlertDeliveryOutcome, crate::AlertPersistError>,
}

impl ScriptedEscalationSink {
    fn confirmed(id: &str) -> Self {
        Self {
            received: Mutex::new(Vec::new()),
            result: Ok(crate::AlertDeliveryOutcome::Confirmed(Some(id.to_string()))),
        }
    }
}

#[async_trait::async_trait]
impl crate::AlertEscalationSink for ScriptedEscalationSink {
    async fn try_persist_alert(
        &self,
        output: &str,
        confidence: f64,
        error_context: &str,
    ) -> Result<crate::AlertDeliveryOutcome, crate::AlertPersistError> {
        self.received
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((output.to_string(), confidence, error_context.to_string()));
        self.result.clone()
    }
}

/// Escalation sink that records what it received and reports the
/// best-effort `Attempted` outcome — handed off, not confirmed.
struct BestEffortEscalationSink(Mutex<Vec<(String, f64, String)>>);

#[async_trait::async_trait]
impl crate::AlertEscalationSink for BestEffortEscalationSink {
    async fn try_persist_alert(
        &self,
        output: &str,
        confidence: f64,
        error_context: &str,
    ) -> Result<crate::AlertDeliveryOutcome, crate::AlertPersistError> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push((
            output.to_string(),
            confidence,
            error_context.to_string(),
        ));
        Ok(crate::AlertDeliveryOutcome::Attempted)
    }
}

fn escalate_directive() -> CuratorDirective {
    CuratorDirective::EscalateDomain {
        domain: "storage".to_string(),
        severity: EscalationSeverity::Warning,
        evidence: "variety deficit".to_string(),
    }
}

/// The escalation-delivery test preamble: a capturing event sink, a fresh
/// loop with `escalation_sink` wired, one escalate directive sent and
/// processed — returns the event sink and its directive acknowledgments.
/// The escalation sink is the caller's (the scripted outcome under test);
/// the outcome asserts stay per test.
async fn escalate_with_sink(
    escalation_sink: Arc<dyn crate::AlertEscalationSink>,
) -> (Arc<CapturingSink>, Vec<serde_json::Value>) {
    let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let (mut regulation_loop, tx) =
        loop_with_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>).await;
    regulation_loop.set_alert_escalation_sink(Some(escalation_sink));
    tx.send(escalate_directive()).expect("send escalation");
    regulation_loop.process_inbox().await;
    let acks = directive_acks(&sink);
    (sink, acks)
}

/// An undampened explicit escalation is delivered to the reviewable
/// queue retaining domain, severity, and evidence — and the
/// acknowledgment reports the confirmed queue outcome with the
/// escalation id. The queue payload must NOT fabricate measured
/// deficit/threshold fields: an explicit concern is not a sensor
/// reading.
#[tokio::test]
async fn explicit_escalation_confirmed_in_queue() {
    let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let (mut regulation_loop, tx) =
        loop_with_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>).await;
    let escalation_sink = Arc::new(ScriptedEscalationSink::confirmed("esc-42"));
    regulation_loop.set_alert_escalation_sink(Some(
        Arc::clone(&escalation_sink) as Arc<dyn crate::AlertEscalationSink>
    ));

    tx.send(escalate_directive()).expect("send escalation");
    regulation_loop.process_inbox().await;

    // The sink received the escalation with the concern's identity.
    let received = escalation_sink
        .received
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    assert_eq!(received.len(), 1, "exactly one queue delivery");
    let (output, confidence, error_context) = &received[0];
    assert!(
        output.contains("storage") && output.contains("warning"),
        "the queue output must name the domain and severity: {output}"
    );
    assert_eq!(*confidence, 0.5, "Warning maps to confidence 0.5");
    let context: serde_json::Value = serde_json::from_str(error_context).expect("context");
    assert_eq!(
        context.get("domain").and_then(|d| d.as_str()),
        Some("storage")
    );
    assert_eq!(
        context.get("severity").and_then(|s| s.as_str()),
        Some("warning")
    );
    assert_eq!(
        context.get("evidence").and_then(|e| e.as_str()),
        Some("variety deficit")
    );
    assert_eq!(
        context.get("explicit").and_then(|e| e.as_bool()),
        Some(true),
        "the board alert context must mark an explicit escalation"
    );
    assert!(
        context.get("deficit").is_none() && context.get("threshold").is_none(),
        "an explicit escalation must not fabricate measured deficit/threshold fields"
    );

    // The acknowledgment reports the confirmed outcome with the id and
    // retains the concern's identity.
    let acks = directive_acks(&sink);
    assert_eq!(acks.len(), 1);
    let ack = &acks[0];
    assert_eq!(outcome_of(ack), "confirmed");
    assert_eq!(ack.get("task_id").and_then(|i| i.as_str()), Some("esc-42"));
    assert_eq!(ack.get("domain").and_then(|d| d.as_str()), Some("storage"));
    assert_eq!(
        ack.get("severity").and_then(|s| s.as_str()),
        Some("warning")
    );
    assert_eq!(
        ack.get("evidence").and_then(|e| e.as_str()),
        Some("variety deficit")
    );
}

/// A repeat comment on an existing open card has no new id, but still
/// reports confirmed delivery to the board.
#[tokio::test]
async fn explicit_escalation_repeat_reports_confirmed_without_new_id() {
    let (_sink, acks) = escalate_with_sink(Arc::new(ScriptedEscalationSink {
        received: Mutex::new(Vec::new()),
        result: Ok(crate::AlertDeliveryOutcome::Confirmed(None)),
    }) as Arc<dyn crate::AlertEscalationSink>)
    .await;

    assert_eq!(acks.len(), 1);
    assert_eq!(outcome_of(&acks[0]), "confirmed");
    assert!(
        acks[0].get("task_id").is_none(),
        "a repeat comment has no new task id — the ack must not invent one"
    );
}

/// A missing escalation sink is surfaced as "missing_sink" — never
/// silently dropped and never claimed as delivered.
#[tokio::test]
async fn explicit_escalation_without_sink_reports_missing_sink() {
    let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
    let (regulation_loop, tx) =
        loop_with_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>).await;

    tx.send(escalate_directive()).expect("send escalation");
    regulation_loop.process_inbox().await;

    let acks = directive_acks(&sink);
    assert_eq!(acks.len(), 1);
    assert_eq!(
        outcome_of(&acks[0]),
        "missing_sink",
        "no escalation sink wired — the ack must surface that, not claim delivery"
    );
}

/// A failed board write is surfaced as attempted, never confirmed.
#[tokio::test]
async fn explicit_escalation_failed_write_reports_attempted() {
    let (_sink, acks) = escalate_with_sink(Arc::new(ScriptedEscalationSink {
        received: Mutex::new(Vec::new()),
        result: Err(crate::AlertPersistError::BoardWrite(
            "board unavailable".to_string(),
        )),
    }) as Arc<dyn crate::AlertEscalationSink>)
    .await;

    assert_eq!(acks.len(), 1);
    assert_eq!(
        outcome_of(&acks[0]),
        "attempted",
        "a failed write was tried — the ack must not claim it queued"
    );
}

/// A best-effort sink (the trait default — cannot report the
/// durable outcome) is "attempted", distinct from confirmed.
#[tokio::test]
async fn explicit_escalation_best_effort_sink_reports_attempted() {
    let (_sink, acks) =
        escalate_with_sink(Arc::new(BestEffortEscalationSink(Mutex::new(Vec::new())))
            as Arc<dyn crate::AlertEscalationSink>)
        .await;

    assert_eq!(acks.len(), 1);
    assert_eq!(outcome_of(&acks[0]), "attempted");
}
