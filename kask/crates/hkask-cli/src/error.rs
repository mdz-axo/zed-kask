//! CLI error types — replaces `Result<_, String>` anti-pattern.
//!
//! `CliError` is a thiserror enum with variants for each CLI error category.
//! All variants use `#[error("{0}")]` so `Display` output matches the original
//! `String` error messages exactly.

use thiserror::Error;

/// Error type for CLI command handlers.
#[derive(Debug, Error)]
pub enum CliError {
    /// Bad user input (pod ID, token, wallet ID, etc.)
    #[error("{0}")]
    InvalidInput(String),
    /// Daemon communication failures
    #[error("{0}")]
    Daemon(String),
    /// Agent service errors
    #[error("{0}")]
    AgentService(String),
    /// Filesystem errors
    #[error("{0}")]
    Io(String),
    /// Configuration errors
    #[error("{0}")]
    Config(String),
    /// Matrix integration errors
    #[error("{0}")]
    Matrix(String),
    /// Onboarding pipeline errors
    #[error("{0}")]
    Onboarding(String),
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for CliError {
    fn from(e: serde_json::Error) -> Self {
        CliError::InvalidInput(e.to_string())
    }
}
