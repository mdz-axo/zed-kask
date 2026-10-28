//! Core loop types — identifiers, channels, and quality telemetry.
//!
//! The Loop trait uses async-trait for object safety.

use super::actions::{ActionType, RegulatoryAction};
use super::signals::{Deviation, SignalMetric};
use crate::algedonic::RuntimeAlert;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    /// Model-initiated but human-gated — a skill's PDCA loop reached a stage
    /// that needs a person AND a model that obliges (Fermi `Prompted`).
    /// Distinct from `Manual` (operator-initiated): `Prompted` is
    /// model-initiated, the human gates rather than drives. Example:
    /// `algedonic-review` step 4 (ACT — Execute operator decisions) — the
    /// skill presents the triage, the operator confirms each resolve/dismiss.
    /// The `skill_id` is tracked in the skill execution context, not here
    /// (keeping this enum `Copy` for `LoopMetrics`).
    Prompted,
}

/// Result of an evidence-bearing before/after impact check.
///
/// Fermi pattern: the "impact gate" compares observations on opposite sides
/// of a recorded action boundary. Advisory routing does not create a report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImpactReport {
    /// The action that was verified.
    pub action_type: ActionType,
    /// The metric the action targeted.
    pub metric: SignalMetric,
    /// Metric value before the recorded action boundary.
    pub before: f64,
    /// Metric value after the recorded action boundary.
    pub after: f64,
    /// Absolute change: after − before.
    pub delta: f64,
    /// Did the metric move in the intended direction?
    pub improved: bool,
    /// Classification decision based on the impact magnitude.
    pub decision: ActionDecision,
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
        let improved = match metric.impact_direction() {
            Some(true) => delta > 0.0,
            Some(false) => delta < 0.0,
            None => false,
        };
        Self {
            action_type,
            metric,
            before,
            after,
            delta,
            improved,
            decision,
        }
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
    /// Observation improved or stayed within noise tolerance; not causal proof.
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
    /// Fraction of deviations that produced a typed disposition, bounded to
    /// 0.0–1.0. Both `Notify` and `Escalate` are handled responses.
    pub response_coverage: f64,
    /// How well actions match deviations (0.0–1.0).
    /// 1.0 = every deviation had a corresponding action (or no deviations
    /// detected — trivially matched). 0.0 = deviations detected but none matched.
    /// Computed as: matched_deviations / total_deviations.
    pub fidelity_score: f64,
    /// Fraction of evidence-bearing rollout impact reports that improved.
    pub rollout_progress_score: Option<f64>,
    /// What triggered this tick.
    pub trigger: TriggerOrigin,
}

impl Default for LoopMetrics {
    fn default() -> Self {
        Self {
            delay_ms: 0,
            response_coverage: 1.0,
            fidelity_score: 1.0,
            rollout_progress_score: None,

            trigger: TriggerOrigin::Scheduled,
        }
    }
}

