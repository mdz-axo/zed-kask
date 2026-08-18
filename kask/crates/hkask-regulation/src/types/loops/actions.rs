//! Loop action types — efferent actions and their type classification.

use super::core::LoopId;

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
    /// Wallet balance ratio low.
    WalletBalanceLow {
        balance_ratio: f64,
        severity: String,
        threshold: f64,
    },
    /// Wallet key unhealthy (expired or exhausted).
    WalletKeyUnhealthy { severity: String, threshold: f64 },
    /// Public seam coverage degraded.
    SeamCoverageDegraded {
        coverage_pct: f64,
        previous_coverage: f64,
        drop_magnitude: f64,
        severity: String,
    },
    /// Public seam coverage improved (positive signal).
    SeamCoverageImproved {
        coverage_pct: f64,
        previous_coverage: f64,
        improvement: f64,
    },
    /// Tool reliability degraded below threshold.
    ToolReliabilityDegraded { reliability: f64, threshold: f64 },
    /// Grounding clean rate degraded below the configured floor. Carries the
    /// measured clean_rate and the floor that was violated. The coverage_rate
    /// is not carried — the operator queries `curator_grounding_coverage` for
    /// the coverage gap (a separate signal with its own alert).
    GroundingCleanRateDegraded { clean_rate: f64, floor: f64 },
    /// Grounding coverage rate degraded below the configured floor. Carries
    /// the measured coverage_rate and the floor that was violated.
    GroundingCoverageDegraded { coverage_rate: f64, floor: f64 },
    /// Grounding violation delta is positive — new nulled fields appeared
    /// since the last sense tick. Carries the delta (new nulled count since
    /// last tick). The absolute counts are available via
    /// `curator_grounding_violations`.
    GroundingViolationDeltaIncreased { delta: i64 },
    /// Curator (metacognition) budget override directed at a named agent.
    ///
    /// Carries the LLM-produced target agent name and new budget so `act()`
    /// can issue a `CuratorDirective::OverrideEnergyBudget` without losing the
    /// values (previously the action carried only a reason string and the
    /// budget/target were silently dropped).
    CuratorBudgetOverride { agent: String, new_budget: u64 },
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

    /// Extract `deficit` if this variant carries one.
    pub fn deficit(&self) -> Option<f64> {
        match self {
            RegulationData::VarietyDeficitExceeded { deficit, .. } => Some(*deficit),
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
