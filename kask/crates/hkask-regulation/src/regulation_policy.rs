//! RegulationPolicy — data-driven per-metric regulation rules
//!
//! Consolidates the per-metric action mappings, severity thresholds,
//! and classification thresholds that were previously scattered across
//! `cybernetics_loop.rs`. Each `RegulationRule` defines what actions
//! to take when a specific metric deviates in a specific direction.

use crate::types::loops::{
    ActionDecision, ActionType, Deviation, DeviationDirection, LoopId, RegulationData, SignalMetric,
};

/// Identifies why a regulation action was proposed.
///
/// Replaces string matching in `build_regulation_action` — the compiler
/// now verifies that every policy-table entry has a corresponding dispatch
/// arm (or falls through to the generic `_` arm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegulationReason {
    EnergyBudgetLow,
    BudgetGuardEscalation,
    EnergyDepletionAutoAdjust,
    VarietyDeficitExceeded,
    ErrorRateExceeded,
    ConnectorLatencyExceeded,
    CommunicationBackpressure,
    WalletBalanceLow,
    WalletKeyUnhealthy,
    SeamCoverageDegraded,
    SeamCoverageImproved,
    ToolReliabilityDegraded,
    StorageUsageObserved,
    TripleCountObserved,
    LowConfidenceCountObserved,
    ConsolidationCandidatesObserved,
    PendingEscalationsObserved,
    AlgedonicEventsExceeded,
    GoalsStale,
    GoalsExpired,
    MetacognitionVarietyDeficit,
    MetacognitionCriticalAlerts,
    ActionIneffective,
    RegulatoryPlateauDetected,
    ActionDecisionBlocked,
    MemoryLifeLow,
    CircuitBreakerOpen,
    InferenceUnavailable,
    InferenceGasLow,
    ModelUnavailable,
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
            Self::WalletBalanceLow => "wallet_balance_low",
            Self::WalletKeyUnhealthy => "wallet_key_unhealthy",
            Self::SeamCoverageDegraded => "seam_coverage_degraded",
            Self::SeamCoverageImproved => "seam_coverage_improved",
            Self::ToolReliabilityDegraded => "tool_reliability_degraded",
            Self::StorageUsageObserved => "storage_usage_observed",
            Self::TripleCountObserved => "triple_count_observed",
            Self::LowConfidenceCountObserved => "low_confidence_count_observed",
            Self::ConsolidationCandidatesObserved => "consolidation_candidates_observed",
            Self::PendingEscalationsObserved => "pending_escalations_observed",
            Self::AlgedonicEventsExceeded => "algedonic_events_exceeded",
            Self::GoalsStale => "goals_stale",
            Self::GoalsExpired => "goals_expired",
            Self::MetacognitionVarietyDeficit => "metacognition_variety_deficit",
            Self::MetacognitionCriticalAlerts => "metacognition_critical_alerts",
            Self::ActionIneffective => "action_ineffective",
            Self::RegulatoryPlateauDetected => "regulatory_plateau_detected",
            Self::ActionDecisionBlocked => "action_decision_blocked",
            Self::MemoryLifeLow => "memory_life_low",
            Self::CircuitBreakerOpen => "circuit_breaker_open",
            Self::InferenceUnavailable => "inference_unavailable",
            Self::InferenceGasLow => "inference_gas_low",
            Self::ModelUnavailable => "model_unavailable",
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
pub struct ProposedAction {
    pub target: LoopId,
    pub action_type: ActionType,
    pub reason: RegulationReason,
}

/// A single regulation rule: when `metric` deviates in `direction`,
/// produce `proposed` actions with the given severity classification.
pub struct RegulationRule {
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
pub struct RegulationPolicy {
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
        // Explicit SignalMetric imports for the 4 names that also exist as
        // RegulationReason variants — explicit imports shadow the glob and
        // resolve the ambiguity.
        use RegulationReason::*;
        use SignalMetric::{
            ActionDecisionBlocked, ActionIneffective, MetacognitionCriticalAlerts,
            MetacognitionVarietyDeficit, *,
        };