impl LoopMetrics {
    /// Compute loop quality from the cycle's inputs and outputs.
    ///
    /// expect: "Only evidence-bearing rollout checks contribute to impact progress"
    /// \[P9\] Homeostatic Self-Regulation — loop quality enables Regulation self-observation
    /// pre:  elapsed_ms is measured wall-clock time; deviations and actions are from
    ///       the same regulation cycle
    /// post: returns LoopMetrics with response_coverage, fidelity_score, and
    ///       rollout progress computed from evidence-bearing cycle data
    ///
    /// - `elapsed_ms`: wall-clock time from sense start to act end
    /// - `deviations`: deviations detected during compare
    /// - `actions`: actions produced during compute
    /// - `impact_reports`: results from evidence-bearing `verify_impact`
    /// - `trigger`: what triggered this tick
    pub fn from_cycle(
        elapsed_ms: u64,
        deviations: &[Deviation],
        actions: &[RegulatoryAction],
        impact_reports: &[ImpactReport],

        trigger: TriggerOrigin,
    ) -> Self {
        // Response coverage. When no deviations exist, the loop is
        // trivially responsive (it responded to all zero deviations) — 1.0,
        // not 0.0. Reporting 0.0 when healthy makes "broken" and "healthy"
        // indistinguishable to the operator.
        let response_coverage = if deviations.is_empty() {
            1.0
        } else {
            (actions.len() as f64 / deviations.len() as f64).min(1.0)
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

        let rollout_progress_score = if impact_reports.is_empty() {
            None
        } else {
            let improved = impact_reports.iter().filter(|r| r.improved).count() as f64;
            Some(improved / impact_reports.len() as f64)
        };

        Self {
            delay_ms: elapsed_ms,
            response_coverage,
            fidelity_score,
            rollout_progress_score,

            trigger,
        }
    }
}

// ── Trust/absence assembly layer (Fermi LoopView) ──────────────────────────

/// Four-way absence distinction for sense inputs (Fermi `panel_absence` module).
///
/// The `.rules` `unwrap_or(0)` trap exists because this distinction is missing:
/// a DB outage returning 0 is read as "no deviation." This enum makes the
/// distinction a type — `Empty` is a real zero, `Fault` is an error, `Unknown`
/// is a None, and `Idle` means the sensor hasn't ticked yet.
///
/// Fermi's defining discipline: empty is never blank. `idle` / `fault` /
/// `unknown` are distinct; unobserved counters are neither healthy nor broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SenseReading {
    /// No data — counter at 0, but the sensor ticked and returned 0. This is
    /// a real measurement, not an absence.
    Empty,
    /// Wired but not yet ticked — the sensor has never run. Distinct from
    /// `Empty` (which is a real zero) and `Fault` (which is an error).
    Idle,
    /// Tick fired but returned an error — the sensor is broken. Distinct
    /// from `Unknown` (which is a None, not an error).
    Fault,
    /// Tick fired but returned None — the sensor can't tell. Distinct from
    /// `Fault` (which is an error) and `Empty` (which is a real zero).
    Unknown,
}

/// Does the loop's output chain produce? (Fermi `loop_model` module)
///
/// Answers: is the loop emitting actions in response to deviations? A loop
/// that is wired but never ticks is `NotProducing`; a loop that ticks but
/// emits no actions is `Stalled`; a loop that ticks and emits actions is
/// `Producing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopModel {
    /// The chain is producing — ticks fire and actions are emitted.
    Producing,
    /// The chain is stalled — ticks fire but no actions are emitted. The loop
    /// is turning but not producing output.
    Stalled,
    /// The chain is not producing — ticks are not firing. The loop is wired
    /// but not turning.
    NotProducing,
}

/// Does the output carry the signal? (Fermi `outcome_trust` module)
///
/// Answers: when the loop produces actions, do those actions actually move
/// the metric? This is the impact-verification layer — `Trusted` means the
/// action improved the metric (verified via `ImpactReport`), `Untrusted` means
/// the action worsened it, and `Unverified` means no impact report exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeTrust {
    /// Output is trusted — impact verified, action improved the metric.
    Trusted,
    /// Output is untrusted — impact verified, action worsened the metric.
    Untrusted,
    /// Output is unverified — no impact reports, can't tell. Distinct from
    /// `Trusted` (which means verified-and-good) and `Untrusted` (which means
    /// verified-and-bad). `Unverified` means we don't know.
    Unverified,
}

/// Has the writer ever run? (Fermi `liveness_trust` module)
///
/// Answers: has the loop's tick ever fired? `NeverRun` means the loop is wired
/// but has never ticked (the dominant failure mode — a loop that reports
/// success while having never run). `Stale` means the loop has run before but
/// the last tick is outside the expected interval. `Live` means the loop is
/// ticking within the expected interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LivenessTrust {
    /// The writer has run recently — last tick within the expected interval.
    Live,
    /// The writer has run before but is stale — last tick outside the interval.
    Stale,
    /// The writer has never run — no tick history. This is the wiring-closed
    /// state: the loop is wired but has never turned.
    NeverRun,
}

