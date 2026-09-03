//! Loop action types — efferent actions and their type classification.

use super::core::LoopId;
use super::signals::SignalMetric;

/// Budget option presented to the Curator during budget guard escalation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BudgetOption {
    pub id: String,
    pub label: String,
}

/// Typed regulation data — replaces the previous `serde_json::Value` pass-through.
///
/// Each variant corresponds to a regulation reason. The `#[serde(tag = "reason")]`
/// encoding ensures serialized JSON is self-describing and backward-compatible
/// with consumers that inspect the `reason` field.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum RegulationData {
    /// Energy budget below set-point (Autonomous mode).
    EnergyBudgetLow {
        remaining_ratio: f64,
        set_point: f64,
    },
    /// Budget guard escalation to Curator (CuratorMediated mode).
    BudgetGuardEscalation {
        remaining_ratio: f64,
        set_point: f64,
        projected_minutes: u64,
        options: Vec<BudgetOption>,
        curator_timeout_secs: u64,
        fallback: String,
    },
    /// Automatic energy adjustment within set-point bounds.
    EnergyDepletionAutoAdjust {
        remaining_ratio: f64,
        set_point: f64,
    },
    /// Variety deficit exceeded threshold.
    VarietyDeficitExceeded { deficit: f64, threshold: f64 },
    /// Error rate exceeded threshold.
    ErrorRateExceeded { error_rate: f64, threshold: f64 },
    /// Connector latency exceeded threshold.
    ConnectorLatencyExceeded { latency_secs: f64, threshold: f64 },
    /// Communication queue backpressure.
    CommunicationBackpressure { queue_depth: f64, threshold: f64 },
    // Wallet and SeamCoverage data variants removed 2026-08-30 with their
    // policy rules — residuals of the deleted wallet module (219c74b180)
    // and a never-built seam watcher.
    /// Tool reliability degraded below threshold.
    ToolReliabilityDegraded { reliability: f64, threshold: f64 },
    /// Context-server fleet health degraded — some registered servers are
    /// stuck in `Starting` or `Error` instead of `Running`.
    ///
    /// Carries the snapshot at escalation time so `verify_impact` can re-sense
    /// and compare. `healthy_count` / `total_count` are the fleet counts from
    /// `ContextServerHealthSource` at the moment the action was built.
    ContextServerFleetHealth {
        healthy_count: u64,
        total_count: u64,
    },
    /// OCR silent failures exceeded the set-point (0) — the corpus OCR
    /// endpoint returned empty output on page(s) within the recent window.
    ///
    /// Carries the count at escalation time so `verify_impact` can re-sense
    /// and compare: as the storm's entries age out of the window the count
    /// declines, the re-sensed delta turns negative (improvement for a
    /// ceiling metric), and `auto_resolve_cleared` closes the escalation
    /// without operator action.
    OcrSilentFailuresExceeded { count: f64, threshold: f64 },
    /// Curator (metacognition) budget override directed at a named agent.
    ///
    /// Carries the LLM-produced target agent name and new budget so `act()`
    /// can issue a `CuratorDirective::OverrideEnergyBudget` without losing the
    /// values (previously the action carried only a reason string and the
    /// budget/target were silently dropped).
    CuratorBudgetOverride { agent: String, new_budget: u64 },
    /// A regulatory action whose impact should be verified against a rollout
    /// in the event store (event-substrate phase 6). `verify_impact` queries
    /// the wired `RolloutEventSource` for the metric's value at
    /// `before_position` and at the rollout's end — the store answers "what
    /// changed after this action" as a query instead of a struct walk.
    RolloutImpactCheck {
        rollout_id: String,
        before_position: i64,
        metric: String,
    },
    /// No typed regulation data — used for non-regulation actions.
    #[serde(rename = "no_data")]
    #[default]
    NoData,
}

impl RegulationData {
    /// Extract `remaining_ratio` if this variant carries one.
    pub fn remaining_ratio(&self) -> Option<f64> {
        match self {
            RegulationData::EnergyBudgetLow {
                remaining_ratio, ..
            }
            | RegulationData::BudgetGuardEscalation {
                remaining_ratio, ..
            }
            | RegulationData::EnergyDepletionAutoAdjust {
                remaining_ratio, ..
            } => Some(*remaining_ratio),
            _ => None,
        }
    }

