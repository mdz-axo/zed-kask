//! Core loop types — identifiers, the Loop trait, and quality telemetry.
//!
//! The Loop trait uses async-trait for object safety.

use super::actions::{ActionType, RegulatoryAction};
use super::signals::{Deviation, SignalMetric};

/// Loop identifiers for the 4-loop model.
///
/// VSM correspondence:
/// - Loop 1:  Inference    (S1 Implementation)
/// - Loop 2:  Memory       (S2 Coordination — unified memory store)
/// - Loop 5:  Curation     (S4 Intelligence — meta-observer)
/// - Loop 6:  Cybernetics  (S3 Control — homeostatic regulation)
///
/// No Loop 3: Control absorbed into Cybernetics (intentional).
/// No Loop 4: VSM S4 = Curation (Loop 5).
/// StorageGuard and McpServerGuard loops were folded into Cybernetics.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum LoopId {
    Inference,
    Memory,
    Curation,
    Cybernetics,
}

impl std::fmt::Display for LoopId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoopId::Inference => write!(f, "inference"),
            LoopId::Memory => write!(f, "memory"),
            LoopId::Curation => write!(f, "curation"),
            LoopId::Cybernetics => write!(f, "cybernetics"),
        }
    }
}

/// What triggered this regulation cycle.
///
/// Adapted from Fermi's `TriggerReason` pattern — recording provenance
/// enables Regulation to correlate trigger type with regulatory effectiveness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerOrigin {
    /// Regular scheduled tick (timer-driven).
    Scheduled,
    /// Triggered by an incoming algedonic alert.
    AlertDriven,
    /// Manually invoked via operator directive.
    Manual,
    /// Triggered by an external event (regulation record, goal transition, etc.).
    EventDriven,
}

/// Result of verifying whether a regulatory action improved its target metric.
///
/// Fermi pattern: the "impact gate" — after acting, re-sense the targeted
/// metric and compare against the pre-action value. This closes the cybernetic
/// feedback loop: sense → compare → compute → act → **verify**.
///
/// # Toyota Kata alignment (ADR-056 §6.1)
///
/// When the action carried a `prediction` (expected post-action value),
/// `prediction_error` measures the gap between predicted and actual.
/// This validates the regulator's *model*, not just its *effectiveness*.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImpactReport {
    /// The action that was verified.
    pub action_type: ActionType,
    /// The metric the action targeted.
    pub metric: SignalMetric,
    /// Metric value before the action was applied.
    pub before: f64,
    /// Metric value after the action was applied (re-sensed).
    pub after: f64,
    /// Absolute change: after − before.
    pub delta: f64,
    /// Did the metric move in the intended direction?
    pub improved: bool,
    /// Classification decision based on the impact magnitude.
    pub decision: ActionDecision,
    /// Expected metric value after the action (Toyota Kata prediction).
    /// `None` if the action carried no prediction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prediction: Option<f64>,
    /// Absolute error between prediction and actual post-action value.
    /// `None` if no prediction was made. Small error = model is correct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prediction_error: Option<f64>,
}

impl ImpactReport {
    /// Construct an ImpactReport, computing `improved` from the metric semantics.
    ///
    /// expect: "The system closes the cybernetic feedback loop by measuring action impact"
    /// \[P9\] Homeostatic Self-Regulation — impact verification closes the regulation cycle
    /// pre:  metric is a valid SignalMetric; before and after are sane numeric values
    /// post: returns ImpactReport with delta=after-before, improved computed per metric semantics
    ///
    /// `decision` should be computed via `RegulationRule::classify()` by the caller.
    pub fn new(
        action_type: ActionType,
        metric: SignalMetric,
        before: f64,
        after: f64,
        decision: ActionDecision,
    ) -> Self {
        let delta = after - before;
        let improved = match metric {
            SignalMetric::EnergyRemaining => delta > 0.0,
            SignalMetric::VarietyDeficit => delta < 0.0,
            _ => delta.abs() > f64::EPSILON,
        };
        Self {
            action_type,
            metric,
            before,
            after,
            delta,
            improved,
            decision,
            prediction: None,
            prediction_error: None,
        }
    }

    /// Construct an ImpactReport with a prediction (Toyota Kata alignment).
    ///
    /// When the action carried a predicted post-action value, this constructor
    /// computes `prediction_error` = |after - prediction|. Small error means
    /// the regulator's model is correct; large error means the model needs
    /// revision (Conant-Ashby: the regulator must model the system).
    pub fn with_prediction(
        action_type: ActionType,
        metric: SignalMetric,
        before: f64,
        after: f64,
        decision: ActionDecision,
        prediction: f64,
    ) -> Self {
        let mut report = Self::new(action_type, metric, before, after, decision);
        report.prediction = Some(prediction);
        report.prediction_error = Some((after - prediction).abs());
        report
    }
}

