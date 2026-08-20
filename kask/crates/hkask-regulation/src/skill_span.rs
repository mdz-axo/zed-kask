//! Skill feedback Regulation spans — the unified cybernetic feedback channel.
//!
//! Every skill emits exactly six semantic spans under `reg.skill.<skill-id>.*`,
//! one per PDCA phase. These spans are the single regulated feedback channel
//! from skills to the Curator and the Regulation nervous system. Fine-grained
//! execution telemetry uses `hkask.template.<skill-id>.*` (performative,
//! unregulated) instead.
//!
//! The six spans map to the cybernetic loop:
//!   Sense  → Classify, Gather
//!   Act    → Draft, Write
//!   Check  → Evaluate, Convergence
//!
//! Registration: `reg.skill` is registered in `CANONICAL_NAMESPACES`. The
//! hierarchical `is_canonical` function makes `reg.skill.<any-skill-id>.*`
//! valid without per-skill registration.
//!
//! Pattern: thin typed enum mirroring InfraSpan / QaSpan.
//! Reference: P9 §9.1 — Regulation span coverage.

use hkask_types::ObservableSpan;

/// The semantic skill-feedback spans. Every skill emits these under its
/// own `reg.skill.<skill-id>.<phase>` namespace. The `<skill-id>` segment is
/// provided at emission time (from the manifest's `id` field), not encoded
/// in the variant — so this enum is shared across all skills.
///
/// The first six spans map to the cybernetic loop PDCA phases and are emitted
/// by every skill invocation. The two outcome spans are emitted
/// asynchronously, after the skill's output has been used, when outcome
/// evidence or operator feedback becomes available. They close the feedback
/// loop for self-improvement (extrinsic exploratory experience, τ_t).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkillFeedbackSpan {
    /// After step 1 (classify) — the skill has determined what kind of
    /// artifact to produce. Carries: domain, conservation_mode,
    /// ontology_anchor, candidate_node_count.
    Classify,
    /// After step 2 (gather) — the skill has gathered (or asked for) the
    /// inputs needed to produce the artifact. Carries: path (A/B),
    /// question_count, delegation_target, spec_gap.
    Gather,
    /// After step 3 (draft) — the skill has produced a draft artifact.
    /// Carries: node_count, edge_count, conservation_discrepancies.
    Draft,
    /// After step 4 (evaluate) — the skill has scored the draft against
    /// quality criteria. Carries: weighted_total, data_integrity_score,
    /// fabrication_detected.
    Evaluate,
    /// After step 5 (convergence) — the skill has computed whether the
    /// PDCA loop has converged. Carries: convergence_metric, converged,
    /// iterations_remaining, fabrication_escalation.
    Convergence,
    /// After step 6 (write) — the skill has committed the final artifact.
    /// Carries: output_path, bytes_written.
    Write,
    /// Outcome evidence — emitted asynchronously after the skill's output
    /// has been used in the real world. For lora-training: training
    /// completed/failed, OOM occurred, convergence issues, eval results.
    /// For task-breakdown: tasks completed/reworked/blocked, plan followed
    /// or abandoned. Carries: outcome_kind, outcome_data, skill_output_ref,
    /// latency_since_skill_output. This is the extrinsic exploratory
    /// experience signal (τ_t) for self-improvement.
    Outcome,
    /// Operator feedback — emitted when the operator explicitly reacts to
    /// the skill's output (accepted, overridden, rejected, or post-hoc
    /// correction). Carries: disposition, override_reason, corrected_field,
    /// skill_output_ref. This is the intrinsic evaluative feedback signal
    /// (e_t) for self-improvement, sourced from the operator rather than
    /// the skill itself.
    OperatorFeedback,
}

impl SkillFeedbackSpan {
    /// The phase suffix for this span — e.g. "classify", "gather".
    /// Combined with a skill-id via `namespace(skill_id)`, this produces
    /// the full `reg.skill.<skill-id>.<phase>` string.
    pub fn phase(&self) -> &'static str {
        match self {
            Self::Classify => "classify",
            Self::Gather => "gather",
            Self::Draft => "draft",
            Self::Evaluate => "evaluate",
            Self::Convergence => "convergence",
            Self::Write => "write",
            Self::Outcome => "outcome",
            Self::OperatorFeedback => "operator_feedback",
        }
    }

    /// Construct the full namespace string for this span under a given skill.
    /// E.g. `SkillFeedbackSpan::Classify.namespace("sankey-flow")` →
    /// `"reg.skill.sankey-flow.classify"`.
    pub fn namespace(&self, skill_id: &str) -> String {
        format!("reg.skill.{}.{}", skill_id, self.phase())
    }
}

impl std::fmt::Display for SkillFeedbackSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "reg.skill.<skill-id>.{}", self.phase())
    }
}

impl ObservableSpan for SkillFeedbackSpan {
    /// Returns the base namespace `reg.skill`. The full namespace is
    /// `reg.skill.<skill-id>.<phase>` — use `namespace(skill_id)` for the
    /// concrete emission string. This impl returns the registered ancestor
    /// so `SpanNamespace::from_observable()` validates against `reg.skill`.
    fn as_str(&self) -> &'static str {
        "reg.skill"
    }
}
