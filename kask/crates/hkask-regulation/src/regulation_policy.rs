//! RegulationPolicy — data-driven per-metric regulation rules
//!
//! Consolidates the per-metric action mappings, severity thresholds,
//! and classification thresholds that were previously scattered across
//! `cybernetics_loop.rs`. Each `RegulationRule` defines what actions
//! to take when a specific metric deviates in a specific direction.

use crate::loops::{
    ActionDecision, ActionType, Deviation, DeviationDirection, LoopId, RegulationData, SignalMetric,
};

/// Identifies why a regulation action was proposed.
///
/// Replaces string matching in `build_regulation_action` — the compiler
/// now verifies that every policy-table entry has a corresponding dispatch
/// arm (or falls through to the generic `_` arm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegulationReason {
    EnergyBudgetLow,
    BudgetGuardEscalation,
    EnergyDepletionAutoAdjust,
    VarietyDeficitExceeded,
    ErrorRateExceeded,
    ConnectorLatencyExceeded,
    CommunicationBackpressure,
    ToolReliabilityDegraded,
    StorageUsageObserved,
    TripleCountObserved,
    LowConfidenceCountObserved,
    ConsolidationCandidatesObserved,
    PendingEscalationsObserved,
    AlgedonicEventsExceeded,
    /// The in-memory algedonic alert log is approaching its cap. The operator
    /// (or the `algedonic-review` skill) should review and clear reviewed
    /// entries before they are evicted unread.
    AlgedonicLogApproachingCap,
    GoalsStale,
    GoalsExpired,
    MetacognitionCriticalAlerts,
    MemoryLifeLow,
    CircuitBreakerOpen,
    InferenceUnavailable,
    ModelUnavailable,
    ContextServerFleetDegraded,
    OcrSilentFailuresExceeded,
}

impl RegulationReason {
    /// The wire-format string used in `RegulatoryActionParams` and logs.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EnergyBudgetLow => "energy_budget_low",
            Self::BudgetGuardEscalation => "budget_guard_escalation",
            Self::EnergyDepletionAutoAdjust => "energy_depletion_auto_adjust",
            Self::VarietyDeficitExceeded => "variety_deficit_exceeded",
            Self::ErrorRateExceeded => "error_rate_exceeded",
            Self::ConnectorLatencyExceeded => "connector_latency_exceeded",
            Self::CommunicationBackpressure => "communication_backpressure",
            Self::ToolReliabilityDegraded => "tool_reliability_degraded",
            Self::StorageUsageObserved => "storage_usage_observed",
            Self::TripleCountObserved => "triple_count_observed",
            Self::LowConfidenceCountObserved => "low_confidence_count_observed",
            Self::ConsolidationCandidatesObserved => "consolidation_candidates_observed",
            Self::PendingEscalationsObserved => "pending_escalations_observed",
            Self::AlgedonicEventsExceeded => "algedonic_events_exceeded",
            Self::AlgedonicLogApproachingCap => "algedonic_log_approaching_cap",
            Self::GoalsStale => "goals_stale",
            Self::GoalsExpired => "goals_expired",
            Self::MetacognitionCriticalAlerts => "metacognition_critical_alerts",
            Self::MemoryLifeLow => "memory_life_low",
            Self::CircuitBreakerOpen => "circuit_breaker_open",
            Self::InferenceUnavailable => "inference_unavailable",
            Self::ModelUnavailable => "model_unavailable",
            Self::ContextServerFleetDegraded => "context_server_fleet_degraded",
            Self::OcrSilentFailuresExceeded => "ocr_silent_failures_exceeded",
        }
    }
}

/// A proposed action before substitution and mode-specific filtering.
///
/// `target` and `action_type` are read by `build_regulation_action` to
/// construct the dispatched `RegulatoryAction`. `try_substitute` may
/// override `action_type` via the stagnation ladder, and mode-specific
/// filtering may skip the action entirely.
#[derive(Debug, Clone)]
pub(crate) struct ProposedAction {
    pub target: LoopId,
    pub action_type: ActionType,
    pub reason: RegulationReason,
}

/// A single regulation rule: when `metric` deviates in `direction`,
/// produce `proposed` actions with the given severity classification.
pub(crate) struct RegulationRule {
    pub metric: SignalMetric,
    pub direction: DeviationDirection,
    /// The proposed actions for this rule. A single rule can produce
    /// multiple proposed actions (e.g., EnergyRemaining triggers both
    /// Throttle and AdjustEnergyBudget).
    pub proposed: &'static [ProposedAction],
}

