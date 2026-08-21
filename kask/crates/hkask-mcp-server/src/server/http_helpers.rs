//! HTTP error classification for MCP tool responses.

use hkask_inference::openai_compat::sanitize_error_body;

use super::error::McpToolError;

/// Classify an HTTP error response into an `McpToolError`.
///
/// `pre`: service is non-empty, status is valid.
/// `post`: returns an `McpToolError` whose kind maps from the status code
/// (401/403 → permission_denied, 404 → not_found, 422 → invalid_argument,
/// 429 → rate_limited, 5xx → unavailable, else → internal). The body is
/// redacted via `sanitize_error_body` before formatting.
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
