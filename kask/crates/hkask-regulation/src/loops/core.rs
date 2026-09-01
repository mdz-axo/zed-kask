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
/// `["curator_escalation_resolve", "curator_escalation_dismiss"]` — the
/// `algedonic-review` skill's step 4 (ACT) is a `Prompted` stage, and the
/// MCP tools that serve as its human door are `curator_escalation_resolve`
/// and `curator_escalation_dismiss`.
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

// ── Nine failure distinctions (Fermi) ───────────────────────────────────────

/// Fermi's nine failure distinctions — the ways a loop can look fine and
/// not be fine. Each distinction has an enforcement point in zed-kask.
///
/// The defining discipline: **wiring-closed ≠ turning ≠ working**. A loop
/// that is wired (every hop has a call site) is not necessarily turning
/// (the loop has moved on real data), and a loop that is turning is not
/// necessarily working (it has been observed to succeed, not merely
/// reached).
///
/// Source: Fermi `docs/architecture/FEEDBACK_LOOPS.md` §"Nine ways a loop
/// can look fine and not be fine."
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LoopFailureDistinctions {
    /// 1. **Declared ≠ dispatched.** A tool/skill is declared on a card or
    /// prompt but has no dispatch arm. Enforcement: `DIVERGENCE.md` D-seam
    /// + pinning tests (e.g. `test_skill_tool_returns_content`).
    pub declared_not_dispatched: DistinctionState,

    /// 2. **Written ≠ readable.** A loop writes to one store but the
    /// display surface reads from another. Enforcement: `memory_insert`
    /// requires `evidence_h_mem_id`; `curator_memory_recall` reads back
    /// from the same store.
    pub written_not_readable: DistinctionState,

    /// 3. **Closed ≠ turning.** Every hop has a call site but the loop has
    /// never moved on real data. Enforcement: `LoopView.reading ==
    /// WiringClosed` when `LivenessTrust == NeverRun`.
    pub closed_not_turning: DistinctionState,

    /// 4. **Reachable ≠ reached.** A gate is wired but never satisfied —
    /// a permission denial is the system working correctly, producing no
    /// error. Enforcement: the curator memory-edit gate was removed
    /// (operator decision, 2026-09-01) after the live finding that models
    /// cannot emit calls to tools absent from their visible list — its
    /// no-effect state was silent and indistinguishable from a thread
    /// kill. The remaining filter layers (profile allowlists, server
    /// scope) are surfaced by the D44 hidden-tools marker +
    /// `list_mcp_tools`, so an unreached tool is named, not silent.
    pub reachable_not_reached: DistinctionState,

    /// 5. **Called ≠ succeeded.** A hop is called on every cycle but has
    /// never once worked — non-fatal failure paths hide the bug.
    /// Enforcement: `ToolReliabilitySensor` aggregates success rates; a
    /// domain with 0% success triggers a `ToolReliabilityDegraded` signal.
    pub called_not_succeeded: DistinctionState,

    /// 6. **One dependency, two resolutions.** Two code paths answer the
    /// same question independently; only the one you test is correct.
    /// Enforcement: `resolve_db_passphrase` is the canonical 2-tier chain
    /// (ctx.credentials → resolve_credential); all MCP servers use it.
    pub one_dependency_two_resolutions: DistinctionState,

    /// 7. **Gated by data, invoked by a constant.** A gate reads from data
    /// but the invoker hardcodes the value — undetectable while the data
    /// equals the constant. Enforcement: `CURATOR_AGENT_ID` is a constant,
    /// but curator identity flows from data —
    /// `NativeAgent::new_session` sets `KaskThreadState.agent_id` from
    /// the agent variant, and the memory port routes on that field.
    pub gated_by_data_invoked_by_constant: DistinctionState,

    /// 8. **Deferred-work comment.** A comment asserts another component
    /// will finish the job — treat as an untested assertion. Enforcement:
    /// the `.rules` trap-avoidance map + `check-zed-isolation.sh` CI gate.
    pub deferred_work_comment: DistinctionState,

    /// 9. **Phantom tool.** A tool is declared on a card/skill but has no
    /// dispatch arm — the model calls it and gets "Unknown tool: X."
    /// Enforcement: `verify_tool_advertisement` checks prompt against
    /// server `TOOL_NAMES`; the D44 visibility marker + `list_mcp_tools`
    /// meta-tool keep the registered surface legible so a filtered tool is
    /// never mistaken for a nonexistent one (the LazyToolRouter that once
    /// filtered MCP tools per turn was removed, D44 2026-08-30).
    pub phantom_tool: DistinctionState,
}

