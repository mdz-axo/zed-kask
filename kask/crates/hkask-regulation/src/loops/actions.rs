//! Typed central regulation dispositions and evidence payloads.

use super::core::LoopId;

/// Typed regulation data — replaces the previous `serde_json::Value` pass-through.
///
/// Each variant corresponds to a regulation reason. The `#[serde(tag = "reason")]`
/// encoding ensures serialized JSON is self-describing and backward-compatible
/// with consumers that inspect the `reason` field.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum RegulationData {
    /// Variety deficit exceeded threshold.
    VarietyDeficitExceeded { deficit: f64, threshold: f64 },

    // Wallet and SeamCoverage data variants removed 2026-08-30 with their
    // policy rules — residuals of the deleted wallet module (219c74b180)
    // and a never-built seam watcher.
    /// Tool reliability degraded below threshold.
    ToolReliabilityDegraded { reliability: f64, threshold: f64 },
    /// Context-server fleet health degraded — some registered servers are
    /// stuck in `Starting` or `Error` instead of `Running`.
    ///
    /// Carries the fleet snapshot at escalation time as reviewable trigger
    /// evidence. `healthy_count` / `total_count` come from
    /// `ContextServerHealthSource` when the advisory is built.
    ContextServerFleetHealth {
        healthy_count: u64,
        total_count: u64,
    },
    /// OCR silent failures exceeded the set-point (0) — the corpus OCR
    /// endpoint returned empty output on page(s) within the recent window.
    ///
    /// Carries the count at escalation time as quantitative trigger evidence
    /// for the advisory. Subsequent sensing may show that the condition
    /// recovered, but recovery alone does not establish that advice worked.
    OcrSilentFailuresExceeded { count: f64, threshold: f64 },

    /// No typed regulation data — used for non-regulation actions.
    #[serde(rename = "no_data")]
    #[default]
    NoData,
}

impl RegulationData {
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
        matches!(self, RegulationData::ToolReliabilityDegraded { .. })
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegulatoryActionParams {
    /// Human-readable reason for the action (required for observability).
    pub reason: String,
    /// Typed regulation data (non-regulation actions use `RegulationData::NoData`).
    #[serde(default)]
    pub data: RegulationData,
}

impl RegulatoryActionParams {
    /// Create parameters with just a reason (no regulation data).
    pub fn reason(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            data: RegulationData::NoData,
        }
    }

    /// Create parameters with reason + typed regulation data.
    pub fn with_data(reason: impl Into<String>, data: RegulationData) -> Self {
        Self {
            reason: reason.into(),
            data,
        }
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

/// Central disposition produced by a loop's compute phase.
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

/// Truthful dispositions the central regulation loop can execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ActionType {
    /// Route an evidence-bearing condition to Curation/human review.
    Escalate,
    /// Record an informational observation without intervention.
    Notify,
}

impl ActionType {
    /// Stable string representation (not Debug — semantic identity).
    ///
    /// Used for stagnation keys, substitution ladders, and Regulation span metadata.
    /// Must stay in sync with `from_str`.
    pub fn as_str(&self) -> &'static str {
        match self {
            ActionType::Escalate => "Escalate",
            ActionType::Notify => "Notify",
        }
    }
}