/// Consolidates all per-metric regulation rules.
///
/// Fuel source: declaration of what actions to propose when a metric
/// deviates. Runtime concerns (substitution ladders, throttle modes)
/// are handled by the caller in `compute()`.
pub(crate) struct RegulationPolicy {
    rules: Vec<RegulationRule>,
}

impl RegulationPolicy {
    /// Build the default regulation policy with all currently-supported rules.
    ///
    /// Covers all 31 `SignalMetric` variants per ADR-056 (Ashby's Law closure).
    /// Metrics are categorized by cybernetic role:
    /// - **Notify** (observational, no regulation needed)
    /// - **Escalate** (meta-regulatory, route to Curation)
    /// - **Domain-specific** (Calibrate/Throttle/CircuitBreak/Prune)
    pub fn default() -> Self {
        use ActionType::*;
        use DeviationDirection::*;
        use LoopId::*;
        // Explicit SignalMetric imports for the names that also exist as
        // RegulationReason variants — explicit imports shadow the glob and
        // resolve the ambiguity.
        use RegulationReason::*;
        use SignalMetric::{AlgedonicLogApproachingCap, MetacognitionCriticalAlerts, *};

        Self {
            rules: vec![
                // ── Energy (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: EnergyRemaining,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: Throttle,
                        reason: EnergyBudgetLow,
                    }],
                },
                RegulationRule {
                    metric: EnergyRemaining,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: BudgetGuardEscalation,
                    }],
                },
                RegulationRule {
                    metric: EnergyRemaining,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Cybernetics,
                        action_type: AdjustEnergyBudget,
                        reason: EnergyDepletionAutoAdjust,
                    }],
                },
                // ── Variety (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: VarietyDeficit,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: VarietyDeficitExceeded,
                    }],
                },
                // ── Error Rate (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: ErrorRate,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: CircuitBreak,
                        reason: ErrorRateExceeded,
                    }],
                },
                // ── Connector Latency (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: ConnectorLatency,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Cybernetics,
                        action_type: Throttle,
                        reason: ConnectorLatencyExceeded,
                    }],
                },
                // ── Communication Queue Depth (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: CommunicationQueueDepth,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Cybernetics,
                        action_type: Throttle,
                        reason: CommunicationBackpressure,
                    }],
                },
                // ── Wallet and Seam Coverage rules removed 2026-08-30 —
                // residuals of the deleted wallet module (219c74b180) and a
                // never-built seam watcher. No sensor ever emitted these
                // metrics; the rules could never fire.
                // ── Tool Reliability (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: ToolReliability,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: ToolReliabilityDegraded,
                    }],
                },
                // ── Category A: Observational metrics → Notify (no regulation needed) ──
                RegulationRule {
                    metric: StorageUsage,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: StorageUsageObserved,
                    }],
                },
                RegulationRule {
                    metric: TripleCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: TripleCountObserved,
                    }],
                },
                RegulationRule {
                    metric: LowConfidenceCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: LowConfidenceCountObserved,
                    }],
                },
                RegulationRule {
                    metric: ConsolidationCandidates,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: ConsolidationCandidatesObserved,
                    }],
                },
                RegulationRule {
                    metric: PendingEscalations,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: PendingEscalationsObserved,
                    }],
                },
                // ── Category B: Meta-regulatory metrics → Escalate to Curation ──
                RegulationRule {
                    metric: AlgedonicEvents,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: AlgedonicEventsExceeded,
                    }],
                },
                RegulationRule {
                    metric: AlgedonicLogApproachingCap,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: RegulationReason::AlgedonicLogApproachingCap,
                    }],
                },
                RegulationRule {
                    metric: GoalStaleCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: GoalsStale,
                    }],
                },
                RegulationRule {
                    metric: GoalExpiredCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: GoalsExpired,
                    }],
                },
                RegulationRule {
                    metric: MetacognitionCriticalAlerts,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: RegulationReason::MetacognitionCriticalAlerts,
                    }],
                },
                // MetacognitionVarietyDeficit / ActionIneffective /
                // RegulatoryPlateau / ActionDecisionBlocked rules removed
                // 2026-08-30 — superseded duplicates. MetacognitionVarietyDeficit
                // duplicated VarietyDeficit (same ledger overall_deficit,
                // same Escalate→Curation rule); the three action metrics were
                // superseded by the loop's direct escalation paths
                // (try_substitute; plateau/blocked alerts persisted to the
                // review queue and sensed as PendingEscalations).
                // ── Category C: Domain-specific regulation ──
                // MemoryLife (Memory Loop 2) → Calibrate
                RegulationRule {
                    metric: MemoryLife,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Memory,
                        action_type: Calibrate,
                        reason: MemoryLifeLow,
                    }],
                },
                // CircuitBreakerState (Inference Loop 1) → Throttle
                RegulationRule {
                    metric: CircuitBreakerState,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: Throttle,
                        reason: CircuitBreakerOpen,
                    }],
                },
                // InferenceAvailable (Inference Loop 1) → Throttle
                RegulationRule {
                    metric: InferenceAvailable,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: Throttle,
                        reason: InferenceUnavailable,
                    }],
                },
                // InferenceModelAvailable (Inference Loop 1) → Calibrate
                RegulationRule {
                    metric: InferenceModelAvailable,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: Calibrate,
                        reason: ModelUnavailable,
                    }],
                },
                // ContextServerHealth (Cybernetics Loop 6) → Escalate
                //
                // A degraded context-server fleet (servers stuck in Starting
                // or Error) is not something the loop can self-heal — it
                // indicates the foreground executor is starving the stdio
                // transport tasks, or a credential/config failure prevented
                // `initialize`. Escalate to Curation for operator attention.
                RegulationRule {
                    metric: ContextServerHealth,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: ContextServerFleetDegraded,
                    }],
                },
                // OcrSilentFailures (Cybernetics Loop 6) → Escalate
                //
                // A dead-but-responsive OCR endpoint (HTTP 200 with empty
                // content on every Complex page) is not something the loop
                // can self-heal — the corpus pipeline already degrades to
                // Tesseract and quarantines the endpoint via its circuit
                // breaker. Escalate to Curation for operator attention:
                // the endpoint needs fixing (prompt format, RAW_OPENAI_OUTPUT,
                // image encoding) or replacing.
                RegulationRule {
                    metric: OcrSilentFailures,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: OcrSilentFailuresExceeded,
                    }],
                },
            ],
        }
    }

    /// Find all proposed actions for a given deviation.
    ///
    /// Returns a flat list of `ProposedAction` references matching
    /// the deviation's `(metric, direction)`. The caller applies
    /// `try_substitute`, mode filtering, and data population.
    pub fn decide(&self, dev: &Deviation) -> Vec<&ProposedAction> {
        self.rules
            .iter()
            .filter(|r| r.metric == dev.signal.metric && r.direction == dev.direction)
            .flat_map(|r| r.proposed.iter())
            .collect()
    }
}

