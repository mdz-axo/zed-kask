use super::*;

/// ADR-056 Ashby closure, made checkable: every `SignalMetric` variant
/// either has a policy rule or is allowlisted with its no-producer
/// reason. The or-pattern below is exhaustive (no `_` arm) — adding a
/// variant breaks the build until this test carries it. D87 added
/// `ExperimentationStuckExperiments` with a sensor but no rule: the deviation
/// was sensed and then silently dropped by `decide` — the exact gap
/// this test now pins shut.
#[test]
fn every_signal_metric_has_a_rule_or_documented_allowlist_entry() {
    use crate::loops::SignalMetric;
    let all = {
        use SignalMetric::*;
        // Exhaustive or-pattern (no `_` arm): a new variant fails this
        // match's exhaustiveness check until the list grows with it.
        match VarietyDeficit {
            VarietyDeficit
            | MemoryLife
            | TripleCount
            | LowConfidenceCount
            | CircuitBreakerState
            | InferenceModelAvailable
            | ContextServerHealth
            | OcrSilentFailures
            | AlgedonicEvents
            | AlgedonicLogApproachingCap
            | PendingEscalations
            | ConsolidationCandidates
            | GoalStaleCount
            | GoalExpiredCount
            | MetacognitionCriticalAlerts
            | ExperimentationStuckExperiments
            | NativeFirstRate
            | ToolReliability
            | PassRate
            | TestCoverage
            | MutationScore => vec![
                VarietyDeficit,
                MemoryLife,
                TripleCount,
                LowConfidenceCount,
                CircuitBreakerState,
                InferenceModelAvailable,
                ContextServerHealth,
                OcrSilentFailures,
                AlgedonicEvents,
                AlgedonicLogApproachingCap,
                PendingEscalations,
                ConsolidationCandidates,
                GoalStaleCount,
                GoalExpiredCount,
                MetacognitionCriticalAlerts,
                ExperimentationStuckExperiments,
                NativeFirstRate,
                ToolReliability,
                PassRate,
                TestCoverage,
                MutationScore,
            ],
        }
    };
    // No production sensor emits these (verified 2026-10-01: zero
    // production `Signal::new` sites): the event-driven impact-check
    // channel (PassRate — consumed by verify_impact, not the policy
    // decide path) and decode-only persisted-history metrics
    // (TestCoverage, MutationScore). The legacy Loop-6 vocabulary
    // (EnergyRemaining, ErrorRate, ConnectorLatency,
    // CommunicationQueueDepth) was deleted 2026-10-01 — zero
    // production emission sites, and the string-resolution paths
    // degrade to a visible warn-and-skip for any stale submitted
    // name.
    let allowlisted = [
        SignalMetric::PassRate,
        SignalMetric::TestCoverage,
        SignalMetric::MutationScore,
    ];
    let policy = RegulationPolicy::default();
    for metric in all {
        if allowlisted.contains(&metric) {
            continue;
        }
        let covered = policy.rules.iter().any(|rule| rule.metric == metric);
        assert!(
            covered,
            "{metric:?} has a producer but no policy rule — a deviation it emits \
             would be sensed and then silently dropped by decide()"
        );
    }
}

#[test]
fn block_severity_reserves_critical_for_large_worsening() {
    use crate::algedonic::AlertSeverity;
    assert_eq!(block_severity(0.30, 0.20), AlertSeverity::Warning);
    assert_eq!(block_severity(0.40, 0.20), AlertSeverity::Warning);
    assert_eq!(block_severity(0.625, 0.20), AlertSeverity::Critical);
}