/// Three-tier decision gate for verified actions (Fermi impact-gate pattern).
///
/// After re-sensing the target metric post-action, classify the outcome:
/// - **Accept** — action improved the metric or worsened within noise tolerance.
/// - **Stage** — action was moderately ineffective; escalate as Warning for review.
/// - **Block** — action was severely counterproductive; prevent re-use for this metric.
///
/// Thresholds are per-metric configurable via SetPoints. Defaults:
/// - Stage threshold: 5% relative worsening.
/// - Block threshold: 20% relative worsening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionDecision {
    /// Action was effective or within noise tolerance. Continue.
    Accept,
    /// Action was moderately ineffective — worth reviewing. Escalate as Warning.
    Stage,
    /// Action was severely counterproductive — prevent re-use. Escalate as Critical.
    Block,
}

/// Loop-quality telemetry — measures the loop's own performance.
///
/// These metrics are about the loop itself, not the signals it processes.
/// They enable Regulation observability of loop health: is the loop responding
/// quickly enough? Is it producing appropriate actions for detected deviations?
/// Are those actions actually effective?
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct LoopMetrics {
    /// Milliseconds between sense start and act completion (loop latency).
    pub delay_ms: u64,
    /// Ratio of actions produced to deviations detected (responsiveness).
    /// 1.0 = every deviation produced an action (or no deviations detected —
    /// trivially responsive). 0.0 = deviations detected but no actions produced.
    pub gain: f64,
    /// How well actions match deviations (0.0–1.0).
    /// 1.0 = every deviation had a corresponding action (or no deviations
    /// detected — trivially matched). 0.0 = deviations detected but none matched.
    /// Computed as: matched_deviations / total_deviations.
    pub fidelity_score: f64,
    /// Ratio of actions that actually improved their target metric (0.0–1.0).
    ///
    /// Fermi impact-gate pattern: 1.0 = every verified action moved its
    /// metric toward the set-point. 0.0 = either no verification ran (no
    /// impact reports) or no action had measurable impact. An operator seeing
    /// 0.0 must check whether verification was skipped (no data) or actions
    /// genuinely failed — the score does not conflate "unverified" with "success."
    pub effectiveness_score: f64,
    /// What triggered this tick.
    pub trigger: TriggerOrigin,
}

impl Default for LoopMetrics {
    fn default() -> Self {
        Self {
            delay_ms: 0,
            gain: 1.0,
            fidelity_score: 1.0,
            effectiveness_score: 0.0,
            trigger: TriggerOrigin::Scheduled,
        }
    }
}

