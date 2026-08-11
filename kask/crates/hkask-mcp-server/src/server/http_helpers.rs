//! HTTP helpers — tool output wrapper and HTTP error classification.

use hkask_inference::openai_compat::sanitize_error_body;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::McpToolError;

/// Tool result with optional observability metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct McpToolOutput {
    pub(crate) content: Value,
}

impl McpToolOutput {
    pub(crate) fn new(content: Value) -> Self {
        Self { content }
    }

    /// Serialize to JSON string for rmcp tool return value.
    pub(crate) fn to_json_string(&self) -> String {
        serde_json::to_string(&serde_json::json!({"content": &self.content})).unwrap_or_else(|e| {
            serde_json::json!({"content": format!("serialization error: {e}")}).to_string()
        })
    }
}

// ── HTTP helpers ──────────────────────────────────────────────────────────

/// Classify an HTTP error response into a structured `McpToolError`.
/// Classify an HTTP error response into an McpToolError.
///
/// pre:  service is non-empty, status is valid
/// post: returns McpToolError with appropriate kind based on status code
#[must_use]
pub fn classify_http_error(service: &str, status: reqwest::StatusCode, body: &str) -> McpToolError {
    let sanitized = sanitize_error_body(body);
    let msg = format!("{service} API returned {status}: {}", sanitized.trim());
    match status.as_u16() {
        401 | 403 => McpToolError::permission_denied(msg),
        404 => McpToolError::not_found(msg),
        422 => McpToolError::invalid_argument(msg),
        429 => McpToolError::rate_limited(msg),
        502 | 503 => McpToolError::unavailable(msg),
        _ if status.is_server_error() => McpToolError::unavailable(msg),
        _ => McpToolError::internal(msg),
    }
}
