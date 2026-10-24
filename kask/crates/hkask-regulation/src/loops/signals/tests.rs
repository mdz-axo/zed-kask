use super::*;

/// Pins the per-metric impact direction: fleet health, tool
/// reliability, and pass rate improve upward; variety deficit
/// improves downward; everything else has no verified impact path
/// (`verify_impact` falls back to any-nonzero-delta).
#[test]
fn impact_direction_covers_the_verifiable_metrics() {
    assert_eq!(
        SignalMetric::ContextServerHealth.impact_direction(),
        Some(true)
    );
    assert_eq!(SignalMetric::ToolReliability.impact_direction(), Some(true));
    assert_eq!(SignalMetric::PassRate.impact_direction(), Some(true));
    assert_eq!(SignalMetric::VarietyDeficit.impact_direction(), Some(false));
    assert_eq!(
        SignalMetric::OcrSilentFailures.impact_direction(),
        Some(false)
    );
    assert_eq!(
        SignalMetric::ExperimentationStuckExperiments.impact_direction(),
        Some(false)
    );
    assert_eq!(
        SignalMetric::NativeFirstRate.impact_direction(),
        Some(false),
        "an increase in native-first turns is a degradation, not an improvement"
    );
    assert_eq!(SignalMetric::TestCoverage.impact_direction(), None);
}

/// Pins the strict boundary semantics: a value exactly AT the
/// set-point is not a deviation. This is the boundary check behind
/// `tool_reliability_degraded` and friends — non-strict (`>=`/`<=`)
/// semantics here would fire an alert when value and threshold are
/// equal, the "0 exceeds 0" false-positive class.
#[test]
fn from_signal_returns_none_at_set_point_equality() {
    let signal = Signal::new(
        LoopId::Cybernetics,
        SignalMetric::ToolReliability,
        0.80,
        0.80,
    );
    assert!(
        Deviation::from_signal(&signal).is_none(),
        "value == set-point is the homeostatic state, not a deviation"
    );
}

#[test]
fn metric_name_round_trips() {
    // Every variant must survive as_str -> from_str_name. A variant
    // missing from the parse table would silently fall to the caller's
    // fallback and mislabel impact reports.
    let names = [
        "variety_deficit",
        "memory_life",
        "triple_count",
        "low_confidence_count",
        "circuit_breaker_state",
        "inference_model_available",
        "context_server_health",
        "ocr_silent_failures",
        "algedonic_events",
        "algedonic_log_approaching_cap",
        "pending_escalations",
        "consolidation_candidates",
        "goal_stale_count",
        "goal_expired_count",
        "metacognition_critical_alerts",
        "experimentation_stuck_experiments",
        "tool_reliability",
        "pass_rate",
        "test_coverage",
        "mutation_score",
    ];
    for name in names {
        let parsed =
            SignalMetric::from_str_name(name).unwrap_or_else(|| panic!("{name} must parse"));
        assert_eq!(parsed.as_str(), name);
    }
    // Unknown names are None, not a silent default.
    assert!(SignalMetric::from_str_name("not_a_metric").is_none());
}