/// Extract (deficit, threshold) from a `RegulationData` variant.
/// Extract the metric value and threshold from a `RegulationData` variant.
///
/// Returns `Some((value, threshold))` for variants that carry a quantitative
/// value/threshold pair, `None` for `NoData` and variants without quantitative
/// data. The caller uses the `None` case to fall back to the action's reason
/// string for the alert message — avoiding the misleading "Variety deficit 0
/// exceeds threshold 0" message that the previous `(0, 0)` fallback produced
/// for non-variety alerts.
///
/// Fractional scalars are unit-scaled before the u64 conversion — a bare
/// `as u64` cast truncates (0.80 → 0), which produced the live-observed
/// "tool_reliability_degraded — value 0 exceeds threshold 0" escalation:
/// a positive 0.80 floor displayed as 0. Rates and ratios scale to whole
/// percent (0.80 → 80), latency seconds to whole milliseconds, and
/// count-valued scalars round to the nearest integer.
pub(crate) fn extract_deficit_threshold(data: &RegulationData) -> Option<(u64, u64)> {
    match data {
        RegulationData::VarietyDeficitExceeded { deficit, threshold } => {
            Some((rounded_count(*deficit), rounded_count(*threshold)))
        }
        RegulationData::ErrorRateExceeded {
            error_rate,
            threshold,
        } => Some((percent_of(*error_rate), percent_of(*threshold))),
        RegulationData::ConnectorLatencyExceeded {
            latency_secs,
            threshold,
        } => Some((millis_of(*latency_secs), millis_of(*threshold))),
        RegulationData::CommunicationBackpressure {
            queue_depth,
            threshold,
        } => Some((rounded_count(*queue_depth), rounded_count(*threshold))),
        RegulationData::ToolReliabilityDegraded {
            reliability,
            threshold,
        } => Some((percent_of(*reliability), percent_of(*threshold))),
        RegulationData::EnergyBudgetLow {
            remaining_ratio,
            set_point,
        } => Some((percent_of(*remaining_ratio), percent_of(*set_point))),
        RegulationData::BudgetGuardEscalation {
            remaining_ratio,
            set_point,
            ..
        } => Some((percent_of(*remaining_ratio), percent_of(*set_point))),
        RegulationData::EnergyDepletionAutoAdjust {
            remaining_ratio,
            set_point,
        } => Some((percent_of(*remaining_ratio), percent_of(*set_point))),
        RegulationData::ContextServerFleetHealth {
            healthy_count,
            total_count,
        } => Some((*total_count - *healthy_count, *total_count)),
        RegulationData::OcrSilentFailuresExceeded { count, threshold } => {
            Some((rounded_count(*count), rounded_count(*threshold)))
        }
        RegulationData::CuratorBudgetOverride { .. }
        | RegulationData::RolloutImpactCheck { .. }
        | RegulationData::NoData => None,
    }
}

