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
    /// The in-memory algedonic alert log is approaching its cap. The operator
    /// (or the `algedonic-review` skill) should review and clear reviewed
    /// entries before they are evicted unread.
    AlgedonicLogApproachingCap,
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
    ModelUnavailable,
    /// Grounding clean rate dropped below the configured floor — more than
    /// the tolerated fraction of grounded delegations have nulled fields.
    /// A quality regression: a tool broke, an agent's prompt drifted, or a
    /// model was swapped. The operator's remediation is to investigate the
    /// recent violations (via `curator_grounding_violations`) and fix the
    /// root cause — the regulation system does not auto-fix grounding
    /// contracts (that's a human decision).
    GroundingCleanRateDegraded,
    /// Grounding coverage rate dropped below the configured floor — more
    /// than the tolerated fraction of delegations have no grounding contract.
    /// A coverage gap (paper §6): agent types exist with delegations but no
    /// contract. The operator's remediation is to register contracts for the
    /// uncovered agent types.
    GroundingCoverageDegraded,
    /// Grounding violation delta is positive — new nulled fields appeared
    /// since the last sense tick. The spike is the regulation signal that
    /// something changed. Routed as Escalate so the Curator surfaces it to
    /// the user via the algedonic alert path.
    GroundingViolationDeltaIncreased,
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
            Self::AlgedonicLogApproachingCap => "algedonic_log_approaching_cap",
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
            Self::ModelUnavailable => "model_unavailable",
            Self::GroundingCleanRateDegraded => "grounding_clean_rate_degraded",
            Self::GroundingCoverageDegraded => "grounding_coverage_degraded",
            Self::GroundingViolationDeltaIncreased => "grounding_violation_delta_increased",
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
            ActionDecisionBlocked, ActionIneffective, AlgedonicLogApproachingCap,
            MetacognitionCriticalAlerts, MetacognitionVarietyDeficit, *,
        };

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
///
/// For grounding variants, the deficit/threshold vocabulary is adapted:
/// - `GroundingCleanRateDegraded`: deficit = floor - clean_rate (how far
///   below floor), threshold = floor scaled to 100 for readability.
/// - `GroundingCoverageDegraded`: deficit = floor - coverage_rate, threshold
///   = floor scaled to 100.
/// - `GroundingViolationDeltaIncreased`: deficit = delta (new violations),
///   threshold = 0 (any positive delta is a deviation).
/// This ensures the `error_context` JSON in the escalation queue carries
/// meaningful values, not (0, 0) which looks like a bug.
pub(crate) fn extract_deficit_threshold(data: &RegulationData) -> (u64, u64) {
    match data {
        RegulationData::VarietyDeficitExceeded { deficit, threshold } => {
            (*deficit as u64, *threshold as u64)
        }
        RegulationData::GroundingCleanRateDegraded { clean_rate, floor } => {
            // Deficit = how far below the floor (scaled to 0-100).
            // A clean_rate of 0.5 with floor 0.8 → deficit 30, threshold 80.
            let deficit = ((floor - clean_rate) * 100.0).max(0.0) as u64;
            let threshold = (floor * 100.0) as u64;
            (deficit, threshold)
        }
        RegulationData::GroundingCoverageDegraded {
            coverage_rate,
            floor,
        } => {
            let deficit = ((floor - coverage_rate) * 100.0).max(0.0) as u64;
            let threshold = (floor * 100.0) as u64;
            (deficit, threshold)
        }
        RegulationData::GroundingViolationDeltaIncreased { delta } => {
            // Deficit = the delta (new violations). Threshold = 0 (any
            // positive delta is a deviation).
            (*delta as u64, 0)
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
        // ── Energy ──
        SignalMetric::EnergyRemaining => &[Throttle, AdjustEnergyBudget, Escalate],
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
        SignalMetric::AlgedonicLogApproachingCap => &[Escalate, Calibrate],
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
        // Grounding metrics route to Escalate (terminal for grounding — the
        // regulation system does not auto-fix grounding contracts, that's a
        // human decision). No substitution ladder.
        SignalMetric::StorageUsage
        | SignalMetric::TripleCount
        | SignalMetric::LowConfidenceCount
        | SignalMetric::ConsolidationCandidates
        | SignalMetric::PendingEscalations
        | SignalMetric::SeamCoverage
        | SignalMetric::ToolReliability
        | SignalMetric::TestCoverage
        | SignalMetric::MutationScore
        => &[],
    }
}
