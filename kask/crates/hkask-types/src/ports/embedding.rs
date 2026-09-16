/// One embedding batch plus its request and provider-reported model identities.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingBatch {
    pub vectors: Vec<Vec<f32>>,
    /// Exact model identity supplied by the caller.
    pub requested_model: String,
    /// Exact model identity returned by the provider, when present.
    pub actual_model: Option<String>,
}

/// Errors from embedding generation backends (OpenAI, local models, etc.).
#[derive(Debug, Clone, thiserror::Error)]
pub enum EmbeddingGenerationError {
    #[error("Invalid embedding request: {0}")]
    InvalidRequest(String),
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("API error: status {0}: {1}")]
    Api(u16, String),
    #[error("JSON parse error: {0}")]
    Json(String),
    #[error("Empty response from embedding model")]
    EmptyResponse,
    #[error("Dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
}