impl LoopMetrics {
    /// Compute loop quality from the cycle's inputs and outputs.
    ///
    /// expect: "The system measures its own regulatory effectiveness"
    /// \[P9\] Homeostatic Self-Regulation — loop quality enables Regulation self-observation
    /// pre:  elapsed_ms is measured wall-clock time; deviations and actions are from
    ///       the same regulation cycle
    /// post: returns LoopMetrics with gain, fidelity_score, and
    ///       effectiveness_score computed from cycle data
    ///
    /// - `elapsed_ms`: wall-clock time from sense start to act end
    /// - `deviations`: deviations detected during compare
    /// - `actions`: actions produced during compute
    /// - `impact_reports`: results from `verify_impact` (empty → effectiveness = 0.0,
    ///   signaling "unverified" — not "all actions effective")
    /// - `trigger`: what triggered this tick
    pub fn from_cycle(
        elapsed_ms: u64,
        deviations: &[Deviation],
        actions: &[RegulatoryAction],
        impact_reports: &[ImpactReport],
        trigger: TriggerOrigin,
    ) -> Self {
        // Gain: responsiveness. When no deviations exist, the loop is
        // trivially responsive (it responded to all zero deviations) — 1.0,
        // not 0.0. Reporting 0.0 when healthy makes "broken" and "healthy"
        // indistinguishable to the operator.
        let gain = if deviations.is_empty() {
            1.0
        } else {
            actions.len() as f64 / deviations.len() as f64
        };

        // Fidelity: count how many deviations had a matching action by metric_name.
        let matched = deviations
            .iter()
            .filter(|d| {
                let metric_str = d.signal.metric.as_str();
                actions
                    .iter()
                    .any(|a| a.metric_name.as_deref() == Some(metric_str))
            })
            .count() as f64;
        let fidelity_score = if deviations.is_empty() {
            1.0
        } else {
            matched / deviations.len() as f64
        };
        // All matches use metric_name directly.

        // Effectiveness: percentage of verified actions that were Accepted
        // (i.e., either improved or within noise tolerance). Staged/Blocked
        // actions reduce the score. When no impact reports exist, no
        // verification ran — report 0.0 ("unverified"), NOT 1.0 ("all
        // effective"). Reporting 1.0 when unverified conflates "no data" with
        // "success" — the operator cannot distinguish a working loop from one
        // that never checks its own impact.
        let effectiveness_score = if impact_reports.is_empty() {
            0.0
        } else {
            let accepted = impact_reports
                .iter()
                .filter(|r| r.decision == ActionDecision::Accept)
                .count() as f64;
            accepted / impact_reports.len() as f64
        };

        Self {
            delay_ms: elapsed_ms,
            gain,
            fidelity_score,
            effectiveness_score,
            trigger,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::actions::RegulatoryActionParams;
    use super::super::signals::Signal;
    use super::*;

    /// Pins F1 + F2 + F3: when no deviations and no impact reports exist
    /// (the healthy steady-state), gain=1.0 (trivially responsive),
    /// fidelity=1.0 (trivially matched), and effectiveness=0.0 (unverified —
    /// NOT 1.0, which would conflate "no data" with "all effective").
    ///
    /// Before the fix, all three reported 0.0 / 0.0 / 1.0 — the operator
    /// could not distinguish "loop broken" (gain=0) from "system healthy"
    /// (gain=0), nor "all actions effective" (effectiveness=1) from "no
    /// verification ran" (effectiveness=1).
    #[test]
    fn from_cycle_healthy_reports_trivially_correct_metrics() {
        let metrics = LoopMetrics::from_cycle(
            0,
            &[], // no deviations — healthy
            &[], // no actions
            &[], // no impact reports — unverified
            TriggerOrigin::Scheduled,
        );
        assert_eq!(
            metrics.gain, 1.0,
            "gain=1.0 when healthy (trivially responsive)"
        );
        assert_eq!(
            metrics.fidelity_score, 1.0,
            "fidelity=1.0 when healthy (trivially matched)"
        );
        assert_eq!(
            metrics.effectiveness_score, 0.0,
            "effectiveness=0.0 when unverified (not 1.0)"
        );
    }

    /// Pins F1: gain = actions / deviations when deviations exist. Two
    /// deviations, one action → gain = 0.5.
    #[test]
    fn from_cycle_gain_is_actions_over_deviations() {
        let signal_a = Signal::new(LoopId::Cybernetics, SignalMetric::EnergyRemaining, 0.1, 0.2);
        let signal_b = Signal::new(
            LoopId::Cybernetics,
            SignalMetric::VarietyDeficit,
            200.0,
            100.0,
        );
        let deviations = [
            Deviation::from_signal(&signal_a).unwrap(),
            Deviation::from_signal(&signal_b).unwrap(),
        ];
        let action = RegulatoryAction::with_metric(
            LoopId::Inference,
            ActionType::Throttle,
            RegulatoryActionParams::reason("energy_budget_low"),
            "energy_remaining".into(),
        );
        let metrics =
            LoopMetrics::from_cycle(0, &deviations, &[action], &[], TriggerOrigin::Scheduled);
        assert_eq!(metrics.gain, 0.5, "1 action / 2 deviations = 0.5");
        assert_eq!(
            metrics.fidelity_score, 0.5,
            "1 matched / 2 deviations = 0.5"
        );
        assert_eq!(
            metrics.effectiveness_score, 0.0,
            "no impact reports → unverified → 0.0"
        );
    }

    /// Pins F3: effectiveness = accepted / total when impact reports exist.
    /// Two reports, one Accept, one Block → effectiveness = 0.5.
    #[test]
    fn from_cycle_effectiveness_is_accepted_over_verified() {
        let report_accept = ImpactReport::new(
            ActionType::Throttle,
            SignalMetric::EnergyRemaining,
            0.1,
            0.3, // improved (delta > 0 for EnergyRemaining)
            ActionDecision::Accept,
        );
        let report_block = ImpactReport::new(
            ActionType::CircuitBreak,
            SignalMetric::ErrorRate,
            0.3,
            0.5, // worsened
            ActionDecision::Block,
        );
        let metrics = LoopMetrics::from_cycle(
            0,
            &[],
            &[],
            &[report_accept, report_block],
            TriggerOrigin::Scheduled,
        );
        assert_eq!(
            metrics.effectiveness_score, 0.5,
            "1 accepted / 2 verified = 0.5"
        );
        // gain and fidelity are 1.0 because no deviations (healthy state).
        assert_eq!(metrics.gain, 1.0);
        assert_eq!(metrics.fidelity_score, 1.0);
    }
}
