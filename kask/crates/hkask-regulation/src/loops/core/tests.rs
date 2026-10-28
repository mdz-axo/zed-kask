use super::super::actions::RegulatoryActionParams;
use super::super::signals::Signal;
use super::*;

/// Pins F1 + F2 + F3: when no deviations or measurements exist
/// (the healthy steady-state), response coverage and fidelity are 1.0,
/// while rollout progress remains unknown.
/// Unknown progress is represented as `None`, not a numeric zero or one,
/// so unmeasured and measured-stagnant cycles remain distinguishable.
#[test]
fn from_cycle_healthy_reports_trivially_correct_metrics() {
    let metrics = LoopMetrics::from_cycle(
        0,
        &[], // no deviations — healthy
        &[], // no actions
        &[], // no impact reports — unverified
        TriggerOrigin::Scheduled,
    );
    assert_eq!(
        metrics.response_coverage, 1.0,
        "response coverage is 1.0 when healthy"
    );
    assert_eq!(
        metrics.fidelity_score, 1.0,
        "fidelity=1.0 when healthy (trivially matched)"
    );
    assert_eq!(metrics.rollout_progress_score, None);
}

/// Response coverage counts handled deviations and remains bounded.
#[test]
fn from_cycle_response_coverage_is_dispositions_over_deviations() {
    let signal_a = Signal::new(LoopId::Cybernetics, SignalMetric::ToolReliability, 0.1, 0.2);
    let signal_b = Signal::new(
        LoopId::Cybernetics,
        SignalMetric::VarietyDeficit,
        200.0,
        100.0,
    );
    let deviations = [
        Deviation::from_signal(&signal_a).unwrap(),
        Deviation::from_signal(&signal_b).unwrap(),
    ];
    let action = RegulatoryAction::with_metric(
        LoopId::Curation,
        ActionType::Escalate,
        RegulatoryActionParams::reason("tool_reliability_degraded"),
        "tool_reliability".into(),
    );
    let metrics = LoopMetrics::from_cycle(0, &deviations, &[action], &[], TriggerOrigin::Scheduled);
    assert_eq!(
        metrics.response_coverage, 0.5,
        "1 disposition / 2 deviations = 0.5"
    );
    assert_eq!(
        metrics.fidelity_score, 0.5,
        "1 matched / 2 deviations = 0.5"
    );
    assert_eq!(
        metrics.rollout_progress_score, None,
        "no impact reports remain visibly unverified"
    );
}

/// Observed progress requires improvement, not just acceptance.
#[test]
fn from_cycle_progress_is_improved_over_verified() {
    let report_accept = ImpactReport::new(
        ActionType::Notify,
        SignalMetric::ToolReliability,
        0.1,
        0.3, // improved (delta > 0 for the upward-good floor)
        ActionDecision::Accept,
    );
    let report_block = ImpactReport::new(
        ActionType::Escalate,
        SignalMetric::VarietyDeficit,
        0.3,
        0.5, // worsened (delta > 0 for the downward-good metric)
        ActionDecision::Block,
    );
    let metrics = LoopMetrics::from_cycle(
        0,
        &[],
        &[],
        &[report_accept, report_block],
        TriggerOrigin::Scheduled,
    );
    assert_eq!(
        metrics.rollout_progress_score,
        Some(0.5),
        "1 improved / 2 verified = 0.5"
    );
    // Response coverage and fidelity are 1.0 because no deviations.
    assert_eq!(metrics.response_coverage, 1.0);
    assert_eq!(metrics.fidelity_score, 1.0);
}

// ── TriggerOrigin::Prompted tests (F1) ─────────────────────────────────

