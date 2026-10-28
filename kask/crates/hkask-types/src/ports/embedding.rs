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
    /// The USD cost observed from the provider response, when present.
    pub cost_usd: Option<f64>,
}

/// Rerank scores plus the call's measured usage — the rerank twin of
/// [`EmbeddingBatch`]. `reported: false` on `usage` means the provider
/// omitted the usage wire field.
#[derive(Debug, Clone, PartialEq)]
pub struct RerankBatch {
    pub scores: Vec<crate::inference_ipc::RerankScoreEntry>,
    pub usage: crate::ports::InferenceUsage,
    pub cost_usd: Option<f64>,
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