/// The state of a failure distinction — whether it's enforced, a gap, or
/// unverified. Maps to Fermi's "enforced | gap | unverified" status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistinctionState {
    /// The distinction is enforced — there is a test, gate, or type that
    /// catches this failure mode.
    Enforced,
    /// The distinction is a gap — the failure mode is possible but not
    /// caught. This is the highest-value output: a gap is a finding.
    Gap,
    /// The distinction is unverified — we cannot tell whether it's
    /// enforced or a gap. Distinct from `Enforced` (which means verified)
    /// and `Gap` (which means verified-not-enforced).
    Unverified,
}

impl LoopFailureDistinctions {
    /// The current zed-kask enforcement map. Each distinction is mapped to
    /// its enforcement point. `Gap` means the failure mode is possible but
    /// not caught; `Unverified` means we cannot tell.
    ///
    /// This is a static snapshot of the enforcement state — it does not
    /// change at runtime. It exists so `curator_status` can surface the
    /// nine distinctions to the operator, making the enforcement surface
    /// visible rather than implicit.
    pub fn current() -> Self {
        Self {
            // 1. D-seam + pinning tests (DIVERGENCE.md D1-D36)
            declared_not_dispatched: DistinctionState::Enforced,
            // 2. memory_insert requires evidence_h_mem_id; recall reads
            //    from the same store
            written_not_readable: DistinctionState::Enforced,
            // 3. LoopView.reading == WiringClosed when NeverRun
            closed_not_turning: DistinctionState::Enforced,
            // 4. The curator memory-edit gate was removed (2026-09-01);
            //    the remaining filter layers are surfaced by the D44
            //    marker + list_mcp_tools, so an unreached tool is named,
            //    not silent.
            reachable_not_reached: DistinctionState::Enforced,
            // 5. ToolReliabilitySensor aggregates success rates; 0%
            //    success triggers ToolReliabilityDegraded
            called_not_succeeded: DistinctionState::Enforced,
            // 6. resolve_db_passphrase is the canonical 2-tier chain;
            //    all MCP servers use it (per .rules)
            one_dependency_two_resolutions: DistinctionState::Enforced,
            // 7. CURATOR_AGENT_ID is a constant, but curator identity
            //    flows from data: new_session sets KaskThreadState.agent_id
            //    from the agent variant, and the memory port routes on
            //    that field. The constant-vs-data risk exists but is
            //    mitigated by the agent variant being the source.
            gated_by_data_invoked_by_constant: DistinctionState::Enforced,
            // 8. .rules trap-avoidance map + CI gates catch deferred-work
            //    comments that become traps. The distinction is enforced
            //    at the process level, not at the code level.
            deferred_work_comment: DistinctionState::Enforced,
            // 9. verify_tool_advertisement checks prompt against server
            //    TOOL_NAMES; the D44 marker + list_mcp_tools meta-tool keep
            //    the registered surface legible (the LazyToolRouter was
            //    removed, D44 2026-08-30 — no per-turn MCP filtering).
            phantom_tool: DistinctionState::Enforced,
        }
    }

    /// Count how many distinctions are enforced.
    pub fn enforced_count(&self) -> usize {
        let all = [
            self.declared_not_dispatched,
            self.written_not_readable,
            self.closed_not_turning,
            self.reachable_not_reached,
            self.called_not_succeeded,
            self.one_dependency_two_resolutions,
            self.gated_by_data_invoked_by_constant,
            self.deferred_work_comment,
            self.phantom_tool,
        ];
        all.iter()
            .filter(|d| **d == DistinctionState::Enforced)
            .count()
    }

    /// Count how many distinctions are gaps.
    pub fn gap_count(&self) -> usize {
        let all = [
            self.declared_not_dispatched,
            self.written_not_readable,
            self.closed_not_turning,
            self.reachable_not_reached,
            self.called_not_succeeded,
            self.one_dependency_two_resolutions,
            self.gated_by_data_invoked_by_constant,
            self.deferred_work_comment,
            self.phantom_tool,
        ];
        all.iter().filter(|d| **d == DistinctionState::Gap).count()
    }

