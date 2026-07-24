//! Kata-specific error types.

#[derive(Debug, thiserror::Error)]
pub enum KataError {
    #[error("Failed to load manifest: {0}")]
    LoadFailed(String),
    #[error("Failed to parse manifest: {0}")]
    ParseFailed(String),
    #[error("Unknown kata type: {0}")]
    UnknownType(String),
    #[error("Manifest '{0}' has no steps/questions/practices")]
    NoSteps(String),
    #[error("Gas exceeded: consumed {consumed}, cap {cap}")]
    GasExceeded { consumed: u64, cap: u64 },
    #[error("Inference failed: {0}")]
    InferenceFailed(String),
    #[error("Template not found: {0}")]
    TemplateNotFound(String),
}