/// Pin F1: `TriggerOrigin::Prompted` is distinct from `Manual`. The
/// `Prompted` variant represents a model-initiated-but-human-gated stage
/// (e.g. `algedonic-review` step 4), while `Manual` is operator-initiated.
#[test]
fn triggered_origin_prompted_is_distinct_from_manual() {
    assert_ne!(TriggerOrigin::Prompted, TriggerOrigin::Manual);
    assert_ne!(TriggerOrigin::Prompted, TriggerOrigin::Scheduled);
    assert_ne!(TriggerOrigin::Prompted, TriggerOrigin::AlertDriven);
    assert_ne!(TriggerOrigin::Prompted, TriggerOrigin::EventDriven);
}

/// Pin F1: `LoopMetrics::from_cycle` accepts `Prompted` as a trigger and
/// records it. The trigger is carried through to the `LoopMetrics.trigger`
/// field so downstream consumers can distinguish `Prompted` cycles from
/// `Manual` cycles.
#[test]
fn prompted_triggers_tracked_separately_from_manual() {
    let metrics_prompted = LoopMetrics::from_cycle(0, &[], &[], &[], TriggerOrigin::Prompted);
    assert_eq!(metrics_prompted.trigger, TriggerOrigin::Prompted);

    let metrics_manual = LoopMetrics::from_cycle(0, &[], &[], &[], TriggerOrigin::Manual);
    assert_eq!(metrics_manual.trigger, TriggerOrigin::Manual);
    assert_ne!(
        metrics_prompted.trigger, metrics_manual.trigger,
        "Prompted and Manual must be distinguishable in LoopMetrics"
    );
}

// ── LoopView tests (F2 + F5) ───────────────────────────────────────────

/// Pin F5: `SenseReading` has four distinct variants. Empty is never
/// blank — idle, fault, and unknown are distinct states, not a single
/// "no data" bucket.
#[test]
fn sense_reading_distinguishes_four_states() {
    assert_ne!(SenseReading::Empty, SenseReading::Idle);
    assert_ne!(SenseReading::Empty, SenseReading::Fault);
    assert_ne!(SenseReading::Empty, SenseReading::Unknown);
    assert_ne!(SenseReading::Idle, SenseReading::Fault);
    assert_ne!(SenseReading::Idle, SenseReading::Unknown);
    assert_ne!(SenseReading::Fault, SenseReading::Unknown);
}

