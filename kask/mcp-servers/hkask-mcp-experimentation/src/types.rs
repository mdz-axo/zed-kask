//! Domain types for the experiment registry (repair plan §P8).

use serde::{Deserialize, Serialize};

/// The six artifact layers (§P8.3) — the shard map's genotype media.
pub const LAYERS: &[&str] = &[
    "skill",
    "agent_card",
    "prompt_template",
    "tool_schema",
    "regulation_scalar",
    "lora_adapter",
];

/// Experiment lifecycle (§P8.4): proposed at registration, running once the
/// first variant registers, resolved by the first selection. One selection
/// resolves an experiment; a further selection is a failed_precondition
/// conflict, never a silent overwrite.
pub const STATUS_PROPOSED: &str = "proposed";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_RESOLVED: &str = "resolved";

pub const STATUSES: &[&str] = &[STATUS_PROPOSED, STATUS_RUNNING, STATUS_RESOLVED];

pub const VERDICT_SELECTED: &str = "selected";
pub const VERDICT_REJECTED: &str = "rejected";

/// Canonical stage roles within an experiment (Lu et al. 2024 Stage 2 +
/// EvoSkills `experiment-pipeline`).
pub const ROLE_BASELINE: &str = "baseline";
pub const ROLE_TUNING: &str = "tuning";
pub const ROLE_CANDIDATE: &str = "candidate";
pub const ROLE_ABLATION: &str = "ablation";

pub const VARIANT_ROLES: &[&str] = &[ROLE_BASELINE, ROLE_TUNING, ROLE_CANDIDATE, ROLE_ABLATION];

/// Canonical rejection taxonomy (EvoSkills IVE + zed-kask §P8.9 void markers).
///
/// Separates design/infrastructure voids and local implementation defects from
/// genuine hypothesis refutations so Brier calibration and Ideation Memory are
/// never polluted by untried claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    /// Design void: baseline saturated (`overall_pass_rate == 1.0`), so the
    /// eval set cannot discriminate. Excluded from Brier calibration.
    NoHeadroom,
    /// Infrastructure void: provider/harness outage produced zero valid
    /// measurement rollouts. Excluded from Brier calibration.
    MeasurementVoid,
    /// Implementation defect (EvoSkills IVE): the variant failed due to a
    /// local code, syntax, or setup bug rather than a test of the underlying
    /// hypothesis. Excluded from Brier refutation scoring; routes to bounded
    /// repair rather than poisoning the research direction.
    ImplementationDefect,
    /// Fundamental direction failure (EvoSkills IVE / Popper refutation): the
    /// mechanism executed cleanly and its pre-registered prediction was
    /// contradicted by the measured oracle. Scored as outcome 0.0 in Brier.
    HypothesisRefuted,
    /// Ablation failure: the candidate beat baseline, but its ablation variant
    /// achieved equal or better fitness within `noise_band` (the added
    /// component failed Ousterhout's deletion test). Scored as outcome 0.0.
    AblationUnjustified,
}

impl FailureClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoHeadroom => "no_headroom",
            Self::MeasurementVoid => "measurement_void",
            Self::ImplementationDefect => "implementation_defect",
            Self::HypothesisRefuted => "hypothesis_refuted",
            Self::AblationUnjustified => "ablation_unjustified",
        }
    }

    /// Parse a caller-supplied or stored failure class string, accepting both
    /// `snake_case` and `kebab-case` forms.
    pub fn parse(value: &str) -> Result<Self, ExperimentationError> {
        match value.trim() {
            "no_headroom" | "no-headroom" => Ok(Self::NoHeadroom),
            "measurement_void" | "measurement-void" => Ok(Self::MeasurementVoid),
            "implementation_defect" | "implementation-defect" => Ok(Self::ImplementationDefect),
            "hypothesis_refuted" | "hypothesis-refuted" => Ok(Self::HypothesisRefuted),
            "ablation_unjustified" | "ablation-unjustified" => Ok(Self::AblationUnjustified),
            other => Err(ExperimentationError::UnknownFailureClass(other.to_string())),
        }
    }

    /// Infer the failure class from `reject_reasons` when migrating pre-IVE
    /// rows or when a caller omits `failure_class` on a rejected verdict.
    pub fn infer_from_reject_reasons(reasons: &[String]) -> Self {
        let Some(first) = reasons.first() else {
            return Self::HypothesisRefuted;
        };
        let lower = first.to_ascii_lowercase();
        if lower.contains("no-headroom") || lower.contains("no_headroom") {
            Self::NoHeadroom
        } else if lower.contains("measurement-void") || lower.contains("measurement_void") {
            Self::MeasurementVoid
        } else if lower.contains("implementation-defect") || lower.contains("implementation_defect")
        {
            Self::ImplementationDefect
        } else if lower.contains("ablation-unjustified") || lower.contains("ablation_unjustified") {
            Self::AblationUnjustified
        } else {
            Self::HypothesisRefuted
        }
    }

    /// Whether this failure class means the pre-registered hypothesis claim
    /// was never tested and must be excluded from Brier calibration.
    pub const fn excludes_from_brier(self) -> bool {
        matches!(
            self,
            Self::NoHeadroom | Self::MeasurementVoid | Self::ImplementationDefect
        )
    }
}