/// Compose the alert message for a native-Escalate action from its typed
/// data and reason — the single source of truth for this format.
///
/// `route_action_as_alert` persists this exact string to the escalation
/// queue, and `verify_impact`'s `auto_resolve_cleared` reconstruction must
/// match it byte-for-byte to find the pending escalation — drift there
/// silently breaks stuck-loop auto-resolution. Both sites call this
/// helper so the identity is structural, not comment-enforced.
///
/// The verb follows the variant's bad direction (see
/// `RegulationData::below_threshold_is_bad`): floor metrics read
/// "fell below", ceiling metrics read "exceeds". Variants without a
/// threshold pair fall back to the advisory form.
pub(crate) fn alert_message(data: &RegulationData, reason: &str) -> String {
    match extract_deficit_threshold(data) {
        Some((deficit, threshold)) => {
            let verb = if data.below_threshold_is_bad() {
                "fell below"
            } else {
                "exceeds"
            };
            format!(
                "{} — value {} {} threshold {}",
                reason, deficit, verb, threshold
            )
        }
        None => format!("{} — regulatory escalation", reason),
    }
}

/// Extract the stable condition key from an alert message composed by
/// [`alert_message`].
///
/// `alert_message` embeds the per-cycle value ("{reason} — value {v} …" or
/// "{reason} — regulatory escalation"), so two messages for the same
/// persistently re-sensed condition differ every cycle and never
/// exact-match. Dedup, supersede, and auto-resolve must key on the
/// condition — the reason prefix before the " — " separator. Messages
/// without the separator are their own condition (exact match, the
/// previous behavior).
pub fn alert_condition(message: &str) -> &str {
    match message.find(" — ") {
        Some(idx) => &message[..idx],
        None => message,
    }
}

/// Scale a rate or ratio in [0.0, 1.0] to whole percent, rounding to
/// nearest.
///
/// A bare `as u64` cast truncates (0.80 → 0); percent scaling preserves
/// the set-point's magnitude so an alert never displays a threshold of 0
/// for a positive floor.
fn percent_of(value: f64) -> u64 {
    (value * 100.0).round() as u64
}

/// Scale a duration in seconds to whole milliseconds, rounding to nearest
/// so fractional-second set-points survive the u64 conversion.
fn millis_of(value: f64) -> u64 {
    (value * 1000.0).round() as u64
}

