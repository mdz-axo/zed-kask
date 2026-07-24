//! Error types for hkask-mcp library operations.
//!
//! Two error layers:
//! - `McpError` — server-level failures (missing credentials, daemon errors, storage, transport)
//! - `McpToolError` — tool-level failures with structured classification (internal, not_found, etc.)

use hkask_types::McpErrorKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Unified error type for hkask-mcp library operations.
///
/// Replaces `anyhow::Error` in all public APIs. Every variant carries
/// structured context suitable for Regulation spans and operator diagnostics.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("{0} set but HKASK_DB_PASSPHRASE missing")]
    DatabasePassphrase(String),

    #[error(
        "UserPod '{userpod}' is not authenticated. Enter the userpod's passphrase in the hKask terminal."
    )]
    Auth { userpod: String },

    #[error(
        "UserPod '{userpod}' is not assigned to the {role} MCP role. Use 'kask pod assign {userpod} {role}' to grant this role."
    )]
    RoleAssignment { userpod: String, role: String },

    #[error("Unexpected {context} response: {detail}")]
    UnexpectedResponse { context: String, detail: String },

    #[error(
        "Missing required credentials: {missing}. Set them via environment variables or hkask-keystore."
    )]
    MissingCredentials { missing: String },

    #[error("MCP host identity is required: set {env_var}")]
    MissingHostIdentity { env_var: String },

    #[error("Daemon communication error: {0}")]
    Daemon(#[from] std::io::Error),

    #[error("Storage error: {0}")]
    Storage(#[from] hkask_storage::DatabaseError),

    #[error("Transport error: {0}")]
    Transport(Box<rmcp::RmcpError>),
}

impl From<rmcp::RmcpError> for McpError {
    fn from(e: rmcp::RmcpError) -> Self {
        McpError::Transport(Box::new(e))
    }
}

// ── McpToolError ──────────────────────────────────────────────────────────

/// Structured error from a tool dispatch, carrying semantic classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolError {
    pub kind: McpErrorKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

impl McpToolError {
    /// Create a new McpToolError.
    ///
    /// expect: "The system reports tool dispatch failures with structured classification"
    /// pre:  kind is a valid McpErrorKind; message is non-empty
    /// post: returns McpToolError with the given kind and message
    #[must_use]
    pub fn new(kind: McpErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            details: None,
        }
    }
    /// Create an internal error.
    ///
    /// expect: "The system reports tool dispatch failures with structured classification"
    /// post: returns McpToolError with Internal kind
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(McpErrorKind::Internal, message)
    }
    /// Create a not-found error.
    ///
    /// expect: "The system reports tool dispatch failures with structured classification"
    /// post: returns McpToolError with NotFound kind
    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(McpErrorKind::NotFound, message)
    }
    /// Create an invalid-argument error.
    ///
    /// expect: "The system reports tool dispatch failures with structured classification"
    /// post: returns McpToolError with InvalidArgument kind
    #[must_use]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(McpErrorKind::InvalidArgument, message)
    }
    /// Create an unavailable error.
    ///
    /// expect: "The system reports tool dispatch failures with structured classification"
    /// post: returns McpToolError with Unavailable kind
    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(McpErrorKind::Unavailable, message)
    }
    /// Create a timeout error.
    ///
    /// expect: "The system reports tool dispatch failures with structured classification"
    /// post: returns McpToolError with Timeout kind
    #[must_use]
    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(McpErrorKind::Timeout, message)
    }
    /// Create a permission-denied error.
    ///
    /// expect: "The system reports tool dispatch failures with structured classification"
    /// post: returns McpToolError with PermissionDenied kind
    #[must_use]
    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::new(McpErrorKind::PermissionDenied, message)
    }
    /// Create a rate-limited error.
    ///
    /// expect: "The system reports tool dispatch failures with structured classification"
    /// post: returns McpToolError with RateLimited kind
    #[must_use]
    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(McpErrorKind::RateLimited, message)
    }
    /// Create a failed-precondition error.
    ///
    /// expect: "The system reports tool dispatch failures with structured classification"
    /// post: returns McpToolError with FailedPrecondition kind
    #[must_use]
    pub fn failed_precondition(message: impl Into<String>) -> Self {
        Self::new(McpErrorKind::FailedPrecondition, message)
    }
    /// Serialize to JSON string for MCP wire format.
    ///
    /// expect: "The system reports tool dispatch failures with structured classification"
    /// post: returns JSON string with "error" object containing message and kind
    #[must_use]
    pub fn to_json_string(&self) -> String {
        serde_json::json!({"error": self.message, "kind": self.kind.to_string()}).to_string()
    }
}

impl std::fmt::Display for McpToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.message)
    }
}

impl std::error::Error for McpToolError {}
