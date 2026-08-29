//! Error types for hkask-mcp library operations.
//!
//! Two error layers:
//! - `McpError` — server-level failures (missing credentials, storage, transport)
//! - `McpToolError` — tool-level failures with structured classification (internal, not_found, etc.)

use hkask_types::McpErrorKind;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Unified error type for hkask-mcp library operations.
///
/// Replaces `anyhow::Error` in all public APIs. Every variant carries
/// structured context suitable for Regulation spans and operator diagnostics.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("{0} set but HKASK_DB_PASSPHRASE missing")]
    DatabasePassphrase(String),

    #[error("Unexpected {context} response: {detail}")]
    UnexpectedResponse { context: String, detail: String },

    #[error(
        "Missing required credentials: {missing}. Set them via environment variables or hkask-keystore."
    )]
    MissingCredentials { missing: String },

    #[error("Storage error: {0}")]
    Storage(#[from] hkask_storage::DatabaseError),

    #[error("Infrastructure error: {0}")]
    Infrastructure(#[from] hkask_types::InfrastructureError),

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
    /// Serialize to JSON string — the `structured_content` payload of the
    /// wire error result (see the `IntoCallToolResult` impl below).
    #[must_use]
    pub fn to_json_string(&self) -> String {
        serde_json::json!({"error": self.message, "kind": self.kind.to_string()}).to_string()
    }
}

/// The core wire pattern for tool errors: a tool-logical error is a REAL
/// error result. rmcp's blanket `IntoCallToolResult for Result<T, E>` marks
/// the result `is_error: true` when a tool returns `Err`, so servers whose
/// tools return `Result<String, McpToolError>` get native error semantics
/// on the wire — the typed kind rides in `structured_content` and the
/// human-readable message as text content. No in-band envelope sniffing
/// is needed on the client: `is_error` is set by the protocol, and the
/// kind is read from `structured_content` (shape: `{"error", "kind"}`,
/// parseable by `hkask_types::tool_response::parse_tool_error`).
impl rmcp::handler::server::tool::IntoCallToolResult for McpToolError {
    fn into_call_tool_result(
        self,
    ) -> std::result::Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        // `CallToolResult` is `#[non_exhaustive]` in this rmcp version — it
        // cannot be built with a struct expression outside the rmcp crate,
        // so build from `Default` and assign fields.
        let mut result = rmcp::model::CallToolResult::default();
        result.is_error = Some(true);
        result.content = vec![rmcp::model::ContentBlock::text(self.message.clone())];
        result.structured_content = Some(serde_json::json!({
            "error": self.message,
            "kind": self.kind.to_string(),
        }));
        Ok(result.into())
    }
}

impl std::fmt::Display for McpToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.message)
    }
}

impl std::error::Error for McpToolError {}