/// Round a count-valued f64 to the nearest integer.
fn rounded_count(value: f64) -> u64 {
    value.round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let data = RegulationData::ErrorRateExceeded {
            error_rate: 0.45,
            threshold: 0.30,
        };
        assert_eq!(extract_deficit_threshold(&data), Some((45, 30)));

        let data = RegulationData::EnergyBudgetLow {
            remaining_ratio: 0.15,
            set_point: 0.20,
        };
        assert_eq!(extract_deficit_threshold(&data), Some((15, 20)));

        let data = RegulationData::EnergyDepletionAutoAdjust {
            remaining_ratio: 0.12,
            set_point: 0.20,
        };
        assert_eq!(extract_deficit_threshold(&data), Some((12, 20)));

        // Latency scales to milliseconds so fractional seconds survive.
        let data = RegulationData::ConnectorLatencyExceeded {
            latency_secs: 2.5,
            threshold: 30.0,
        };
        assert_eq!(extract_deficit_threshold(&data), Some((2500, 30000)));

        // Count-valued scalars round to the nearest integer.
        let data = RegulationData::CommunicationBackpressure {
            queue_depth: 12.7,
            threshold: 10.0,
        };
        assert_eq!(extract_deficit_threshold(&data), Some((13, 10)));

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
        assert_eq!(
            extract_deficit_threshold(&RegulationData::CuratorBudgetOverride {
                agent: "curator".into(),
                new_budget: 1000,
            }),
            None
        );
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

        let data = RegulationData::EnergyBudgetLow {
            remaining_ratio: 0.15,
            set_point: 0.20,
        };
        assert_eq!(
            alert_message(&data, "energy_budget_low"),
            "energy_budget_low — value 15 fell below threshold 20"
        );

        // Ceiling metrics: above-threshold is the bad direction — the
        // verb stays "exceeds".
        let data = RegulationData::ErrorRateExceeded {
            error_rate: 0.45,
            threshold: 0.30,
        };
        assert_eq!(
            alert_message(&data, "error_rate_exceeded"),
            "error_rate_exceeded — value 45 exceeds threshold 30"
        );

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
}

/// Classify an action's impact decision using Fermi's three-tier gate.
///
/// - `worsening`: absolute value of the negative delta (0.0 if improved).
/// - `stage_ratio`: below this → Accept (noise).
/// - `block_ratio`: at or above this → Block (hard reject).
/// - Between → Stage (escalate for review).
pub(crate) fn classify_decision(
    worsening: f64,
    stage_ratio: f64,
    block_ratio: f64,
) -> ActionDecision {
    debug_assert!(
        stage_ratio <= block_ratio,
        "stage_worsening_ratio ({stage_ratio}) must be <= block_worsening_ratio ({block_ratio})"
    );
    if worsening >= block_ratio {
        ActionDecision::Block
    } else if worsening < stage_ratio {
        ActionDecision::Accept
    } else {
        ActionDecision::Stage
    }
}

/// Return the default substitution ladder for a metric.
///
/// These are the built-in ladders used when no custom ladders are configured
/// via `SetPoints.action_substitutions`. Each ladder is an ordered list of
/// action types to try when the primary action is repeatedly ineffective.
pub(crate) fn default_substitution_ladder(metric: SignalMetric) -> &'static [ActionType] {
    use ActionType::*;
    match metric {
        // ── Energy ──
        SignalMetric::EnergyRemaining => &[Throttle, AdjustEnergyBudget, Escalate],
        // ── Variety ──
        SignalMetric::VarietyDeficit => &[Escalate, Calibrate, OverrideEnergyBudget],
        // ── Error Rate ──
        SignalMetric::ErrorRate => &[CircuitBreak, Calibrate, Escalate],
        SignalMetric::CircuitBreakerState => &[CircuitBreak, Calibrate, Escalate],
        // ── Latency / Backpressure ──
        SignalMetric::ConnectorLatency => &[Throttle, Calibrate, Escalate],
        SignalMetric::CommunicationQueueDepth => &[Throttle, Escalate],
        // ── Meta-regulatory (only Curation can break the plateau) ──
        SignalMetric::AlgedonicEvents => &[Escalate, Calibrate],
        SignalMetric::AlgedonicLogApproachingCap => &[Escalate, Calibrate],
        SignalMetric::GoalStaleCount => &[Escalate, Calibrate],
        SignalMetric::GoalExpiredCount => &[Escalate, Calibrate],
        SignalMetric::MetacognitionCriticalAlerts => &[Escalate, Calibrate, OverrideEnergyBudget],
        // ── Domain-specific ──
        SignalMetric::MemoryLife => &[Calibrate, Escalate],
        SignalMetric::InferenceAvailable => &[Throttle, Calibrate, Escalate],
        SignalMetric::InferenceModelAvailable => &[Calibrate, Escalate],
        SignalMetric::ContextServerHealth => &[Escalate, Calibrate],
        SignalMetric::OcrSilentFailures => &[Escalate, Calibrate],
        // ── Observational (no substitution — Notify is terminal) ──
        SignalMetric::StorageUsage
        | SignalMetric::TripleCount
        | SignalMetric::LowConfidenceCount
        | SignalMetric::ConsolidationCandidates
        | SignalMetric::PendingEscalations
        | SignalMetric::ToolReliability
        | SignalMetric::TestCoverage
        | SignalMetric::MutationScore => &[],
    }
}
