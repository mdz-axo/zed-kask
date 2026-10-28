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

/// Percent-encode a query-parameter value per RFC 3986 (unreserved + `+` for
/// space). Prevents caller-controlled values from injecting extra query params
/// (`&limit=...`) or truncating the URL (`#`).
#[must_use]
pub fn url_encode_query_value(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push('%');
                encoded.push(hex_digit(byte >> 4));
                encoded.push(hex_digit(byte & 0x0f));
            }
        }
    }
    encoded
}

const fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + (nibble - 10)) as char,
        _ => '0',
    }
}

/// Execute a GET query against `https://serpapi.com/search` and parse the
/// JSON response body. Shared by the corpus server and its siblings.
pub async fn serpapi_get_json(
    client: &reqwest::Client,
    params: &[(&str, String)],
) -> Result<serde_json::Value, McpToolError> {
    let response = client
        .get("https://serpapi.com/search")
        .query(params)
        .send()
        .await
        .map_err(|error| McpToolError::unavailable(format!("request failed: {error}")))?;
    let body = response
        .text()
        .await
        .map_err(|error| McpToolError::unavailable(format!("body read failed: {error}")))?;
    serde_json::from_str(&body)
        .map_err(|error| McpToolError::unavailable(format!("malformed JSON: {error}")))
}
