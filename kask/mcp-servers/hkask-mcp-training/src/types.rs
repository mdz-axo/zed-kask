//! Request types for the Training MCP server — all tool input structs and their supporting types.
//!
//! Eight tools: ingest_qa, ingest_dataset, assemble_dataset, submit (handles
//! retrain via optional feedback_path), status, cancel, evaluate,
//! validate_config. Deployment tools removed in favor of `AdapterPort`;
//! register/list/delete adapters removed in favor of `AdapterStore` /
//! `AdapterPort` direct calls; preflight_check replaced by validate_config
//! (runs the actual lora-training skill gates).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::providers::TrainingParams;

// ── Request structs ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QaItem {
    pub question: String,
    pub answer: String,
    #[serde(default)]
    pub bloom_level: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IngestQaRequest {
    /// QA pairs to ingest for training.
    pub qa_items: Vec<QaItem>,
    /// Source document or dataset identifier.
    pub source: String,
    /// Optional training dataset name (default: "default").
    #[serde(default)]
    pub dataset: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrainSubmitRequest {
    /// Path to the training dataset file.
    pub dataset_path: String,
    /// Base model to fine-tune (provider-prefixed, e.g., "OM/qwen3:8b").
    pub base_model: String,
    /// Optional training hyperparameters. Uses defaults if not provided.
    #[serde(default)]
    pub params: Option<TrainingParams>,
    // ── Retrain mode (optional) ──────────────────────────────────────────
    //
    // When `feedback_path` is set, `training_submit` enters retrain mode:
    // it merges `dataset_path` (original) with `feedback_path` (curated
    // feedback), deduplicates by user question, increments the adapter
    // version based on existing adapters with the same `skill_name`, and
    // pre-registers the adapter metadata so `training_status` can complete
    // the A/B comparison on job completion.
    //
    // When `feedback_path` is absent, this is a normal training submit.
    /// Path to a feedback JSONL file to merge into the original dataset
    /// (enables retrain mode).
    #[serde(default)]
    pub feedback_path: Option<String>,
    /// Skill name for the adapter registry (retrain mode only).
    /// When set, enables A/B comparison against prior adapters with the
    /// same skill name on job completion.
    #[serde(default)]
    pub skill_name: Option<String>,
    /// Adapter name for the new version (retrain mode only).
    /// If omitted, a name is derived from the skill_name + version.
    #[serde(default)]
    pub adapter_name: Option<String>,
    /// Path to write the merged dataset (retrain mode only).
    /// Defaults to an auto-generated path in the cache dir.
    #[serde(default)]
    pub merged_output_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrainStatusRequest {
    /// Job ID from a previous `training_submit` call.
    pub job_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrainCancelRequest {
    /// Job ID to cancel.
    pub job_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AssembleDatasetRequest {
    /// Training dataset name to filter by (matches QA pairs ingested with this dataset).
    #[serde(default)]
    pub dataset: Option<String>,
    /// Source identifier to filter by.
    #[serde(default)]
    pub source: Option<String>,
    /// Bloom level to filter by (e.g., "remembering", "applying").
    #[serde(default)]
    pub bloom_level: Option<String>,
    /// Path to write the assembled ChatML JSONL file.
    pub output_path: String,
    /// Fraction of examples to reserve for training (default 1.0 = all train, no test split).
    /// Set to 0.8 for an 80/20 train/test split. Test file is written to {output_path}.test.jsonl.
    #[serde(default)]
    pub train_split: Option<f64>,
    /// Maximum number of examples to include (default: all matching).
    #[serde(default)]
    pub max_examples: Option<usize>,
    /// Optional system prompt to prepend to each assembled conversation.
    /// Sets agent persona/context for fine-tuning (e.g., "You are an hKask agent trained in constraint classification.").
    #[serde(default)]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrainEvaluateRequest {
    /// Adapter ID or fine-tuned model name to evaluate.
    pub adapter_id: String,
    /// Path to test dataset (ChatML JSONL). Each line must have a "messages" array
    /// with user/assistant turns. The last assistant message is the expected answer.
    pub test_dataset_path: String,
    /// Model identifier to run evaluation against (provider-prefixed).
    pub model: String,
    /// Evaluation method: "exact_match" (default), "contains", or "semantic".
    /// - exact_match: generated == expected after trimming
    /// - contains: expected substring is found in generated
    /// - semantic: uses a second inference call to judge correctness
    #[serde(default)]
    pub method: Option<String>,
    /// Maximum number of examples to evaluate (default: all).
    #[serde(default)]
    pub max_examples: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrainIngestDatasetRequest {
    /// Path to the raw dataset file (JSONL, JSON, or TXT).
    pub dataset_path: String,
    /// Optional cache directory override (default: server's configured cache dir).
    #[serde(default)]
    pub cache_dir: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrainValidateConfigRequest {
    /// Training parameters to validate against the lora-training skill's
    /// math-contract gates (G-M1..G-M4, G-Q1, G-Q2, G-Q4, G-Q5).
    pub params: TrainingParams,
    /// Optional dataset path — if provided, runs G-D1 (dataset size vs quality).
    #[serde(default)]
    pub dataset_path: Option<String>,
    /// Optional base model — if provided, runs G-Q5 (paged optimizer heuristic).
    #[serde(default)]
    pub base_model: Option<String>,
}

// ── Supporting types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AbBaseline {
    pub previous_version: u32,
    pub previous_loss: f32,
    pub previous_perplexity: f32,
}
