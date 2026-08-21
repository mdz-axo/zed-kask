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
pub(crate) enum SignalMetric {
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
    /// Algedonic log approaching cap (Cybernetics Loop 6).
    /// 1.0 when the in-memory alert log is ≥ 80% of its cap, 0.0 otherwise.
    /// The operator (or the `algedonic-review` skill) should review and clear
    /// reviewed entries before they are evicted unread.
    AlgedonicLogApproachingCap,
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
            SignalMetric::AlgedonicLogApproachingCap => "algedonic_log_approaching_cap",
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
        }
    }

    /// Parse a metric from its snake_case name (the inverse of `as_str`).
    /// `None` for unknown names — callers decide the fallback, never a
    /// silent default that would mislabel the report.
    pub fn from_str_name(name: &str) -> Option<Self> {
        [
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
            SignalMetric::InferenceModelAvailable,
            SignalMetric::AlgedonicEvents,
            SignalMetric::AlgedonicLogApproachingCap,
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
            SignalMetric::TestCoverage,
            SignalMetric::MutationScore,
        ]
        .into_iter()
        .find(|metric| metric.as_str() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_name_round_trips() {
        // Every variant must survive as_str -> from_str_name. A variant
        // missing from the parse table would silently fall to the caller's
        // fallback and mislabel impact reports.
        let names = [
            "energy_remaining",
            "variety_deficit",
            "error_rate",
            "connector_latency",
            "communication_queue_depth",
            "storage_usage",
            "memory_life",
            "triple_count",
            "low_confidence_count",
            "circuit_breaker_state",
            "inference_available",
            "inference_model_available",
            "algedonic_events",
            "algedonic_log_approaching_cap",
            "pending_escalations",
            "consolidation_candidates",
            "goal_stale_count",
            "goal_expired_count",
            "metacognition_variety_deficit",
            "metacognition_critical_alerts",
            "wallet_balance_ratio",
            "wallet_key_health",
            "seam_coverage",
            "action_ineffective",
            "regulatory_plateau",
            "action_decision_blocked",
            "tool_reliability",
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
pub(crate) struct Deviation {
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
pub(crate) enum DeviationDirection {
    AboveSetPoint,
    BelowSetPoint,
}