/// A pre-registered prediction — the tiny controller (§P8.1). The claim is
/// scored against the experiment's measured outcome by the linked kanban
/// goal; the confidence is the experiment's own probability for the claim.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Prediction {
    /// The falsifiable claim the experiment pre-registers (e.g. "variant B
    /// passes the fixed task set at a higher rate than the baseline").
    pub claim: String,
    /// The experiment's own confidence in the claim, in [0, 1].
    pub confidence: f64,
}

/// A registered experiment (§P8.4 `experiment_propose`).
#[derive(Debug, Clone, Serialize)]
pub struct ExperimentRecord {
    pub id: String,
    pub hypothesis: String,
    pub layer: String,
    pub genotype_refs: Vec<String>,
    pub eval_set: serde_json::Value,
    pub fitness_fn: String,
    pub noise_band: serde_json::Value,
    pub prediction: Prediction,
    pub budget: serde_json::Value,
    /// The machine-readable run ceiling parsed from `budget.max_runs` at
    /// declaration (§P8.9 step 2, D-1 enforce) — the Layer-A set point the
    /// store enforces at `fitness_record`. NULL only on pre-rule records
    /// (all resolved; the branch is unreachable for new experiments).
    pub max_runs: Option<u64>,
    /// The linked kanban goal (§P8.9 step 5) — the goal loop Brier-scores
    /// the pre-registered claim against the measured outcome; this field is
    /// the explicit registry ↔ goal-loop join.
    pub linked_goal_id: Option<String>,
    pub status: String,
    pub created_at: String,
}

/// A registered variant — one genotype configuration inside an experiment,
/// with its parent for lineage (§P8.1) and stage role (`baseline | tuning |
/// candidate | ablation`).
#[derive(Debug, Clone, Serialize)]
pub struct VariantRecord {
    pub id: String,
    pub experiment_id: String,
    pub genotype_config: serde_json::Value,
    pub parent_variant_id: Option<String>,
    pub role: String,
    pub ablated_component: Option<String>,
    pub created_at: String,
}

/// A grounded fitness record — recorded report refs only, never simulated
/// (§P8.3).
#[derive(Debug, Clone, Serialize)]
pub struct FitnessRecord {
    pub id: String,
    pub experiment_id: String,
    pub variant_id: String,
    pub runs: Vec<String>,
    /// The number of rollouts the referenced reports cover (§P8.9 step 2) —
    /// a report reference names a harness run whose rollout count the
    /// caller states; the store sums these against `max_runs`.
    pub run_count: u64,
    pub scores: serde_json::Value,
    pub created_at: String,
}

/// A selection fossil — selected and rejected both retained (§P8.7-Q4), with
/// typed IVE failure attribution on rejected verdicts.
#[derive(Debug, Clone, Serialize)]
pub struct SelectionRecord {
    pub id: String,
    pub experiment_id: String,
    pub verdict: String,
    pub selected_variant_id: Option<String>,
    pub reject_reasons: Vec<String>,
    pub failure_class: Option<FailureClass>,
    pub algedonic_reference: Option<String>,
    pub created_at: String,
}