/// Pins the fractional-set-point fix in `extract_deficit_threshold`.
///
/// The previous `*value as u64` casts truncated every fractional
/// scalar: a `ToolReliabilityDegraded { reliability: 0.0, threshold: 0.80 }`
/// extracted as `(0, 0)`, producing the live-observed
/// "tool_reliability_degraded — value 0 exceeds threshold 0" escalation —
/// a threshold of 0 the loop could never meaningfully breach, and
/// indistinguishable from a broken sense input returning zero (the
/// `.rules` `unwrap_or(0)` trap). The extraction must preserve the
/// set-point's magnitude.
#[test]
fn extract_deficit_threshold_preserves_fractional_magnitude() {
    // Rates/ratios scale to whole percent (0.80 → 80).
    let data = RegulationData::ToolReliabilityDegraded {
        reliability: 0.0,
        threshold: 0.80,
    };
    assert_eq!(extract_deficit_threshold(&data), Some((0, 80)));

    // Integer-valued data is unchanged by the scaling.
    let data = RegulationData::VarietyDeficitExceeded {
        deficit: 100.0,
        threshold: 19.0,
    };
    assert_eq!(extract_deficit_threshold(&data), Some((100, 19)));
}

/// A fractional set-point below 0.5 must not display as a threshold
/// of 0 — that is the exact `(0, 0)` pair the operator cannot
/// distinguish from a broken sensor.
#[test]
fn extract_never_yields_zero_threshold_for_positive_set_point() {
    let data = RegulationData::ToolReliabilityDegraded {
        reliability: 0.0,
        threshold: 0.30,
    };
    assert_eq!(extract_deficit_threshold(&data), Some((0, 30)));
}

/// Variants without a quantitative pair return `None` — the caller
/// falls back to the reason string with the `(1, 1)` advisory sentinel,
/// never a fabricated `(0, 0)`.
#[test]
fn extract_returns_none_for_non_threshold_variants() {
    assert_eq!(extract_deficit_threshold(&RegulationData::NoData), None);
}

/// Pins the direction-aware verb in `alert_message`: floor metrics
/// (the deviation is the value falling below the threshold) must read
/// "fell below" — the previous shared "exceeds" verb lied for them,
/// reading a reliability of 0 against a 0.80 floor as
/// "value 0 exceeds threshold 80".
/// `alert_condition` extracts the stable reason prefix that dedup,
/// supersede, and auto-resolve key on — the per-cycle value after the
/// separator must not participate in matching. Two messages for the
/// same condition sensed in different cycles must yield the same key.
#[test]
fn alert_condition_strips_per_cycle_value() {
    assert_eq!(
        alert_condition("variety_deficit_exceeded — value 2149 exceeds threshold 20"),
        "variety_deficit_exceeded"
    );
    assert_eq!(
        alert_condition("variety_deficit_exceeded — value 53 exceeds threshold 20"),
        "variety_deficit_exceeded"
    );
    // Advisory form: the reason is still the condition.
    assert_eq!(
        alert_condition("algedonic_events_exceeded — regulatory escalation"),
        "algedonic_events_exceeded"
    );
    // No separator: exact-match behavior is preserved.
    assert_eq!(
        alert_condition("Variety deficit 150 exceeds threshold 100"),
        "Variety deficit 150 exceeds threshold 100"
    );
}

#[test]
fn alert_message_verb_follows_metric_direction() {
    // Floor metrics: below-threshold is the bad direction.
    let data = RegulationData::ToolReliabilityDegraded {
        reliability: 0.0,
        threshold: 0.80,
    };
    assert_eq!(
        alert_message(&data, "tool_reliability_degraded"),
        "tool_reliability_degraded — value 0 fell below threshold 80"
    );

    // Ceiling metrics retain the "exceeds" verb.
    let data = RegulationData::VarietyDeficitExceeded {
        deficit: 100.0,
        threshold: 19.0,
    };
    assert_eq!(
        alert_message(&data, "variety_deficit_exceeded"),
        "variety_deficit_exceeded — value 100 exceeds threshold 19"
    );

    // No threshold pair — the advisory fallback carries no verb.
    assert_eq!(
        alert_message(&RegulationData::NoData, "some_reason"),
        "some_reason — regulatory escalation"
    );
}