    /// The rollout this action's impact should be verified against, and the
    /// event position marking "before the action" (event-substrate phase 6).
    /// `None` for actions that don't target a rollout — those take the
    /// re-sense fallback in `verify_impact`.
    pub fn rollout_target(&self) -> Option<(String, i64)> {
        match self {
            RegulationData::RolloutImpactCheck {
                rollout_id,
                before_position,
                ..
            } => Some((rollout_id.clone(), *before_position)),
            _ => None,
        }
    }

    /// The metric name this action's data concerns (for store queries).
    pub fn metric_name(&self) -> &str {
        match self {
            RegulationData::EnergyBudgetLow { .. }
            | RegulationData::BudgetGuardEscalation { .. }
            | RegulationData::EnergyDepletionAutoAdjust { .. } => "energy_remaining",
            RegulationData::VarietyDeficitExceeded { .. } => "variety_deficit",
            RegulationData::ErrorRateExceeded { .. } => "error_rate",
            RegulationData::ConnectorLatencyExceeded { .. } => "connector_latency",
            RegulationData::CommunicationBackpressure { .. } => "queue_depth",
            RegulationData::ToolReliabilityDegraded { .. } => "tool_reliability",
            RegulationData::ContextServerFleetHealth { .. } => "context_server_health",
            RegulationData::OcrSilentFailuresExceeded { .. } => "ocr_silent_failures",
            RegulationData::CuratorBudgetOverride { .. } => "energy_remaining",
            RegulationData::RolloutImpactCheck { metric, .. } => metric,
            RegulationData::NoData => "no_metric",
        }
    }

    /// Extract `deficit` if this variant carries one.
    pub fn deficit(&self) -> Option<f64> {
        match self {
            RegulationData::VarietyDeficitExceeded { deficit, .. } => Some(*deficit),
            _ => None,
        }
    }

    /// Whether this variant's deviation is the value falling *below* its
    /// threshold (a floor metric), as opposed to rising above it (a ceiling
    /// metric).
    ///
    /// Alert wording must follow the direction or the message lies about
    /// the deviation: tool reliability and energy remaining are floors, and
    /// the previous shared "exceeds" verb read a reliability of 0 against
    /// a 0.80 floor as "value 0 exceeds threshold 80". Only meaningful for
    /// variants that carry a threshold pair (those where
    /// `regulation_policy::extract_deficit_threshold` returns `Some`) —
    /// the rest never reach verb selection.
    pub fn below_threshold_is_bad(&self) -> bool {
        matches!(
            self,
            RegulationData::ToolReliabilityDegraded { .. }
                | RegulationData::EnergyBudgetLow { .. }
                | RegulationData::BudgetGuardEscalation { .. }
                | RegulationData::EnergyDepletionAutoAdjust { .. }
        )
    }

    /// The (metric, before-value) pair `verify_impact` compares a
    /// re-sensed after-value against — the value this variant carried at
    /// escalation time. `None` for variants that carry no before-value
    /// (`NoData` and the meta-regulatory / observational arms);
    /// `verify_impact` warns and skips those.
    ///
    /// This is the per-variant impact table, colocated with the variants
    /// it describes: adding impact verification to a variant is one arm
    /// here, plus a re-sense arm in `verify_impact` only if the metric is
    /// new to it.
    pub fn impact_before_value(&self) -> Option<(SignalMetric, f64)> {
        match self {
            RegulationData::EnergyBudgetLow {
                remaining_ratio, ..
            }
            | RegulationData::BudgetGuardEscalation {
                remaining_ratio, ..
            }
            | RegulationData::EnergyDepletionAutoAdjust {
                remaining_ratio, ..
            } => Some((SignalMetric::EnergyRemaining, *remaining_ratio)),
            RegulationData::VarietyDeficitExceeded { deficit, .. } => {
                Some((SignalMetric::VarietyDeficit, *deficit))
            }
            RegulationData::ContextServerFleetHealth {
                healthy_count,
                total_count,
            } => Some((
                SignalMetric::ContextServerHealth,
                *healthy_count as f64 / (*total_count).max(1) as f64,
            )),
            RegulationData::ToolReliabilityDegraded { reliability, .. } => {
                Some((SignalMetric::ToolReliability, *reliability))
            }
            RegulationData::OcrSilentFailuresExceeded { count, .. } => {
                Some((SignalMetric::OcrSilentFailures, *count))
            }
            _ => None,
        }
    }
}

