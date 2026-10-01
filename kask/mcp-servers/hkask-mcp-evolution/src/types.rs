//! Domain types for the evolution experiment registry (repair plan §P8).

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
/// with its parent for lineage (§P8.1).
#[derive(Debug, Clone, Serialize)]
pub struct VariantRecord {
    pub id: String,
    pub experiment_id: String,
    pub genotype_config: serde_json::Value,
    pub parent_variant_id: Option<String>,
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

/// A selection fossil — selected and rejected both retained (§P8.7-Q4).
#[derive(Debug, Clone, Serialize)]
pub struct SelectionRecord {
    pub id: String,
    pub experiment_id: String,
    pub verdict: String,
    pub selected_variant_id: Option<String>,
    pub reject_reasons: Vec<String>,
    pub algedonic_reference: Option<String>,
    pub created_at: String,
}

/// Per-experiment health row for the regulation snapshot (§P8.9 step 1) —
/// the Layer-A afferent view the bridge feeds to the cybernetics loop's
/// `EvolutionHealthSensor`.
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
}

/// The registry's health snapshot: every experiment with its recorded-run
/// total, prediction confidence, and verdict.
#[derive(Debug, Clone, Serialize)]
pub struct EvolutionHealthSnapshot {
    pub experiments: Vec<ExperimentHealth>,
}

impl EvolutionHealthSnapshot {
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
    /// (outcome 1), rejected → refuted (outcome 0). Void experiments — the
    /// first reject reason carries a void marker, "no-headroom" (design
    /// failure: the eval set cannot discriminate) or "measurement-void"
    /// (infrastructure failure: the runs never produced a valid
    /// measurement) — never tested their claim, so they are excluded from
    /// the calibration record rather than scored as refuted. Returns
    /// `(mean_brier, claim_count)`; `None` when no measured claims exist.
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
                    if experiment
                        .reject_reasons
                        .first()
                        .is_some_and(|reason| is_void_marker(reason))
                    {
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

/// Void markers for the calibration record: a reject reason carrying one
/// of these marks an experiment whose pre-registered claim was never
/// tested — "no-headroom" (design failure: baseline saturated, nothing to
/// measure) or "measurement-void" (infrastructure failure: the runs never
/// produced a valid measurement). Such claims are excluded from
/// [`EvolutionHealthSnapshot::resolved_claim_brier`] rather than scored as
/// refuted, so provider or harness noise cannot pollute the calibration
/// record the agenda generator calibrates against.
fn is_void_marker(reason: &str) -> bool {
    reason.contains("no-headroom") || reason.contains("measurement-void")
}

/// Registry failures, classified per-variant for MCP dispatch — never a
/// blanket internal (project rule).
#[derive(Debug, thiserror::Error)]
pub enum EvolutionError {
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