/// The composite reading from the trust/absence assembly layer.
///
/// This is the Fermi `LoopView.reading` — the four modules compose into a
/// single verdict that distinguishes wiring-closed from turning from working.
/// The defining discipline: **wiring-closed ≠ turning ≠ working**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reading {
    /// The loop is turning and producing verified impact. This is the
    /// "working" state — the loop is actually doing its job.
    Turning,
    /// The loop is wired but not turning — ticks are not firing. This is the
    /// dominant failure mode: a loop that reports success while having never
    /// run. Distinct from `Broken` (which means the loop IS turning but not
    /// working).
    WiringClosed,
    /// The loop is turning but not working — actions are not improving
    /// metrics, or the sensor is broken. Distinct from `WiringClosed` (which
    /// means the loop is not turning at all).
    Broken,
    /// The loop is unobserved — not enough data to tell. Distinct from all
    /// other variants: we genuinely cannot determine the state.
    Unobserved,
}

impl std::fmt::Display for Reading {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reading::Turning => write!(f, "turning"),
            Reading::WiringClosed => write!(f, "wiring-closed"),
            Reading::Broken => write!(f, "broken"),
            Reading::Unobserved => write!(f, "unobserved"),
        }
    }
}

/// Trust/absence assembly layer (Fermi `LoopView`).
///
/// Composes four modules into a `Reading`:
/// - `loop_model` — does the chain produce?
/// - `panel_absence` — is empty idle/faulty/unknowable?
/// - `outcome_trust` — does output carry the signal?
/// - `liveness_trust` — has the writer ever run?
///
/// The defining discipline: **wiring-closed ≠ turning ≠ working**. The
/// dominant failure mode is a loop that reports success while having never
/// run — `LoopMetrics` alone cannot distinguish "wired but never ticked"
/// from "ticked but returned no impact reports." `LoopView` makes this
/// distinction by composing liveness (has the writer run?) with loop_model
/// (does the chain produce?) and outcome_trust (does output carry the
/// signal?).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LoopView {
    /// Does the chain produce? (Fermi `loop_model`)
    pub loop_model: LoopModel,
    /// Is empty idle/faulty/unknowable? (Fermi `panel_absence`)
    pub panel_absence: SenseReading,
    /// Does output carry the signal? (Fermi `outcome_trust`)
    pub outcome_trust: OutcomeTrust,
    /// Has the writer ever run? (Fermi `liveness_trust`)
    pub liveness_trust: LivenessTrust,
    /// The composite reading — wiring-closed, turning, broken, or unobserved.
    pub reading: Reading,
}

impl LoopView {
    /// Construct a `LoopView` from the four module readings, computing the
    /// composite `Reading` via [`compute_reading`](Self::compute_reading).
    pub fn new(
        loop_model: LoopModel,
        panel_absence: SenseReading,
        outcome_trust: OutcomeTrust,
        liveness_trust: LivenessTrust,
    ) -> Self {
        let reading =
            Self::compute_reading(loop_model, panel_absence, outcome_trust, liveness_trust);
        Self {
            loop_model,
            panel_absence,
            outcome_trust,
            liveness_trust,
            reading,
        }
    }

