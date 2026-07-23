//! RegulationPolicy — data-driven per-metric regulation rules
//!
//! Consolidates the per-metric action mappings, severity thresholds,
//! and classification thresholds that were previously scattered across
//! `cybernetics_loop.rs`. Each `RegulationRule` defines what actions
//! to take when a specific metric deviates in a specific direction.

use crate::types::loops::{
    ActionDecision, ActionType, Deviation, DeviationDirection, LoopId, RegulationData, SignalMetric,
};

/// A proposed action before substitution and mode-specific filtering.
///
/// The `compute()` method applies `try_substitute` and mode checks
/// to finalize these into `RegulatoryAction` instances.
///
/// Fields are read by `build_regulation_action` via string matching
/// on `reason`; `target`, `action_type`, `data`, and `metric_name`
/// serve as documentation of each rule's intent. `data` is `None` in
/// the policy table because concrete values come from the `Deviation`
/// at runtime.
// Fields serve as documentation of each rule's intent; `build_regulation_action`
// reads via string matching on `reason`. `data` is `None` in the policy table
// because concrete values come from the `Deviation` at runtime.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ProposedAction {
    pub target: LoopId,
    pub action_type: ActionType,
    pub reason: &'static str,
    pub data: Option<RegulationData>,
    pub metric_name: Option<&'static str>,
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
        use SignalMetric::*;

        Self {
            rules: vec![
                // ── Energy / Gas (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: EnergyRemaining,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: Throttle,
                        reason: "energy_budget_low",
                        data: None,
                        metric_name: Some("energy_remaining"),
                    }],
                },
                RegulationRule {
                    metric: EnergyRemaining,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "budget_guard_escalation",
                        data: None,
                        metric_name: Some("energy_remaining"),
                    }],
                },
                RegulationRule {
                    metric: EnergyRemaining,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Cybernetics,
                        action_type: AdjustEnergyBudget,
                        reason: "energy_depletion_auto_adjust",
                        data: None,
                        metric_name: None,
                    }],
                },
                // ── Variety (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: VarietyDeficit,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "variety_deficit_exceeded",
                        data: None,
                        metric_name: None,
                    }],
                },
                // ── Error Rate (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: ErrorRate,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: CircuitBreak,
                        reason: "error_rate_exceeded",
                        data: None,
                        metric_name: None,
                    }],
                },
                // ── Connector Latency (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: ConnectorLatency,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Cybernetics,
                        action_type: Throttle,
                        reason: "connector_latency_exceeded",
                        data: None,
                        metric_name: None,
                    }],
                },
                // ── Communication Queue Depth (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: CommunicationQueueDepth,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Cybernetics,
                        action_type: Throttle,
                        reason: "communication_backpressure",
                        data: None,
                        metric_name: None,
                    }],
                },
                // ── Wallet (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: WalletBalanceRatio,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "wallet_balance_low",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: WalletKeyHealth,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "wallet_key_unhealthy",
                        data: None,
                        metric_name: None,
                    }],
                },
                // ── Seam Coverage (Seam Watcher R7.3) ──
                RegulationRule {
                    metric: SeamCoverage,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "seam_coverage_degraded",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: SeamCoverage,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: "seam_coverage_improved",
                        data: None,
                        metric_name: None,
                    }],
                },
                // ── Tool Reliability (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: ToolReliability,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "tool_reliability_degraded",
                        data: None,
                        metric_name: None,
                    }],
                },
                // ── Category A: Observational metrics → Notify (no regulation needed) ──
                RegulationRule {
                    metric: StorageUsage,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: "storage_usage_observed",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: TripleCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: "triple_count_observed",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: LowConfidenceCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: "low_confidence_count_observed",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: SnapshotInterval,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: "snapshot_interval_exceeded",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: ConsolidationCandidates,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: "consolidation_candidates_observed",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: PendingEscalations,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: "pending_escalations_observed",
                        data: None,
                        metric_name: None,
                    }],
                },
                // ── Category B: Meta-regulatory metrics → Escalate to Curation ──
                RegulationRule {
                    metric: AlgedonicEvents,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "algedonic_events_exceeded",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: GoalStaleCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "goals_stale",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: GoalExpiredCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "goals_expired",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: MetacognitionVarietyDeficit,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "metacognition_variety_deficit",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: MetacognitionCriticalAlerts,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "metacognition_critical_alerts",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: ActionIneffective,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "action_ineffective",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: RegulatoryPlateau,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "regulatory_plateau_detected",
                        data: None,
                        metric_name: None,
                    }],
                },
                RegulationRule {
                    metric: ActionDecisionBlocked,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: "action_decision_blocked",
                        data: None,
                        metric_name: None,
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
                        reason: "memory_life_low",
                        data: None,
                        metric_name: None,
                    }],
                },
                // CircuitBreakerState (Inference Loop 1) → Throttle
                RegulationRule {
                    metric: CircuitBreakerState,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: Throttle,
                        reason: "circuit_breaker_open",
                        data: None,
                        metric_name: None,
                    }],
                },
                // InferenceAvailable (Inference Loop 1) → Throttle
                RegulationRule {
                    metric: InferenceAvailable,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: Throttle,
                        reason: "inference_unavailable",
                        data: None,
                        metric_name: None,
                    }],
                },
                // InferenceGasRemaining (Inference Loop 1) → AdjustEnergyBudget
                RegulationRule {
                    metric: InferenceGasRemaining,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: AdjustEnergyBudget,
                        reason: "inference_gas_low",
                        data: None,
                        metric_name: None,
                    }],
                },
                // InferenceModelAvailable (Inference Loop 1) → Calibrate
                RegulationRule {
                    metric: InferenceModelAvailable,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Inference,
                        action_type: Calibrate,
                        reason: "model_unavailable",
                        data: None,
                        metric_name: None,
                    }],
                },
                // DiskUsagePct (StorageGuard Loop 7) → Prune
                RegulationRule {
                    metric: DiskUsagePct,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: StorageGuard,
                        action_type: Prune,
                        reason: "disk_usage_exceeded",
                        data: None,
                        metric_name: None,
                    }],
                },
                // McpServerHealth (McpServerGuard Loop 8) → CircuitBreak
                RegulationRule {
                    metric: McpServerHealth,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: McpServerGuard,
                        action_type: CircuitBreak,
                        reason: "mcp_server_unhealthy",
                        data: None,
                        metric_name: None,
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
pub fn extract_deficit_threshold(data: &RegulationData) -> (u64, u64) {
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
pub fn classify_decision(worsening: f64, stage_ratio: f64, block_ratio: f64) -> ActionDecision {
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
pub fn default_substitution_ladder(metric: SignalMetric) -> &'static [ActionType] {
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
        SignalMetric::DiskUsagePct => &[Prune, Escalate],
        SignalMetric::McpServerHealth => &[CircuitBreak, Calibrate, Escalate],
        // ── Observational (no substitution — Notify is terminal) ──
        SignalMetric::StorageUsage
        | SignalMetric::TripleCount
        | SignalMetric::LowConfidenceCount
        | SignalMetric::SnapshotInterval
        | SignalMetric::ConsolidationCandidates
        | SignalMetric::PendingEscalations
        | SignalMetric::SeamCoverage
        | SignalMetric::ToolReliability => &[],
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
            SignalMetric::SnapshotInterval,
            SignalMetric::WalletBalanceRatio,
            SignalMetric::WalletKeyHealth,
            SignalMetric::DiskUsagePct,
            SignalMetric::McpServerHealth,
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
        assert!(!default_substitution_ladder(SignalMetric::DiskUsagePct).is_empty());
        assert!(!default_substitution_ladder(SignalMetric::McpServerHealth).is_empty());
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
        assert!(default_substitution_ladder(SignalMetric::SnapshotInterval).is_empty());
        assert!(default_substitution_ladder(SignalMetric::ConsolidationCandidates).is_empty());
        assert!(default_substitution_ladder(SignalMetric::PendingEscalations).is_empty());
        assert!(default_substitution_ladder(SignalMetric::SeamCoverage).is_empty());
    }
}