    /// Total distinctions (always 9).
    pub fn total(&self) -> usize {
        9
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

    // ── TriggerOrigin::Prompted tests (F1) ─────────────────────────────────

    /// Pin F1: `TriggerOrigin::Prompted` is distinct from `Manual`. The
    /// `Prompted` variant represents a model-initiated-but-human-gated stage
    /// (e.g. `algedonic-review` step 4), while `Manual` is operator-initiated.
    #[test]
    fn triggered_origin_prompted_is_distinct_from_manual() {
        assert_ne!(TriggerOrigin::Prompted, TriggerOrigin::Manual);
        assert_ne!(TriggerOrigin::Prompted, TriggerOrigin::Scheduled);
        assert_ne!(TriggerOrigin::Prompted, TriggerOrigin::AlertDriven);
        assert_ne!(TriggerOrigin::Prompted, TriggerOrigin::EventDriven);
    }

    /// Pin F1: `LoopMetrics::from_cycle` accepts `Prompted` as a trigger and
    /// records it. The trigger is carried through to the `LoopMetrics.trigger`
    /// field so downstream consumers can distinguish `Prompted` cycles from
    /// `Manual` cycles.
    #[test]
    fn prompted_triggers_tracked_separately_from_manual() {
        let metrics_prompted = LoopMetrics::from_cycle(0, &[], &[], &[], TriggerOrigin::Prompted);
        assert_eq!(metrics_prompted.trigger, TriggerOrigin::Prompted);

        let metrics_manual = LoopMetrics::from_cycle(0, &[], &[], &[], TriggerOrigin::Manual);
        assert_eq!(metrics_manual.trigger, TriggerOrigin::Manual);
        assert_ne!(
            metrics_prompted.trigger, metrics_manual.trigger,
            "Prompted and Manual must be distinguishable in LoopMetrics"
        );
    }

    // ── LoopView tests (F2 + F5) ───────────────────────────────────────────

    /// Pin F5: `SenseReading` has four distinct variants. Empty is never
    /// blank — idle, fault, and unknown are distinct states, not a single
    /// "no data" bucket.
    #[test]
    fn sense_reading_distinguishes_four_states() {
        assert_ne!(SenseReading::Empty, SenseReading::Idle);
        assert_ne!(SenseReading::Empty, SenseReading::Fault);
        assert_ne!(SenseReading::Empty, SenseReading::Unknown);
        assert_ne!(SenseReading::Idle, SenseReading::Fault);
        assert_ne!(SenseReading::Idle, SenseReading::Unknown);
        assert_ne!(SenseReading::Fault, SenseReading::Unknown);
    }

    /// Pin F2: `LoopView` composes four modules into a `Reading`. The
    /// composition logic must distinguish wiring-closed (never ran) from
    /// turning (running and verified) from broken (running but failing)
    /// from unobserved (can't tell).
    #[test]
    fn loop_view_composes_four_modules_into_reading() {
        // NeverRun -> WiringClosed (dominant failure mode: wired but never ticked)
        let view = LoopView::new(
            LoopModel::Producing,
            SenseReading::Empty,
            OutcomeTrust::Trusted,
            LivenessTrust::NeverRun,
        );
        assert_eq!(
            view.reading,
            Reading::WiringClosed,
            "NeverRun -> WiringClosed regardless of other modules"
        );

        // Live + Fault -> Broken (sensor is broken, loop is turning but can't sense)
        let view = LoopView::new(
            LoopModel::Producing,
            SenseReading::Fault,
            OutcomeTrust::Trusted,
            LivenessTrust::Live,
        );
        assert_eq!(
            view.reading,
            Reading::Broken,
            "Fault sensor -> Broken (turning but can't sense)"
        );

        // Live + Unknown -> Unobserved (can't read the sensor)
        let view = LoopView::new(
            LoopModel::Producing,
            SenseReading::Unknown,
            OutcomeTrust::Trusted,
            LivenessTrust::Live,
        );
        assert_eq!(
            view.reading,
            Reading::Unobserved,
            "Unknown sensor -> Unobserved"
        );

        // Live + Empty + NotProducing -> WiringClosed (chain not producing)
        let view = LoopView::new(
            LoopModel::NotProducing,
            SenseReading::Empty,
            OutcomeTrust::Unverified,
            LivenessTrust::Live,
        );
        assert_eq!(
            view.reading,
            Reading::WiringClosed,
            "NotProducing -> WiringClosed (wired but not turning)"
        );

        // Live + Empty + Producing + Untrusted -> Broken (actions making things worse)
        let view = LoopView::new(
            LoopModel::Producing,
            SenseReading::Empty,
            OutcomeTrust::Untrusted,
            LivenessTrust::Live,
        );
        assert_eq!(
            view.reading,
            Reading::Broken,
            "Untrusted -> Broken (turning but not working)"
        );

        // Live + Empty + Producing + Trusted -> Turning (working!)
        let view = LoopView::new(
            LoopModel::Producing,
            SenseReading::Empty,
            OutcomeTrust::Trusted,
            LivenessTrust::Live,
        );
        assert_eq!(
            view.reading,
            Reading::Turning,
            "Trusted + Producing + Live -> Turning"
        );

        // Live + Empty + Stalled + Unverified -> Unobserved (can't tell)
        let view = LoopView::new(
            LoopModel::Stalled,
            SenseReading::Empty,
            OutcomeTrust::Unverified,
            LivenessTrust::Live,
        );
        assert_eq!(
            view.reading,
            Reading::Unobserved,
            "Stalled + Unverified -> Unobserved (not enough data)"
        );

        // Stale + Empty + Producing + Trusted -> Turning (was running, still verified)
        let view = LoopView::new(
            LoopModel::Producing,
            SenseReading::Empty,
            OutcomeTrust::Trusted,
            LivenessTrust::Stale,
        );
        assert_eq!(
            view.reading,
            Reading::Turning,
            "Stale + Trusted + Producing -> Turning (stale but still verified)"
        );

        // Live + Idle + Producing + Unverified -> Unobserved (sensor hasn't ticked)
        let view = LoopView::new(
            LoopModel::Producing,
            SenseReading::Idle,
            OutcomeTrust::Unverified,
            LivenessTrust::Live,
        );
        assert_eq!(
            view.reading,
            Reading::Unobserved,
            "Idle sensor + Unverified -> Unobserved"
        );
    }

    /// Pin F2: `Reading::Display` produces a human-readable string for each
    /// variant. The Fermi `LoopView` produces "a Reading and a sentence."
    #[test]
    fn reading_display_provides_human_readable_string() {
        assert_eq!(Reading::Turning.to_string(), "turning");
        assert_eq!(Reading::WiringClosed.to_string(), "wiring-closed");
        assert_eq!(Reading::Broken.to_string(), "broken");
        assert_eq!(Reading::Unobserved.to_string(), "unobserved");
    }

    // ── StageActions tests (F4) ─────────────────────────────────────────────

    /// Pin F4: `StageActions` registry maps `Prompted` stages to MCP tool
    /// names. The registry is the declared surface — without it, a new
    /// `Prompted` stage has no enforced door.
    #[test]
    fn stage_actions_registry_maps_prompted_stages_to_mcp_tools() {
        let mut actions = StageActions::new();
        assert!(actions.is_empty());

        // Register the algedonic-review ACT stage as a Prompted door.
        actions.register(
            TriggerOrigin::Prompted,
            "algedonic_review_act",
            vec![
                "curator_escalation_resolve".to_string(),
                "curator_escalation_dismiss".to_string(),
            ],
        );

        assert_eq!(actions.len(), 1);
        assert!(!actions.is_empty());

        // Look up the doors for the Prompted stage.
        let doors = actions.doors(TriggerOrigin::Prompted, "algedonic_review_act");
        assert_eq!(doors.len(), 2);
        assert!(doors.contains(&"curator_escalation_resolve".to_string()));
        assert!(doors.contains(&"curator_escalation_dismiss".to_string()));

        // A Manual trigger for the same stage name returns no doors —
        // Prompted and Manual are distinct trigger types.
        let manual_doors = actions.doors(TriggerOrigin::Manual, "algedonic_review_act");
        assert_eq!(
            manual_doors.len(),
            0,
            "Manual trigger has no doors for a Prompted stage"
        );

        // all_doors returns the full registry.
        let all = actions.all_doors();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, TriggerOrigin::Prompted);
        assert_eq!(all[0].1, "algedonic_review_act");
        assert_eq!(all[0].2.len(), 2);
    }

    // ── LoopFailureDistinctions tests (F3) ──────────────────────────────────

    /// Pin F3: `LoopFailureDistinctions::current()` returns all nine
    /// distinctions, each mapped to its enforcement point. The total is
    /// always 9 — Fermi's nine failure distinctions.
    #[test]
    fn failure_distinctions_current_returns_nine() {
        let d = LoopFailureDistinctions::current();
        assert_eq!(d.total(), 9, "Fermi defines nine failure distinctions");
        assert_eq!(
            d.enforced_count() + d.gap_count(),
            d.total(),
            "enforced + gap must cover all distinctions (unverified not used in current)"
        );
    }

    /// Pin F3: each distinction has a `DistinctionState` — `Enforced`,
    /// `Gap`, or `Unverified`. The state is not a boolean; `Unverified` is
    /// distinct from `Enforced` (verified) and `Gap` (verified-not-enforced).
    #[test]
    fn failure_distinctions_state_is_three_way() {
        assert_ne!(DistinctionState::Enforced, DistinctionState::Gap);
        assert_ne!(DistinctionState::Enforced, DistinctionState::Unverified);
        assert_ne!(DistinctionState::Gap, DistinctionState::Unverified);
    }
}