    /// Compute the composite `Reading` from the four module readings.
    ///
    /// Priority order (first match wins):
    /// 1. `NeverRun` -> `WiringClosed` — the loop is wired but has never
    ///    ticked. This is the dominant failure mode.
    /// 2. `Fault` -> `Broken` — the sensor is broken, so the loop is turning
    ///    but can't sense.
    /// 3. `Unknown` -> `Unobserved` — we can't read the sensor.
    /// 4. `NotProducing` -> `WiringClosed` — the chain is wired but not
    ///    turning (no ticks firing).
    /// 5. `Untrusted` -> `Broken` — the loop is turning but actions are
    ///    making things worse.
    /// 6. `Trusted` + `Producing` -> `Turning` — the loop is turning and
    ///    working.
    /// 7. Default -> `Unobserved` — not enough data to tell (e.g.,
    ///    `Unverified` outcome, or `Stalled` loop model).
    pub fn compute_reading(
        loop_model: LoopModel,
        panel_absence: SenseReading,
        outcome_trust: OutcomeTrust,
        liveness_trust: LivenessTrust,
    ) -> Reading {
        // Priority 1: If the writer has never run, the loop is wired but not
        // turning. This is the dominant failure mode — a loop that reports
        // success while having never run.
        if liveness_trust == LivenessTrust::NeverRun {
            return Reading::WiringClosed;
        }

        // Priority 2: If the sensor is broken, the loop is turning but can't
        // sense. This is distinct from `WiringClosed` (the loop IS running,
        // it just can't read its inputs).
        if panel_absence == SenseReading::Fault {
            return Reading::Broken;
        }

        // Priority 3: If we can't read the sensor, we can't tell the state.
        if panel_absence == SenseReading::Unknown {
            return Reading::Unobserved;
        }

        // Priority 4: If the chain is not producing, the loop is wired but
        // not turning — ticks are not firing.
        if loop_model == LoopModel::NotProducing {
            return Reading::WiringClosed;
        }

        // Priority 5: If the output is untrusted, the loop is turning but
        // not working — actions are making things worse.
        if outcome_trust == OutcomeTrust::Untrusted {
            return Reading::Broken;
        }

        // Priority 6: If the output is trusted and the chain is producing,
        // the loop is turning and working.
        if outcome_trust == OutcomeTrust::Trusted && loop_model == LoopModel::Producing {
            return Reading::Turning;
        }

        // Default: not enough data to tell. This covers:
        // - `Unverified` outcome (running but no impact reports)
        // - `Stalled` loop model (ticks fire but no actions)
        // - `Idle` panel absence (sensor hasn't ticked yet, but writer has)
        Reading::Unobserved
    }
}

// ── Declared door registry (Fermi STAGE_ACTIONS) ───────────────────────────

/// Registry of declared human doors for `Manual` and `Prompted` regulation
/// stages (Fermi `STAGE_ACTIONS`). Maps `(trigger, stage_name)` to the MCP
/// tool names that serve as the human door for that stage.
///
/// The registry is the declared surface — the skill is the model-coordinated
/// executor. Without this registry, a new `Manual`/`Prompted` stage has no
/// enforced door; the mapping is implicit (in the skill body), not declared.
///
/// Example: `(Prompted, "algedonic_review_act")` maps to
/// `["kanban_task_verify"]` — an authorized reviewer records the
/// observation as verification evidence on the board.
#[derive(Debug, Clone, Default)]
pub struct StageActions {
    doors: std::collections::HashMap<(TriggerOrigin, String), Vec<String>>,
}

impl StageActions {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a door for a `(trigger, stage)` pair. Multiple calls for the
    /// same pair replace the prior registration.
    pub fn register(
        &mut self,
        trigger: TriggerOrigin,
        stage: impl Into<String>,
        tools: Vec<String>,
    ) {
        self.doors.insert((trigger, stage.into()), tools);
    }

    /// Look up the declared doors for a `(trigger, stage)` pair. Returns an
    /// empty slice if no doors are registered.
    pub fn doors(&self, trigger: TriggerOrigin, stage: &str) -> &[String] {
        self.doors
            .get(&(trigger, stage.to_string()))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// List all declared doors as `(trigger, stage, tools)` tuples. Used by
    /// `curator_status` to surface the registry to the operator.
    pub fn all_doors(&self) -> Vec<(TriggerOrigin, String, Vec<String>)> {
        self.doors
            .iter()
            .map(|((t, s), v)| (*t, s.clone(), v.clone()))
            .collect()
    }

    /// Number of declared doors.
    pub fn len(&self) -> usize {
        self.doors.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.doors.is_empty()
    }
}

// ── Inter-loop channel types ───────────────────────────────────────────────

/// Cybernetics sends `Alert` through the `mpsc::Sender<CurationInput>` channel.
///
/// Each pathway gets its own typed `tokio::mpsc` channel. Channel identity
/// replaces the former `LoopId` and `DispatchTarget` routing of the old
/// Communication Loop.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CurationInput {
    /// Algedonic alert from Cybernetics (variety deficit escalation)
    Alert(RuntimeAlert),
}

#[cfg(test)]
mod tests;
