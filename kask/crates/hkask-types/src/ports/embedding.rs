use serde::{Deserialize, Serialize};

/// A single reranked document's score — the provider's native relevance
/// judgment, not a parsed LLM generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RerankScoreEntry {
    /// Index of the document in the original input list.
    pub index: usize,
    /// Relevance score of the document to the query (provider-scaled,
    /// typically 0.0-1.0).
    pub relevance_score: f64,
}

/// A typed score question for `InferenceMethod::Decision` — the Jev-class
/// "Score" primitive: an instruction plus an ordered array of level
/// descriptions (least to most), answered with a probability distribution
/// over the levels plus a confidence. The question id is never sent to the
/// model (the provider's contract — all meaning lives in the instructions
/// and the level descriptions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionQuestion {
    /// Caller-side question key — the answer returns under this key.
    pub id: String,
    /// The question text, including any criteria context the levels need.
    pub instructions: String,
    /// Ordered level descriptions, least to most (the score scale).
    pub criteria: Vec<String>,
}

/// One level's probability from a decision answer — the parsed form of the
/// provider's string-keyed `probabilities` map (`{"0": 0.79, ...}`),
/// decoded once at the zed side (the one decode home).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionLevelProbability {
    /// The level's index into the question's `criteria`.
    pub level: u8,
    /// The calibrated probability the model assigns to that level.
    pub probability: f64,
}

/// A typed answer to one `DecisionQuestion` — the provider's native
/// judgment (a probability distribution over the caller's own levels),
/// not a parsed LLM generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionAnswer {
    /// The question key the answer belongs to.
    pub id: String,
    /// The probability-weighted score over the levels (the provider's
    /// `score` field — e.g. 1.21 for mostly-level-1 with some level-2).
    pub score: f64,
    /// The probability distribution over the question's criteria levels.
    pub level_probabilities: Vec<DecisionLevelProbability>,
    /// The concentration of the distribution (0.0-1.0) — how unambiguous
    /// the model's judgment is (not its correctness).
    pub confidence: f64,
}

/// One embedding batch plus its request and provider-reported model identities.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingBatch {
    pub vectors: Vec<Vec<f32>>,
    /// Exact model identity supplied by the caller.
    pub requested_model: String,
    /// Exact model identity returned by the provider, when present.
    pub actual_model: Option<String>,
    /// Token usage the provider reported for this embedding call.
    /// `reported: false` means the provider omitted the usage wire field —
    /// the counts are placeholders, nothing was measured (the same
    /// honesty contract as `InferenceResult::usage`).
    pub usage: crate::ports::InferenceUsage,
}

/// Rerank scores plus the call's measured usage — the rerank twin of
/// [`EmbeddingBatch`]. `reported: false` on `usage` means the provider
/// omitted the usage wire field.
#[derive(Debug, Clone, PartialEq)]
pub struct RerankBatch {
    pub scores: Vec<RerankScoreEntry>,
    pub usage: crate::ports::InferenceUsage,
}

/// Decision answers plus the served model and the call's measured usage —
/// the decision twin of [`RerankBatch`]. `model` is the exact snapshot the
/// provider served (provenance for the caller's result records); the
/// Decisions API reports usage on the wire path.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionBatch {
    pub answers: Vec<DecisionAnswer>,
    pub model: String,
    pub usage: crate::ports::InferenceUsage,
}

/// Errors from embedding generation backends (OpenAI, local models, etc.).
#[derive(Debug, Clone, thiserror::Error)]
pub enum EmbeddingGenerationError {
    #[error("Invalid embedding request: {0}")]
    InvalidRequest(String),
    #[error("Connection error: {0}")]
    Connection(String),
    /// No embedding backend is configured — the model string has no
    /// recognized provider prefix, or the provider's credential env var is
    /// unset. A configuration failure to fix, never a transient outage to
    /// retry: downstream classifiers map this to `permission_denied`
    /// (the typed twin of `InferenceError::NotConfigured`).
    #[error("Not configured: {0}")]
    NotConfigured(String),
    #[error("API error: status {0}: {1}")]
    Api(u16, String),
    #[error("JSON parse error: {0}")]
    Json(String),
    #[error("Empty response from embedding model")]
    EmptyResponse,
    #[error("Dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
}
