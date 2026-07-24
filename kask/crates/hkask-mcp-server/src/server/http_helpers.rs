//! HTTP helpers — tool output wrapper, error classification, and REST convenience functions.

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
    let msg = format!("{service} API returned {status}: {}", body.trim());
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

async fn http_req(
    client: &reqwest::Client,
    service: &str,
    method: &str,
    url: &str,
    payload: Option<&Value>,
) -> Result<Value, McpToolError> {
    let builder = match method {
        "GET" => client.get(url),
        "POST" => client.post(url).json(payload.unwrap_or(&Value::Null)),
        _ => client.put(url).json(payload.unwrap_or(&Value::Null)),
    };
    let resp = builder
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("{service} request failed: {e}")))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(classify_http_error(service, status, &body));
    }
    serde_json::from_str(&body)
        .map_err(|e| McpToolError::internal(format!("Failed to parse {service} response: {e}")))
}

#[must_use = "result must be used"]
pub async fn api_get(
    client: &reqwest::Client,
    service: &str,
    url: &str,
) -> Result<Value, McpToolError> {
    http_req(client, service, "GET", url, None).await
}
#[must_use = "result must be used"]
pub async fn api_put(
    client: &reqwest::Client,
    service: &str,
    url: &str,
    payload: &Value,
) -> Result<Value, McpToolError> {
    http_req(client, service, "PUT", url, Some(payload)).await
}
