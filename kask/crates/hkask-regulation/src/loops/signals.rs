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
    /// Storage usage fraction (Memory Loop 2)
    StorageUsage,
    /// Memory life S in days (Memory Loop 2).
    /// Wozniak-Gorzelanczyk (1995) forgetting curve: R(t) = exp(-t/S).
    /// Default 180 days. Configurable via HKASK_MEMORY_LIFE_DAYS.
    MemoryLife,
    /// h_mem count (Memory Loop 2)
    TripleCount,
    /// Low-confidence h_mem count (Memory Loop 2)
    LowConfidenceCount,

    /// Circuit breaker state 0.0/1.0 (Inference Loop 1)
    CircuitBreakerState,
    /// Inference availability 0.0/1.0 (Inference Loop 1)
    InferenceAvailable,
    /// Inference energy remaining fraction (Inference Loop 1)
    /// Model availability 0.0/1.0 (Inference Loop 1)
    InferenceModelAvailable,
    /// Context server health fraction 0.0/1.0 (Cybernetics Loop 6).
    /// 1.0 = all registered context servers are Running; 0.0 = all are
    /// stuck in Starting, Error, or AuthRequired. The set-point is 1.0.
    /// Without this metric the loop reports `signal_count=0` while every
    /// MCP server is hung on `initialize` — the blind-feedback-loop trap.
    ContextServerHealth,
    /// OCR silent-failure count in the recent window (Cybernetics Loop 6).
    /// Empty LLM OCR output on a page — the dead-but-responsive-endpoint
    /// signature (HTTP 200 with empty content). Sensed from the corpus
    /// server's cross-process health file. Set-point 0.0; any positive
    /// count is a deviation. Without this metric the loop reports
    /// `signal_count=0` during an OCR silent-failure storm because the
    /// warns live in the corpus subprocess's tracing, not the loop's
    /// ledger/DB state — the same blind-feedback-loop trap.
    OcrSilentFailures,
    /// Actionable algedonic alert count — Warning or Critical entries in
    /// the in-memory log (Cybernetics Loop 6). Info diagnostics don't count.
    AlgedonicEvents,
    /// Algedonic log approaching cap (Cybernetics Loop 6).
    /// 1.0 when the in-memory alert log is ≥ 80% of its cap, 0.0 otherwise.
    /// The operator (or the `algedonic-review` skill) should review and clear
    /// reviewed entries before they are evicted unread.
    AlgedonicLogApproachingCap,
    /// Pending escalation count (Curation Loop 5)
    PendingEscalations,
    /// Consolidation candidate count (Memory consolidation bridge)
    ConsolidationCandidates,
    /// Stale goal count (Curation Loop 5)
    GoalStaleCount,
    /// Expired goal count (Curation Loop 5)
    GoalExpiredCount,
    /// Metacognition critical alert count (Curation Loop 5)
    MetacognitionCriticalAlerts,

    // MetacognitionVarietyDeficit removed 2026-08-30 — a pure duplicate of
    // VarietyDeficit (same ledger `overall_deficit`, same Escalate→Curation
    // rule); wiring it would have double-escalated the same number.
    // ActionIneffective / RegulatoryPlateau / ActionDecisionBlocked removed
    // with it — superseded by the loop's direct escalation paths
    // (`try_substitute` at cycle.rs, plateau/blocked alerts persisted to the
    // review queue and sensed as PendingEscalations).
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
            SignalMetric::ContextServerHealth => "context_server_health",
            SignalMetric::OcrSilentFailures => "ocr_silent_failures",
            SignalMetric::AlgedonicEvents => "algedonic_events",
            SignalMetric::AlgedonicLogApproachingCap => "algedonic_log_approaching_cap",
            SignalMetric::PendingEscalations => "pending_escalations",
            SignalMetric::ConsolidationCandidates => "consolidation_candidates",
            SignalMetric::GoalStaleCount => "goal_stale_count",
            SignalMetric::GoalExpiredCount => "goal_expired_count",
            SignalMetric::MetacognitionCriticalAlerts => "metacognition_critical_alerts",
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
            SignalMetric::ContextServerHealth,
            SignalMetric::OcrSilentFailures,
            SignalMetric::AlgedonicEvents,
            SignalMetric::AlgedonicLogApproachingCap,
            SignalMetric::PendingEscalations,
            SignalMetric::ConsolidationCandidates,
            SignalMetric::GoalStaleCount,
            SignalMetric::GoalExpiredCount,
            SignalMetric::MetacognitionCriticalAlerts,
            SignalMetric::ToolReliability,
            SignalMetric::TestCoverage,
            SignalMetric::MutationScore,
        ]
        .into_iter()
        .find(|metric| metric.as_str() == name)
    }

    /// Whether an increase in this metric is an improvement — the impact
    /// direction `verify_impact` compares its before/after delta against.
    /// `None` for metrics with no verified impact path; `verify_impact`
    /// falls back to treating any nonzero delta as a change.
    ///
    /// This is the per-metric direction table, colocated with the metric
    /// it describes.
    pub fn impact_direction(&self) -> Option<bool> {
        match self {
            SignalMetric::EnergyRemaining
            | SignalMetric::ContextServerHealth
            | SignalMetric::ToolReliability => Some(true),
            SignalMetric::VarietyDeficit | SignalMetric::OcrSilentFailures => Some(false),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: "Advice is assessed seven days after confirmed action, with absent evidence kept unknown" [P9]
    #[test]
    fn weekly_advice_review_distinguishes_progress_from_acceptance() {
        let applied = chrono::Utc::now();
        let mut trigger = Signal::new(LoopId::Cybernetics, SignalMetric::ToolReliability, 0.3, 0.8);
        trigger.timestamp = applied;
        let mut current = trigger.clone();
        let now = applied + chrono::Duration::days(7);
        current.timestamp = now;
        assert_eq!(
            trigger.advice_review(Some(&trigger), Some(&current), None, now),
            "awaiting_action"
        );
        assert_eq!(
            trigger.advice_review(
                Some(&trigger),
                Some(&current),
                Some(applied),
                applied + chrono::Duration::days(6)
            ),
            "observation_window"
        );
        assert_eq!(
            trigger.advice_review(Some(&trigger), Some(&current), Some(applied), now),
            "no_improvement"
        );
        current.value = 0.4;
        assert_eq!(
            trigger.advice_review(Some(&trigger), Some(&current), Some(applied), now),
            "improved"
        );
        current.value = 0.8;
        assert_eq!(
            trigger.advice_review(Some(&trigger), Some(&current), Some(applied), now),
            "recovered"
        );
        assert_eq!(
            trigger.advice_review(Some(&trigger), None, Some(applied), now),
            "insufficient_evidence"
        );
        current.timestamp = applied;
        assert_eq!(
            trigger.advice_review(Some(&trigger), Some(&current), Some(applied), now),
            "insufficient_evidence"
        );
    }

    /// Pins the per-metric impact direction: energy remaining, fleet
    /// health, and tool reliability improve upward; variety deficit
    /// improves downward; everything else has no verified impact path
    /// (`verify_impact` falls back to any-nonzero-delta).
    #[test]
    fn impact_direction_covers_the_verifiable_metrics() {
        assert_eq!(SignalMetric::EnergyRemaining.impact_direction(), Some(true));
        assert_eq!(
            SignalMetric::ContextServerHealth.impact_direction(),
            Some(true)
        );
        assert_eq!(SignalMetric::ToolReliability.impact_direction(), Some(true));
        assert_eq!(SignalMetric::VarietyDeficit.impact_direction(), Some(false));
        assert_eq!(
            SignalMetric::OcrSilentFailures.impact_direction(),
            Some(false)
        );
        assert_eq!(SignalMetric::ErrorRate.impact_direction(), None);
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
            "context_server_health",
            "ocr_silent_failures",
            "algedonic_events",
            "algedonic_log_approaching_cap",
            "pending_escalations",
            "consolidation_candidates",
            "goal_stale_count",
            "goal_expired_count",
            "metacognition_critical_alerts",
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
    /// Evidence must describe the current observation window, not an old sample
    /// or a future timestamp. Missing data is represented by no Signal.
    pub fn is_fresh_at(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.value.is_finite()
            && self.set_point.is_finite()
            && self.timestamp <= now
            && self.timestamp
                >= now - chrono::Duration::seconds(crate::OBSERVATION_WINDOW_SECS as i64)
    }

    pub fn is_recovery_trigger(&self) -> bool {
        Deviation::from_signal(self).is_some()
    }

    /// Seven-day post-application review, not a causal-effect estimate.
    pub fn advice_review(
        &self,
        baseline: Option<&Signal>,
        current: Option<&Signal>,
        applied_at: Option<chrono::DateTime<chrono::Utc>>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> &'static str {
        let Some(applied_at) = applied_at else {
            return "awaiting_action";
        };
        if now < applied_at + chrono::Duration::days(7) {
            return "observation_window";
        }
        let (Some(baseline), Some(current)) = (baseline, current) else {
            return "insufficient_evidence";
        };
        if !self.is_recovery_trigger()
            || baseline.metric != self.metric
            || current.metric != self.metric
            || !baseline.is_fresh_at(applied_at)
            || !current.is_fresh_at(now)
        {
            return "insufficient_evidence";
        }
        if self.recovered_by(current) {
            "recovered"
        } else if (self.value < self.set_point && current.value > baseline.value)
            || (self.value > self.set_point && current.value < baseline.value)
        {
            "improved"
        } else {
            "no_improvement"
        }
    }

    /// Whether a fresh observation crosses this original trigger's threshold
    /// back toward health. Neither missing nor non-finite data proves recovery.
    pub fn recovered_by(&self, current: &Signal) -> bool {
        self.is_recovery_trigger()
            && self.metric == current.metric
            && current.timestamp >= self.timestamp
            && current.value.is_finite()
            && self.set_point.is_finite()
            && if self.value < self.set_point {
                current.value >= self.set_point
            } else if self.value > self.set_point {
                current.value <= self.set_point
            } else {
                false
            }
    }

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
        if !signal.value.is_finite() || !signal.set_point.is_finite() {
            return None;
        }
        let diff = signal.value - signal.set_point;
        let healthy = match signal.metric {
            SignalMetric::EnergyRemaining
            | SignalMetric::ContextServerHealth
            | SignalMetric::ToolReliability
            | SignalMetric::TestCoverage
            | SignalMetric::MutationScore
            | SignalMetric::InferenceAvailable
            | SignalMetric::InferenceModelAvailable => diff >= 0.0,
            SignalMetric::VarietyDeficit | SignalMetric::OcrSilentFailures => diff <= 0.0,
            _ => false,
        };
        if healthy || diff.abs() < f64::EPSILON {
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