/// Typed parameters for a loop action.
///
/// Replaces `serde_json::Value` to make the required `reason` field
/// type-safe and compile-time verifiable. Extra structured data is
/// stored in `data` for observation/metrics.
///
/// # Design note: why `reason` is a free-form `String`
///
/// `LoopMetrics::from_cycle` does string matching on `reason` to
/// compute fidelity scores. Making `reason` a typed enum would prevent
/// misspellings but would also require updating the enum every time a
/// new action is added — coupling the type system to runtime heuristics.
/// The current design keeps the heuristic flexible while ensuring the
/// field is always present (no `Option`, no JSON key lookup).
///
/// # Toyota Kata alignment (ADR-056 §6.1)
///
/// The `prediction` field carries the expected metric value after the
/// action. This closes the Kata's prediction gap: `verify_impact()` can
/// compare `after` vs. `prediction` (model validation) in addition to
/// `after` vs. `before` (effectiveness). Without a prediction, the
/// regulator learns whether its actions are effective, but not whether
/// its *model* is correct.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegulatoryActionParams {
    /// Human-readable reason for the action (required for observability).
    pub reason: String,
    /// Typed regulation data (non-regulation actions use `RegulationData::NoData`).
    #[serde(default)]
    pub data: RegulationData,
    /// Expected metric value after the action (Toyota Kata prediction).
    /// When set, `verify_impact()` compares the actual post-action value
    /// against this prediction to validate the regulator's model.
    /// When `None`, only effectiveness (before vs. after) is checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prediction: Option<f64>,
}

impl RegulatoryActionParams {
    /// Create parameters with just a reason (no regulation data, no prediction).
    pub fn reason(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            data: RegulationData::NoData,
            prediction: None,
        }
    }

    /// Create parameters with reason + typed regulation data (no prediction).
    pub fn with_data(reason: impl Into<String>, data: RegulationData) -> Self {
        Self {
            reason: reason.into(),
            data,
            prediction: None,
        }
    }

    /// Create parameters with reason + typed regulation data + prediction.
    ///
    /// The prediction is the expected metric value after the action.
    /// This closes the Toyota Kata prediction gap (ADR-056 §6.1).
    pub fn with_prediction(
        reason: impl Into<String>,
        data: RegulationData,
        prediction: f64,
    ) -> Self {
        Self {
            reason: reason.into(),
            data,
            prediction: Some(prediction),
        }
    }

    /// Set a prediction on existing parameters.
    #[must_use]
    pub fn predicted(mut self, value: f64) -> Self {
        self.prediction = Some(value);
        self
    }
}

impl std::fmt::Display for RegulatoryActionParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.data {
            RegulationData::NoData => write!(f, "{}", self.reason),
            _ => write!(f, "{} {:?}", self.reason, self.data),
        }
    }
}

/// Efferent action produced by a loop's compute phase.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegulatoryAction {
    pub target: LoopId,
    pub action_type: ActionType,
    pub parameters: RegulatoryActionParams,
    /// The signal metric this action targets. Set by `compute()` so
    /// `verify_impact` doesn't need to infer it from JSON key sniffing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_name: Option<String>,
}

impl RegulatoryAction {
    pub fn new(
        target: LoopId,
        action_type: ActionType,
        parameters: RegulatoryActionParams,
    ) -> Self {
        Self {
            target,
            action_type,
            parameters,
            metric_name: None,
        }
    }

    /// Create an action with its target metric set for impact verification.
    pub fn with_metric(
        target: LoopId,
        action_type: ActionType,
        parameters: RegulatoryActionParams,
        metric_name: String,
    ) -> Self {
        Self {
            target,
            action_type,
            parameters,
            metric_name: Some(metric_name),
        }
    }
}

/// Types of regulatory actions a loop can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ActionType {
    /// Reduce resource allocation to a target loop
    Throttle,
    /// Escalate an alert to the Curation loop
    Escalate,
    /// Adjust a threshold or set-point
    Calibrate,
    /// Open a circuit breaker on a target
    CircuitBreak,
    /// Adjust energy budget within set-point bounds (Cybernetics automatic regulation)
    ///
    /// This is a *weaker* capability than `OverrideEnergyBudget`.
    /// Cybernetics can adjust within its set-point range.
    /// Only Curation can override set-points themselves.
    AdjustEnergyBudget,
    /// Override energy budget beyond set-point bounds (Curation metacognitive override)
    ///
    /// This is a *stronger* capability than `AdjustEnergyBudget`.
    /// Only Curation can issue this — it can exceed Cybernetics' set-point range.
    OverrideEnergyBudget,
    /// Replenish an agent's energy budget (Curation directive)
    ///
    /// \[NORMATIVE\] Used when an agent has exhausted its budget but should continue. (P9 — Homeostatic Self-Regulation).
    /// This is the Curator's ability to inject energy into the system.
    ReplenishBudget,
    /// Informational notification — no action required, positive signal.
    /// Used for non-urgent health improvements (e.g., seam coverage increased).
    Notify,
    /// Prune (delete) data to free space.
    /// Used for autonomous disk space management — export pruning, old artifact cleanup.
    /// Pre-authorized by user via P2 Affirmative Consent configuration.
    Prune,
}

