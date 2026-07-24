//! Error types for hkask-codegraph.
//!
//! D3 fix: proper error enum via thiserror (idiomatic-rust Principle 7: errors as values).

use thiserror::Error;

/// All errors the codegraph engine can produce.
#[derive(Debug, Error)]
pub enum CodeGraphError {
    /// Failed to parse source code with tree-sitter.
    #[error("parse error in {file}: {message}")]
    Parse { file: String, message: String },

    /// Failed to index a file or batch of files.
    #[error("index error: {0}")]
    Index(#[from] IndexError),

    /// Database query or operation failed.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// Graph traversal error.
    #[error("traversal error: {0}")]
    Traversal(String),

    /// Serialization error (JSON).
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Internal invariant violation.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Index-specific errors.
#[derive(Debug, Error)]
pub enum IndexError {
    /// A file was not found or couldn't be read.
    #[error("file not accessible: {path}")]
    FileNotAccessible {
        path: String,
        #[source]
        source: Option<std::io::Error>,
    },

    /// A file's content couldn't be read as UTF-8.
    #[error("file not valid UTF-8: {path}")]
    NotUtf8 { path: String },

    /// A batch insert failed.
    #[error("batch insert failed: {0}")]
    BatchInsert(String),
}

/// Convenience result type.
pub type Result<T> = std::result::Result<T, CodeGraphError>;
