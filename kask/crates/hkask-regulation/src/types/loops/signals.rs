//! Signal types — metrics, afferent signals, deviations, and deviation direction.
//!
//! These types have no Regulation-internal dependencies — only LoopId, serde, and chrono.

use super::core::LoopId;

/// Metric names for afferent signals from loop sensing.
///
/// Each variant identifies the kind of measurement a signal carries,
/// replacing magic strings with an exhaustive, type-safe enum
/// (Fowler H7: Replace Type Code with Strategy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalMetric {
    /// Fraction of energy budget remaining (Cybernetics Loop 6)
    EnergyRemaining,
    /// Raw variety deficit count (Cybernetics Loop 6)
    VarietyDeficit,
    /// Error rate as a fraction (Cybernetics Loop 6)
    ErrorRate,
    /// Connector latency in milliseconds (Cybernetics Loop 6)
    ConnectorLatency,
    /// Communication queue depth (backpressure signal)
    CommunicationQueueDepth,
    /// Episodic storage usage fraction (Episodic Loop 2a)
    StorageUsage,
    /// Episodic memory life S in days (Episodic Loop 2a).
    /// Wozniak-Gorzelanczyk (1995) forgetting curve: R(t) = exp(-t/S).
    /// Default 180 days. Configurable via HKASK_MEMORY_LIFE_DAYS.
    MemoryLife,
    /// Semantic h_mem count (Semantic Loop 2b)
    TripleCount,
    /// Low-confidence h_mem count (Semantic Loop 2b)
    LowConfidenceCount,

    /// Circuit breaker state 0.0/1.0 (Inference Loop 1)
    CircuitBreakerState,
    /// Inference availability 0.0/1.0 (Inference Loop 1)
    InferenceAvailable,
    /// Inference energy remaining fraction (Inference Loop 1)
    /// Model availability 0.0/1.0 (Inference Loop 1)
    InferenceModelAvailable,
    /// Algedonic event count (Cybernetics Loop 6)
    AlgedonicEvents,
    /// Pending escalation count (Curation Loop 5)
    PendingEscalations,
    /// Consolidation candidate count (Episodic → Semantic bridge)
    ConsolidationCandidates,
    /// Stale goal count (Curation Loop 5)
    GoalStaleCount,
    /// Expired goal count (Curation Loop 5)
    GoalExpiredCount,
    /// Metacognition variety deficit (Curation Loop 5)
    MetacognitionVarietyDeficit,
    /// Metacognition critical alert count (Curation Loop 5)
    MetacognitionCriticalAlerts,

    /// Wallet rJoule balance ratio (0.0 = empty, 1.0 = full relative to 30-day avg)
    WalletBalanceRatio,

    /// Wallet API key health (1.0 = exhausted/expired, 0.0 = healthy)
    WalletKeyHealth,
    /// Public seam coverage ratio per crate (seam watcher, 0.0–100.0)
    SeamCoverage,
    /// A regulatory action has been ineffective over multiple cycles.
    /// 0.0 = all actions effective, 1.0 = all actions ineffective.
    /// Triggers escalation to Curation for metacognitive override.
    ActionIneffective,
    /// The loop has reached a regulatory plateau — same deviation→action
    /// pattern repeats without metric improvement. Indicates the regulator's
    /// model has converged to a wrong attractor (Conant-Ashby violation).
    RegulatoryPlateau,
    /// An action was blocked because it was severely counterproductive
    /// (Fermi HardBlock pattern). The (metric, action_type) pair is
    /// prevented from re-use until Curation intervenes.
    ActionDecisionBlocked,
    /// Tool reliability: success probability has dropped below threshold.
    /// 0.0 = 0% success rate, 1.0 = 100% success rate.
    /// Set-point: reliability_threshold (default 0.80).
    ToolReliability,
    /// Test coverage fraction (Cybernetics Loop 6).
    /// Read from the latest trace run's `metrics.json` `coverage_pct`.
    /// Set-point: coverage_floor (default 0.70).
    TestCoverage,
    /// Mutation score fraction (Cybernetics Loop 6).
    /// Read from the latest trace run's `metrics.json` `mutation_score`.
    /// Set-point: mutation_score_floor (default 0.50).
    MutationScore,
    /// Grounding clean rate (Cybernetics Loop 6).
    /// Fraction of grounded delegations (contract ran) with zero nulled
    /// fields. `None` (encoded as a signal value of -1.0) when no grounded
    /// delegations exist (absence ≠ 0 — paper Rule 5.3). Higher is better.
    /// Set-point: grounding_clean_rate_floor (default 0.80).
    GroundingCleanRate,
    /// Grounding coverage rate (Cybernetics Loop 6).
    /// Fraction of delegations that had a grounding contract. `None`
    /// (encoded as -1.0) when no delegations exist. Higher is better.
    /// Set-point: grounding_coverage_rate_floor (default 0.50).
    GroundingCoverageRate,
    /// Grounding violation delta (Cybernetics Loop 6).
    /// Change in `delegations_with_nulled` since the last sense tick.
    /// Positive = getting worse (new violations). Negative = improving.
    /// Zero = stable. The signal value is the raw delta; the set-point is 0
    /// (any positive delta is a deviation — a spike in nulled fields is the
    /// regulation signal that something changed: a tool broke, an agent's
    /// prompt drifted, a model was swapped).
    GroundingViolationDelta,
    /// Grounding liveness gap (Cybernetics Loop 6).
    /// Count of delegations that should have produced a grounding record
    /// but didn't. A delegation that skipped `enforce_for_agent` entirely
    /// produces no record — absence of a record is indistinguishable from
    /// "clean." This metric surfaces that gap. Requires a second data
    /// source (swarm ledger) to compute the true gap; until then, returns
    /// 0.0 when records exist and None when the store is empty (absence ≠ 0).
    /// Set-point: 0 (any positive value is a deviation — delegations are
    /// happening without grounding enforcement).
    GroundingLivenessGap,
}