impl ActionType {
    /// Stable string representation (not Debug — semantic identity).
    ///
    /// Used for stagnation keys, substitution ladders, and Regulation span metadata.
    /// Must stay in sync with `from_str`.
    pub fn as_str(&self) -> &'static str {
        match self {
            ActionType::Throttle => "Throttle",
            ActionType::Escalate => "Escalate",
            ActionType::Calibrate => "Calibrate",
            ActionType::CircuitBreak => "CircuitBreak",
            ActionType::AdjustEnergyBudget => "AdjustEnergyBudget",
            ActionType::OverrideEnergyBudget => "OverrideEnergyBudget",
            ActionType::ReplenishBudget => "ReplenishBudget",
            ActionType::Notify => "Notify",
            ActionType::Prune => "Prune",
        }
    }

    /// Parse from the same strings produced by `as_str`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Throttle" => Some(ActionType::Throttle),
            "Escalate" => Some(ActionType::Escalate),
            "Calibrate" => Some(ActionType::Calibrate),
            "CircuitBreak" => Some(ActionType::CircuitBreak),
            "AdjustEnergyBudget" => Some(ActionType::AdjustEnergyBudget),
            "OverrideEnergyBudget" => Some(ActionType::OverrideEnergyBudget),
            "ReplenishBudget" => Some(ActionType::ReplenishBudget),
            "Notify" => Some(ActionType::Notify),
            "Prune" => Some(ActionType::Prune),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the per-variant impact table: every variant that carries a
    /// before-value maps to the metric `verify_impact` re-senses, and the
    /// observational arms map to `None` (`verify_impact` warns and skips).
    #[test]
    fn impact_before_value_covers_the_verifiable_variants() {
        let energy = RegulationData::EnergyBudgetLow {
            remaining_ratio: 0.2,
            set_point: 0.3,
        };
        assert_eq!(
            energy.impact_before_value(),
            Some((SignalMetric::EnergyRemaining, 0.2))
        );

        let guard = RegulationData::BudgetGuardEscalation {
            remaining_ratio: 0.1,
            set_point: 0.3,
            projected_minutes: 5,
            options: Vec::new(),
            curator_timeout_secs: 60,
            fallback: "reduce".to_string(),
        };
        assert_eq!(
            guard.impact_before_value(),
            Some((SignalMetric::EnergyRemaining, 0.1))
        );

        let variety = RegulationData::VarietyDeficitExceeded {
            deficit: 42.0,
            threshold: 19.0,
        };
        assert_eq!(
            variety.impact_before_value(),
            Some((SignalMetric::VarietyDeficit, 42.0))
        );

        // Fleet health carries counts, not a ratio — the before-value is
        // the healthy/total ratio at escalation time.
        let fleet = RegulationData::ContextServerFleetHealth {
            healthy_count: 3,
            total_count: 4,
        };
        assert_eq!(
            fleet.impact_before_value(),
            Some((SignalMetric::ContextServerHealth, 0.75))
        );

        let reliability = RegulationData::ToolReliabilityDegraded {
            reliability: 0.0,
            threshold: 0.8,
        };
        assert_eq!(
            reliability.impact_before_value(),
            Some((SignalMetric::ToolReliability, 0.0))
        );

        // OCR silent failures carry the storm count — the before-value the
        // re-sense arm compares against as entries age out of the window.
        let ocr = RegulationData::OcrSilentFailuresExceeded {
            count: 14.0,
            threshold: 0.0,
        };
        assert_eq!(
            ocr.impact_before_value(),
            Some((SignalMetric::OcrSilentFailures, 14.0))
        );

        // No before-value: verify_impact warns and skips these.
        assert_eq!(RegulationData::NoData.impact_before_value(), None);
        assert_eq!(
            RegulationData::CuratorBudgetOverride {
                agent: "curator".to_string(),
                new_budget: 100,
            }
            .impact_before_value(),
            None
        );
    }
}