/// Pin F2: `LoopView` composes four modules into a `Reading`. The
/// composition logic must distinguish wiring-closed (never ran) from
/// turning (running and verified) from broken (running but failing)
/// from unobserved (can't tell).
#[test]
fn loop_view_composes_four_modules_into_reading() {
    // NeverRun -> WiringClosed (dominant failure mode: wired but never ticked)
    let view = LoopView::new(
        LoopModel::Producing,
        SenseReading::Empty,
        OutcomeTrust::Trusted,
        LivenessTrust::NeverRun,
    );
    assert_eq!(
        view.reading,
        Reading::WiringClosed,
        "NeverRun -> WiringClosed regardless of other modules"
    );

    // Live + Fault -> Broken (sensor is broken, loop is turning but can't sense)
    let view = LoopView::new(
        LoopModel::Producing,
        SenseReading::Fault,
        OutcomeTrust::Trusted,
        LivenessTrust::Live,
    );
    assert_eq!(
        view.reading,
        Reading::Broken,
        "Fault sensor -> Broken (turning but can't sense)"
    );

    // Live + Unknown -> Unobserved (can't read the sensor)
    let view = LoopView::new(
        LoopModel::Producing,
        SenseReading::Unknown,
        OutcomeTrust::Trusted,
        LivenessTrust::Live,
    );
    assert_eq!(
        view.reading,
        Reading::Unobserved,
        "Unknown sensor -> Unobserved"
    );

    // Live + Empty + NotProducing -> WiringClosed (chain not producing)
    let view = LoopView::new(
        LoopModel::NotProducing,
        SenseReading::Empty,
        OutcomeTrust::Unverified,
        LivenessTrust::Live,
    );
    assert_eq!(
        view.reading,
        Reading::WiringClosed,
        "NotProducing -> WiringClosed (wired but not turning)"
    );

    // Live + Empty + Producing + Untrusted -> Broken (actions making things worse)
    let view = LoopView::new(
        LoopModel::Producing,
        SenseReading::Empty,
        OutcomeTrust::Untrusted,
        LivenessTrust::Live,
    );
    assert_eq!(
        view.reading,
        Reading::Broken,
        "Untrusted -> Broken (turning but not working)"
    );

    // Live + Empty + Producing + Trusted -> Turning (working!)
    let view = LoopView::new(
        LoopModel::Producing,
        SenseReading::Empty,
        OutcomeTrust::Trusted,
        LivenessTrust::Live,
    );
    assert_eq!(
        view.reading,
        Reading::Turning,
        "Trusted + Producing + Live -> Turning"
    );

    // Live + Empty + Stalled + Unverified -> Unobserved (can't tell)
    let view = LoopView::new(
        LoopModel::Stalled,
        SenseReading::Empty,
        OutcomeTrust::Unverified,
        LivenessTrust::Live,
    );
    assert_eq!(
        view.reading,
        Reading::Unobserved,
        "Stalled + Unverified -> Unobserved (not enough data)"
    );

    // Stale + Empty + Producing + Trusted -> Turning (was running, still verified)
    let view = LoopView::new(
        LoopModel::Producing,
        SenseReading::Empty,
        OutcomeTrust::Trusted,
        LivenessTrust::Stale,
    );
    assert_eq!(
        view.reading,
        Reading::Turning,
        "Stale + Trusted + Producing -> Turning (stale but still verified)"
    );

    // Live + Idle + Producing + Unverified -> Unobserved (sensor hasn't ticked)
    let view = LoopView::new(
        LoopModel::Producing,
        SenseReading::Idle,
        OutcomeTrust::Unverified,
        LivenessTrust::Live,
    );
    assert_eq!(
        view.reading,
        Reading::Unobserved,
        "Idle sensor + Unverified -> Unobserved"
    );
}

/// Pin F2: `Reading::Display` produces a human-readable string for each
/// variant. The Fermi `LoopView` produces "a Reading and a sentence."
#[test]
fn reading_display_provides_human_readable_string() {
    assert_eq!(Reading::Turning.to_string(), "turning");
    assert_eq!(Reading::WiringClosed.to_string(), "wiring-closed");
    assert_eq!(Reading::Broken.to_string(), "broken");
    assert_eq!(Reading::Unobserved.to_string(), "unobserved");
}

// ── StageActions tests (F4) ─────────────────────────────────────────────

/// Pin F4: `StageActions` registry maps `Prompted` stages to MCP tool
/// names. The registry is the declared surface — without it, a new
/// `Prompted` stage has no enforced door.
#[test]
fn stage_actions_registry_maps_prompted_stages_to_mcp_tools() {
    let mut actions = StageActions::new();
    assert!(actions.is_empty());

    // Register the algedonic-review ACT stage as a Prompted door.
    actions.register(
        TriggerOrigin::Prompted,
        "algedonic_review_act",
        vec!["kanban_task_verify".to_string()],
    );

    assert_eq!(actions.len(), 1);
    assert!(!actions.is_empty());

    // Look up the doors for the Prompted stage.
    let doors = actions.doors(TriggerOrigin::Prompted, "algedonic_review_act");
    assert_eq!(doors, ["kanban_task_verify"]);

    // A Manual trigger for the same stage name returns no doors —
    // Prompted and Manual are distinct trigger types.
    let manual_doors = actions.doors(TriggerOrigin::Manual, "algedonic_review_act");
    assert_eq!(
        manual_doors.len(),
        0,
        "Manual trigger has no doors for a Prompted stage"
    );

    // all_doors returns the full registry.
    let all = actions.all_doors();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].0, TriggerOrigin::Prompted);
    assert_eq!(all[0].1, "algedonic_review_act");
    assert_eq!(all[0].2, ["kanban_task_verify"]);
}