impl std::fmt::Display for SignalMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", self));
        write!(f, "{}", s)
    }
}

impl SignalMetric {
    /// Returns the snake_case string representation for comparison.
    pub fn as_str(&self) -> &'static str {
        match self {
            SignalMetric::EnergyRemaining => "energy_remaining",
            SignalMetric::VarietyDeficit => "variety_deficit",
            SignalMetric::ErrorRate => "error_rate",
            SignalMetric::ConnectorLatency => "connector_latency",
            SignalMetric::CommunicationQueueDepth => "communication_queue_depth",
            SignalMetric::StorageUsage => "storage_usage",
            SignalMetric::MemoryLife => "memory_life",
            SignalMetric::TripleCount => "triple_count",
            SignalMetric::LowConfidenceCount => "low_confidence_count",

            SignalMetric::CircuitBreakerState => "circuit_breaker_state",
            SignalMetric::InferenceAvailable => "inference_available",
            SignalMetric::InferenceModelAvailable => "inference_model_available",
            SignalMetric::AlgedonicEvents => "algedonic_events",
            SignalMetric::PendingEscalations => "pending_escalations",
            SignalMetric::ConsolidationCandidates => "consolidation_candidates",
            SignalMetric::GoalStaleCount => "goal_stale_count",
            SignalMetric::GoalExpiredCount => "goal_expired_count",
            SignalMetric::MetacognitionVarietyDeficit => "metacognition_variety_deficit",
            SignalMetric::MetacognitionCriticalAlerts => "metacognition_critical_alerts",

            SignalMetric::WalletBalanceRatio => "wallet_balance_ratio",

            SignalMetric::WalletKeyHealth => "wallet_key_health",
            SignalMetric::SeamCoverage => "seam_coverage",
            SignalMetric::ActionIneffective => "action_ineffective",
            SignalMetric::RegulatoryPlateau => "regulatory_plateau",
            SignalMetric::ActionDecisionBlocked => "action_decision_blocked",
            SignalMetric::ToolReliability => "tool_reliability",
            SignalMetric::TestCoverage => "test_coverage",
            SignalMetric::MutationScore => "mutation_score",
            SignalMetric::GroundingCleanRate => "grounding_clean_rate",
            SignalMetric::GroundingCoverageRate => "grounding_coverage_rate",
            SignalMetric::GroundingViolationDelta => "grounding_violation_delta",
            SignalMetric::GroundingLivenessGap => "grounding_liveness_gap",
        }
    }
}

/// Afferent signal from a loop's sensing phase.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Signal {
    pub source: LoopId,
    pub metric: SignalMetric,
    pub value: f64,
    pub set_point: f64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl Signal {
    pub fn new(source: LoopId, metric: SignalMetric, value: f64, set_point: f64) -> Self {
        Self {
            source,
            metric,
            value,
            set_point,
            timestamp: chrono::Utc::now(),
        }
    }
}

/// Deviation detected when comparing a signal against its set-point.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Deviation {
    pub signal: Signal,
    pub magnitude: f64,
    pub direction: DeviationDirection,
}

impl Deviation {
    pub fn from_signal(signal: &Signal) -> Option<Self> {
        let diff = signal.value - signal.set_point;
        if diff.abs() < f64::EPSILON {
            return None;
        }
        Some(Self {
            signal: signal.clone(),
            magnitude: diff.abs(),
            direction: if diff > 0.0 {
                DeviationDirection::AboveSetPoint
            } else {
                DeviationDirection::BelowSetPoint
            },
        })
    }
}

/// Direction of a deviation relative to the set-point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeviationDirection {
    AboveSetPoint,
    BelowSetPoint,
}