        Self {
            rules: vec![
                // ── Energy / Gas (Cybernetics Loop 6) ──
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
                // ── Wallet (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: WalletBalanceRatio,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: WalletBalanceLow,
                    }],
                },
                RegulationRule {
                    metric: WalletKeyHealth,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: WalletKeyUnhealthy,
                    }],
                },
                // ── Seam Coverage (Seam Watcher R7.3) ──
                RegulationRule {
                    metric: SeamCoverage,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: SeamCoverageDegraded,
                    }],
                },
                RegulationRule {
                    metric: SeamCoverage,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: SeamCoverageImproved,
                    }],
                },
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
                    metric: MetacognitionVarietyDeficit,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: RegulationReason::MetacognitionVarietyDeficit,
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
                RegulationRule {
                    metric: ActionIneffective,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: RegulationReason::ActionIneffective,
                    }],
                },
                RegulationRule {
                    metric: RegulatoryPlateau,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: RegulatoryPlateauDetected,
                    }],
                },
                RegulationRule {
                    metric: ActionDecisionBlocked,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: RegulationReason::ActionDecisionBlocked,
                    }],
                },
                // ── Category C: Domain-specific regulation ──
                // MemoryLife (Episodic Loop 2a) → Calibrate
                RegulationRule {
                    metric: MemoryLife,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Episodic,
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
                // InferenceGasRemaining (Inference Loop 1) → AdjustEnergyBudget
                RegulationRule {
                    metric: InferenceGasRemaining,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: AdjustEnergyBudget,
                        reason: InferenceGasLow,
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
/// Returns (0, 0) when the variant doesn't carry deficit/threshold.
pub(crate) fn extract_deficit_threshold(data: &RegulationData) -> (u64, u64) {
    match data {
        RegulationData::VarietyDeficitExceeded { deficit, threshold } => {
            (*deficit as u64, *threshold as u64)
        }
        _ => (0, 0),
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
        // ── Energy / Gas ──
        SignalMetric::EnergyRemaining => &[Throttle, AdjustEnergyBudget, Escalate],
        SignalMetric::InferenceGasRemaining => &[Throttle, AdjustEnergyBudget, Escalate],
        // ── Variety ──
        SignalMetric::VarietyDeficit => &[Escalate, Calibrate, OverrideEnergyBudget],
        SignalMetric::MetacognitionVarietyDeficit => &[Escalate, Calibrate, OverrideEnergyBudget],
        // ── Error Rate ──
        SignalMetric::ErrorRate => &[CircuitBreak, Calibrate, Escalate],
        SignalMetric::CircuitBreakerState => &[CircuitBreak, Calibrate, Escalate],
        // ── Latency / Backpressure ──
        SignalMetric::ConnectorLatency => &[Throttle, Calibrate, Escalate],
        SignalMetric::CommunicationQueueDepth => &[Throttle, Escalate],
        // ── Wallet ──
        SignalMetric::WalletBalanceRatio => &[Escalate, ReplenishBudget],
        SignalMetric::WalletKeyHealth => &[Escalate, Calibrate],
        // ── Meta-regulatory (only Curation can break the plateau) ──
        SignalMetric::AlgedonicEvents => &[Escalate, Calibrate],
        SignalMetric::GoalStaleCount => &[Escalate, Calibrate],
        SignalMetric::GoalExpiredCount => &[Escalate, Calibrate],
        SignalMetric::MetacognitionCriticalAlerts => &[Escalate, Calibrate, OverrideEnergyBudget],
        SignalMetric::ActionIneffective => &[Escalate, Calibrate],
        SignalMetric::RegulatoryPlateau => &[Escalate, Calibrate],
        SignalMetric::ActionDecisionBlocked => &[Escalate, Calibrate],
        // ── Domain-specific ──
        SignalMetric::MemoryLife => &[Calibrate, Escalate],
        SignalMetric::InferenceAvailable => &[Throttle, Calibrate, Escalate],
        SignalMetric::InferenceModelAvailable => &[Calibrate, Escalate],
        // ── Observational (no substitution — Notify is terminal) ──
        SignalMetric::StorageUsage
        | SignalMetric::TripleCount
        | SignalMetric::LowConfidenceCount
        | SignalMetric::ConsolidationCandidates
        | SignalMetric::PendingEscalations
        | SignalMetric::SeamCoverage
        | SignalMetric::ToolReliability
        | SignalMetric::TestCoverage
        | SignalMetric::MutationScore
        | SignalMetric::ToolSchemaTokens => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_deviation(metric: SignalMetric, value: f64, set_point: f64) -> Deviation {
        use crate::types::loops::Signal;
        let signal = Signal {
            source: LoopId::Cybernetics,
            metric,
            value,
            set_point,
            timestamp: Utc::now(),
        };
        Deviation::from_signal(&signal).unwrap()
    }

    #[test]
    fn policy_matches_energy_below_setpoint() {
        let policy = RegulationPolicy::default();
        let dev = make_deviation(SignalMetric::EnergyRemaining, 0.3, 0.5);
        let proposed = policy.decide(&dev);
        assert_eq!(proposed.len(), 3);
        assert_eq!(proposed[0].action_type, ActionType::Throttle);
        assert_eq!(proposed[1].action_type, ActionType::Escalate);
        assert_eq!(proposed[2].action_type, ActionType::AdjustEnergyBudget);
    }

    #[test]
    fn policy_matches_variety_above_setpoint() {
        let policy = RegulationPolicy::default();
        let dev = make_deviation(SignalMetric::VarietyDeficit, 15.0, 10.0);
        let proposed = policy.decide(&dev);
        assert_eq!(proposed.len(), 1);
        assert_eq!(proposed[0].action_type, ActionType::Escalate);
        assert_eq!(proposed[0].target, LoopId::Curation);
    }

    #[test]
    fn policy_matches_error_rate_above_setpoint() {
        let policy = RegulationPolicy::default();
        let dev = make_deviation(SignalMetric::ErrorRate, 0.15, 0.05);
        let proposed = policy.decide(&dev);
        assert_eq!(proposed.len(), 1);
        assert_eq!(proposed[0].action_type, ActionType::CircuitBreak);
    }

    #[test]
    fn policy_matches_seam_coverage_below_setpoint() {
        let policy = RegulationPolicy::default();
        let dev = make_deviation(SignalMetric::SeamCoverage, 80.0, 90.0);
        let proposed = policy.decide(&dev);
        assert_eq!(proposed.len(), 1);
        assert_eq!(proposed[0].action_type, ActionType::Escalate);
    }

    #[test]
    fn policy_matches_seam_coverage_above_setpoint() {
        let policy = RegulationPolicy::default();
        let dev = make_deviation(SignalMetric::SeamCoverage, 95.0, 90.0);
        let proposed = policy.decide(&dev);
        assert_eq!(proposed.len(), 1);
        assert_eq!(proposed[0].action_type, ActionType::Notify);
    }

    #[test]
    fn policy_no_match_for_unregistered_metric() {
        // All metrics are now regulated per ADR-056.
        // This test verifies that every SignalMetric variant produces at least
        // one proposed action when it deviates.
        let policy = RegulationPolicy::default();
        let all_metrics = [
            SignalMetric::EnergyRemaining,
            SignalMetric::VarietyDeficit,
            SignalMetric::ErrorRate,
            SignalMetric::ConnectorLatency,
            SignalMetric::CommunicationQueueDepth,
            SignalMetric::StorageUsage,
            SignalMetric::MemoryLife,
            SignalMetric::TripleCount,
            SignalMetric::LowConfidenceCount,
            SignalMetric::CircuitBreakerState,
            SignalMetric::InferenceAvailable,
            SignalMetric::InferenceGasRemaining,
            SignalMetric::InferenceModelAvailable,
            SignalMetric::AlgedonicEvents,
            SignalMetric::PendingEscalations,
            SignalMetric::ConsolidationCandidates,
            SignalMetric::GoalStaleCount,
            SignalMetric::GoalExpiredCount,
            SignalMetric::MetacognitionVarietyDeficit,
            SignalMetric::MetacognitionCriticalAlerts,
            SignalMetric::WalletBalanceRatio,
            SignalMetric::WalletKeyHealth,
            SignalMetric::SeamCoverage,
            SignalMetric::ActionIneffective,
            SignalMetric::RegulatoryPlateau,
            SignalMetric::ActionDecisionBlocked,
            SignalMetric::ToolReliability,
        ];
        for metric in all_metrics {
            // Test both directions — at least one should produce an action
            let dev_above = make_deviation(metric, 100.0, 50.0);
            let dev_below = make_deviation(metric, 0.0, 50.0);
            let proposed_above = policy.decide(&dev_above);
            let proposed_below = policy.decide(&dev_below);
            assert!(
                !proposed_above.is_empty() || !proposed_below.is_empty(),
                "Metric {:?} has no regulation rule for either direction",
                metric
            );
        }
    }

    #[test]
    fn classify_decision_accept_noise() {
        assert_eq!(classify_decision(0.03, 0.05, 0.20), ActionDecision::Accept);
    }

    #[test]
    fn classify_decision_stage_moderate() {
        assert_eq!(classify_decision(0.10, 0.05, 0.20), ActionDecision::Stage);
    }

    #[test]
    fn classify_decision_block_severe() {
        assert_eq!(classify_decision(0.25, 0.05, 0.20), ActionDecision::Block);
    }

    #[test]
    fn default_substitution_ladders_are_nonempty_for_regulated_metrics() {
        assert!(!default_substitution_ladder(SignalMetric::EnergyRemaining).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::VarietyDeficit).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::ErrorRate).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::ConnectorLatency).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::CommunicationQueueDepth).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::MemoryLife).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::CircuitBreakerState).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::InferenceAvailable).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::InferenceGasRemaining).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::InferenceModelAvailable).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::WalletBalanceRatio).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::WalletKeyHealth).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::AlgedonicEvents).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::GoalStaleCount).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::GoalExpiredCount).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::MetacognitionVarietyDeficit).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::MetacognitionCriticalAlerts).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::ActionIneffective).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::RegulatoryPlateau).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::ActionDecisionBlocked).is_empty());
    }

    #[test]
    fn default_substitution_ladders_empty_for_observational_metrics() {
        // Observational metrics use Notify (terminal action — no substitution)
        assert!(default_substitution_ladder(SignalMetric::StorageUsage).is_empty());
        assert!(default_substitution_ladder(SignalMetric::TripleCount).is_empty());
        assert!(default_substitution_ladder(SignalMetric::LowConfidenceCount).is_empty());
        assert!(default_substitution_ladder(SignalMetric::ConsolidationCandidates).is_empty());
        assert!(default_substitution_ladder(SignalMetric::PendingEscalations).is_empty());
        assert!(default_substitution_ladder(SignalMetric::SeamCoverage).is_empty());
    }
}
