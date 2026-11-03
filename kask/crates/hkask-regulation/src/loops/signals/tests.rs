use super::*;

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

/// The healthy side of a higher-is-worse ratio is not a deviation: a
/// native-first rate BELOW its threshold is the compliant reading, never a
/// BelowSetPoint "deviation" that no policy rule can disposition (the
/// chronic `deviations: 1, advisories_computed: 0, coverage 0.0`
/// signature, observed live for weeks through 2026-11-02).
#[test]
fn native_first_rate_below_set_point_is_healthy() {
    let signal = Signal::new(
        LoopId::Cybernetics,
        SignalMetric::NativeFirstRate,
        0.0,
        0.65,
    );
    assert!(
        Deviation::from_signal(&signal).is_none(),
        "rate below threshold is the healthy side of a higher-is-worse ratio"
    );
}

/// The count family is healthy below a configured tolerance too
/// (`algedonic_events_max` is operator-configurable): a count under the
/// tolerance is not a deviation in either direction.
#[test]
fn count_under_configured_tolerance_is_healthy() {
    let signal = Signal::new(LoopId::Cybernetics, SignalMetric::AlgedonicEvents, 1.0, 3.0);
    assert!(
        Deviation::from_signal(&signal).is_none(),
        "count under the configured tolerance is healthy, not a BelowSetPoint deviation"
    );
}

/// Above the threshold the ratio still deviates — the fix must not
/// suppress the actionable side.
#[test]
fn native_first_rate_above_set_point_still_deviates() {
    let signal = Signal::new(
        LoopId::Cybernetics,
        SignalMetric::NativeFirstRate,
        1.0,
        0.65,
    );
    let deviation =
        Deviation::from_signal(&signal).expect("a rate above the threshold still deviates");
    assert_eq!(deviation.direction, DeviationDirection::AboveSetPoint);
}