/// One objective specification for [`ParetoFrontierResult`] (NSGA-II
/// non-dominated sorting with relative noise-band tie handling).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ObjectiveSpec {
    /// Metric key inside `fitness_records.scores` (e.g. `"pass_rate"`,
    /// `"total_tokens"`, `"mean_latency_ms"`).
    pub metric: String,
    /// Optimization direction: `"maximize"` or `"minimize"`.
    pub direction: String,
    /// Optional relative noise band in `[0.0, 1.0)` within which differences
    /// count as ties (e.g. `0.10` for a 10% cost band).
    #[serde(default)]
    pub relative_noise_band: Option<f64>,
}

/// A variant on the non-dominated Pareto frontier, with its extracted
/// objective scores and optional NSGA-II crowding distance.
#[derive(Debug, Clone, Serialize)]
pub struct ParetoFrontierMember {
    pub variant_id: String,
    pub role: String,
    pub scores: serde_json::Map<String, serde_json::Value>,
    pub crowding_distance: Option<f64>,
}

/// A dominated variant and the first frontier/pool variant that dominated it.
#[derive(Debug, Clone, Serialize)]
pub struct DominatedVariant {
    pub variant_id: String,
    pub dominated_by: String,
}

/// Deterministic multi-objective Pareto frontier computation result (`F14`,
/// replacing inline Lisp `SI-2`).
#[derive(Debug, Clone, Serialize)]
pub struct ParetoFrontierResult {
    pub experiment_id: String,
    pub evaluated_variants: usize,
    pub frontier: Vec<ParetoFrontierMember>,
    pub frontier_variant_ids: Vec<String>,
    pub pruned_by_crowding: Vec<String>,
    pub dominated: Vec<DominatedVariant>,
    pub new_members_count: usize,
    pub converged: bool,
}

/// Per-experiment health row for the regulation snapshot (§P8.9 step 1) —
/// the Layer-A afferent view the bridge feeds to the cybernetics loop's
/// `ExperimentationHealthSensor`.
#[derive(Debug, Clone, Serialize)]
pub struct ExperimentHealth {
    pub id: String,
    pub layer: String,
    pub status: String,
    pub created_at: String,
    pub max_runs: Option<u64>,
    pub recorded_runs: u64,
    pub prediction_confidence: Option<f64>,
    pub verdict: Option<String>,
    pub reject_reasons: Vec<String>,
    #[serde(default)]
    pub failure_class: Option<FailureClass>,
}

/// The registry's health snapshot: every experiment with its recorded-run
/// total, prediction confidence, and verdict.
#[derive(Debug, Clone, Serialize)]
pub struct ExperimentationHealthSnapshot {
    pub experiments: Vec<ExperimentHealth>,
}

impl ExperimentationHealthSnapshot {
    /// Ids of running experiments that are stuck (§P8.9 step 1, D-3):
    /// unresolved past `stale_days`, or with their whole declared budget
    /// recorded but no verdict yet. An unparseable timestamp reads as
    /// stuck — visible, never silent.
    pub fn stuck_running(&self, stale_days: u64) -> Vec<String> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(stale_days as i64);
        self.experiments
            .iter()
            .filter(|experiment| experiment.status == STATUS_RUNNING)
            .filter(|experiment| {
                let stale = chrono::DateTime::parse_from_rfc3339(&experiment.created_at)
                    .map(|stamp| stamp.with_timezone(&chrono::Utc) < cutoff)
                    .unwrap_or(true);
                let spent = experiment
                    .max_runs
                    .is_some_and(|ceiling| experiment.recorded_runs >= ceiling);
                stale || spent
            })
            .map(|experiment| experiment.id.clone())
            .collect()
    }

    /// Mean Brier over resolved non-void claims: selected → the claim held
    /// (outcome 1), rejected → refuted (outcome 0). Untested claims — where
    /// [`FailureClass::excludes_from_brier`] is true (`NoHeadroom`,
    /// `MeasurementVoid`, or `ImplementationDefect`) — never tested the
    /// underlying hypothesis, so they are excluded from the calibration
    /// record rather than scored as refuted. Returns `(mean_brier, claim_count)`;
    /// `None` when no measured claims exist.
    pub fn resolved_claim_brier(&self) -> Option<(f64, u32)> {
        let mut sum = 0.0;
        let mut claims = 0u32;
        for experiment in &self.experiments {
            if experiment.status != STATUS_RESOLVED {
                continue;
            }
            let Some(confidence) = experiment.prediction_confidence else {
                continue;
            };
            match experiment.verdict.as_deref() {
                Some(VERDICT_SELECTED) => {
                    sum += (confidence - 1.0) * (confidence - 1.0);
                    claims += 1;
                }
                Some(VERDICT_REJECTED) => {
                    let class = experiment.failure_class.unwrap_or_else(|| {
                        FailureClass::infer_from_reject_reasons(&experiment.reject_reasons)
                    });
                    if class.excludes_from_brier() {
                        continue;
                    }
                    sum += confidence * confidence;
                    claims += 1;
                }
                _ => {}
            }
        }
        if claims == 0 {
            None
        } else {
            Some((sum / claims as f64, claims))
        }
    }
}

