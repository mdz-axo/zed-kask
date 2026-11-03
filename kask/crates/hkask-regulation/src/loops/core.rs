//! Core loop types — identifiers, channels, and quality telemetry.
//!
//! The Loop trait uses async-trait for object safety.

use super::actions::RegulatoryAction;
use super::signals::Deviation;
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
    /// What triggered this tick.
    pub trigger: TriggerOrigin,
}

impl Default for LoopMetrics {
    fn default() -> Self {
        Self {
            delay_ms: 0,
            response_coverage: 1.0,
            fidelity_score: 1.0,

            trigger: TriggerOrigin::Scheduled,
        }
    }
}

impl LoopMetrics {
    /// Compute loop quality from the cycle's inputs and outputs.
    ///
    /// expect: "Loop quality is computed from the cycle's measured inputs and outputs"
    /// pre:  elapsed_ms is measured wall-clock time; deviations and actions are from
    ///       the same regulation cycle
    /// post: returns LoopMetrics with response_coverage and fidelity_score
    ///       computed from the cycle's deviations and actions
    ///
    /// - `elapsed_ms`: wall-clock time from sense start to act end
    /// - `deviations`: deviations detected during compare
    /// - `actions`: actions produced during compute
    /// - `trigger`: what triggered this tick
    pub fn from_cycle(
        elapsed_ms: u64,
        deviations: &[Deviation],
        actions: &[RegulatoryAction],
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

        Self {
            delay_ms: elapsed_ms,
            response_coverage,
            fidelity_score,

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
/// the metric? This was the impact-verification layer (the rollout-impact
/// pathway, removed 2026-10-31); `Unverified` — no impact evidence — is
/// now the only production reading, which the metacognition status surface
/// reports as "impact unverified". `Trusted`/`Untrusted` remain for
/// decoding persisted LoopView composites.
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
pub enum Reading {
    /// The loop is turning and producing verified impact. This is the
    /// "working" state — the loop is actually doing its job.
    Turning,
    /// The loop is turning and met its latest cycle's demand: no
    /// deviations, or every deviation received a typed disposition (open
    /// conditions are carried by the review board). Impact is unverified —
    /// the only impact producer (the rollout-impact pathway) retired
    /// 2026-10-31 — so this is healthy homeostasis with the caveat stated
    /// rather than read as health. Distinct from `Unobserved` (cannot
    /// tell) and `Broken` (deviations with no disposition).
    Homeostatic,
    /// The loop is wired but not turning — ticks are not firing. This is the
    /// dominant failure mode: a loop that reports success while having never
    /// run. Distinct from `Broken` (which means the loop IS turning but not
    /// working).
    WiringClosed,
    /// The loop is turning but not working — actions are not improving
    /// metrics, the sensor is broken, or deviations exist with no typed
    /// disposition (the unacknowledged-alarm class: sensed, then dropped
    /// with no action). Distinct from `WiringClosed` (which means the loop
    /// is not turning at all).
    Broken,
    /// The loop is unobserved — not enough data to tell. Distinct from all
    /// other variants: we genuinely cannot determine the state.
    Unobserved,
}

impl std::fmt::Display for Reading {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reading::Turning => write!(f, "turning"),
            Reading::Homeostatic => write!(f, "homeostatic"),
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
    /// 4. `Idle` -> `Unobserved` — the sensor hasn't ticked; nothing is
    ///    observed, and claiming homeostasis would fabricate an observation.
    /// 5. `NotProducing` -> `WiringClosed` — the chain is wired but not
    ///    turning (no ticks firing).
    /// 6. `Untrusted` -> `Broken` — the loop is turning but actions are
    ///    making things worse.
    /// 7. `Stalled` -> `Broken` — deviations exist with no typed
    ///    disposition: the unacknowledged-alarm class (sensed, then dropped
    ///    with no action), a live defect rather than "not enough data".
    /// 8. `Trusted` + `Producing` -> `Turning` — the loop is turning and
    ///    working with verified impact.
    /// 9. `Producing` + `Unverified` -> `Homeostatic` — the loop is turning
    ///    and met its latest cycle's demand (no deviations, or all
    ///    dispositioned); impact is unverified, stated as a caveat rather
    ///    than collapsing the state into "not enough data".
    /// 10. Default -> `Unobserved` — genuinely not enough data to tell.
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

        // Priority 4: An unticked sensor observes nothing — homeostasis
        // would be a fabricated observation.
        if panel_absence == SenseReading::Idle {
            return Reading::Unobserved;
        }

        // Priority 5: If the chain is not producing, the loop is wired but
        // not turning — ticks are not firing.
        if loop_model == LoopModel::NotProducing {
            return Reading::WiringClosed;
        }

        // Priority 6: If the output is untrusted, the loop is turning but
        // not working — actions are making things worse.
        if outcome_trust == OutcomeTrust::Untrusted {
            return Reading::Broken;
        }

        // Priority 7: Deviations with no typed disposition — the
        // unacknowledged-alarm class. This used to fall to the default
        // `Unobserved` ("not enough data"), reading a live defect as an
        // absence (observed live through 2026-11-02: the chronic
        // `deviations: 1, advisories_computed: 0` signature read
        // `unobserved` for weeks while a below-threshold native-first rate
        // was silently dropped by the policy).
        if loop_model == LoopModel::Stalled {
            return Reading::Broken;
        }

        // Priority 8: If the output is trusted and the chain is producing,
        // the loop is turning and working.
        if outcome_trust == OutcomeTrust::Trusted && loop_model == LoopModel::Producing {
            return Reading::Turning;
        }

        // Priority 9: The chain met its cycle's demand with impact
        // unverified — healthy homeostasis, the ordinary quiet session.
        // `Unverified` is the only production outcome reading since the
        // rollout-impact pathway retired (2026-10-31); before this branch
        // every such session read `Unobserved` indefinitely.
        if loop_model == LoopModel::Producing {
            return Reading::Homeostatic;
        }

        // Default: not enough data to tell.
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
