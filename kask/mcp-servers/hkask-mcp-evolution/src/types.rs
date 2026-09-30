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