/// Registry failures, classified per-variant for MCP dispatch — never a
/// blanket internal (project rule).
#[derive(Debug, thiserror::Error)]
pub enum ExperimentationError {
    #[error("experiment {0} not found")]
    ExperimentNotFound(String),
    #[error("variant {0} not found")]
    VariantNotFound(String),
    #[error(
        "experiment {0} is already resolved — it no longer accepts variants, fitness, or selection"
    )]
    ExperimentResolved(String),
    #[error("variant {0} does not belong to experiment {1}")]
    VariantNotInExperiment(String, String),
    #[error(
        "layer must be one of skill, agent_card, prompt_template, tool_schema, regulation_scalar, lora_adapter; got {0:?}"
    )]
    UnknownLayer(String),
    #[error("variant role must be one of baseline, tuning, candidate, ablation; got {0:?}")]
    UnknownVariantRole(String),
    #[error("an ablation variant (role = \"ablation\") must name its parent_variant_id")]
    AblationRequiresParent,
    #[error("an ablation variant (role = \"ablation\") must name its ablated_component")]
    AblationRequiresComponent,
    #[error("ablated_component is only valid when role is \"ablation\", got role {0:?}")]
    AblatedComponentOnNonAblation(String),
    #[error(
        "experiment {0} requires ablation evidence (budget.require_ablation = true): selected variant {1} has no recorded fitness on an ablation child variant (role = \"ablation\", parent_variant_id = {1})"
    )]
    MissingAblationEvidence(String, String),
    #[error(
        "failure_class must be one of no_headroom, measurement_void, implementation_defect, hypothesis_refuted, ablation_unjustified; got {0:?}"
    )]
    UnknownFailureClass(String),
    #[error("failure_class is only valid when verdict is \"rejected\"")]
    SelectedWithFailureClass,
    #[error("objective direction must be \"maximize\" or \"minimize\", got {0:?}")]
    UnknownObjectiveDirection(String),
    #[error("relative_noise_band must be in [0.0, 1.0), got {0}")]
    NoiseBandOutOfRange(f64),
    #[error("prediction confidence must be in [0, 1], got {0}")]
    ConfidenceOutOfRange(f64),
    #[error(
        "budget.max_runs must be a positive integer — the declared budget is the Layer-A set point the server enforces (§P8.9 step 2, D-1); budget was: {0}"
    )]
    BudgetMissingMaxRuns(String),
    #[error(
        "experiment {0} budget exhausted: recording {1} total runs would exceed the declared ceiling of {2} (§P8.9 step 2, D-1)"
    )]
    BudgetExhausted(String, u64, u64),
    #[error(
        "run_count must be a positive integer — the number of rollouts the referenced reports cover (§P8.9 step 2); got {0}"
    )]
    RunCountInvalid(u64),
    #[error("verdict must be \"selected\" or \"rejected\", got {0:?}")]
    UnknownVerdict(String),
    #[error("status must be one of proposed, running, resolved; got {0:?}")]
    UnknownStatus(String),
    #[error("a selected experiment must name its selected_variant_id")]
    SelectedWithoutVariant,
    #[error("a rejected experiment must record at least one reject_reason")]
    RejectedWithoutReasons,
    #[error("{0} must not be empty")]
    Empty(&'static str),
    #[error("created_since must be an RFC 3339 timestamp, got {0:?}")]
    BadTimestamp(String),
    #[error("database error: {0}")]
    Database(#[from] hkask_storage::database::types::DbError),
    #[error("serialization error: {0}")]
    Serialization(String),
}
