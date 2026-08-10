//! Shared OpenAI-compatible chat completion logic.
//!
//! All six chat backends (DeepInfra, OpenRouter, KiloCode, Fal, Ollama, Cline)
//! use [`openai_compatible_generate`] for their `generate()`
//! method. The function parameterizes the chat endpoint path and auth header
//! prefix to accommodate provider-specific differences:
//!
//! | Provider   | Chat path               | Auth header     |
//! |------------|-------------------------|-----------------|
//! | DeepInfra  | `/v1/chat/completions`  | `Bearer`        |
//! | OpenRouter | `/v1/chat/completions`  | `Bearer`        |
//! | KiloCode   | `/chat/completions`     | `Bearer`        |
//! | Fal        | `/v1/chat/completions`  | `Key`           |
//! | Ollama     | `/v1/chat/completions`  | `Bearer` (ignored) |
//! | Cline      | `/v1/chat/completions`  | `Bearer`        |
//!
//! RunPod does NOT use this function — it is vision/OCR-only (no chat).
//! `base_url` and `api_key` are passed directly (no `ProviderConfig` envelope).

use crate::chat_protocol::build_chat_request_messages;
use crate::chat_protocol::{ChatResponse, chat_response_to_result, validate_prompt};
use hkask_types::template::LLMParameters;
use hkask_types::{ChatMessage, ChatToolDefinition, InferenceError, InferenceResult};
use reqwest::Client;

/// Maximum length of a provider response body embedded in an error string.
pub const ERROR_BODY_MAX_CHARS: usize = 200;

/// Secret-shaped prefixes that a provider error page or proxy debug dump may
/// echo back (CWE-209). Redaction is a simple prefix scan, not a parser:
/// defense-in-depth before the body reaches IPC/log sinks.
///
/// All prefixes MUST be lowercase — they are matched against the lowercased
/// body in `redact_secret_tokens`.
pub const SECRET_PREFIXES: &[&str] = &[
    "authorization:",
    "bearer ",
    "sk-",
    "api_key",
    // Common credential prefixes beyond OpenAI's `sk-` (RR-0049/0050/0051):
    // GitHub PATs, AWS keys, Slack tokens, GitLab tokens, JWTs.
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "akaa", // AWS access key id prefix (case-insensitive match)
    "xoxb-",
    "xoxp-",
    "glpat-",
    "eyj", // JWT header base64 prefix (eyJ...)
];

/// Sanitizes a raw provider response body before embedding it in an error
/// string: redacts secret-shaped substrings (prefix through the end of the
/// whitespace-delimited token) and truncates to [`ERROR_BODY_MAX_CHARS`]
/// (char-boundary safe) with a total-length suffix.
///
/// Shared across inference backends, the MCP `classify_http_error` helper,
/// and research providers (RR-0035 class — RR-0049/0050/0051).
#[must_use]
pub fn sanitize_error_body(body: &str) -> String {
    let redacted = redact_secret_tokens(body);
    let total_bytes = body.len();
    if redacted.chars().count() <= ERROR_BODY_MAX_CHARS {
        redacted
    } else {
        let boundary = redacted
            .char_indices()
            .nth(ERROR_BODY_MAX_CHARS)
            .map(|(index, _)| index)
            .unwrap_or(redacted.len());
        format!("{}… ({} bytes total)", &redacted[..boundary], total_bytes)
    }
}

pub fn redact_secret_tokens(body: &str) -> String {
    // Case-insensitive byte-index scan; prefixes are ASCII so byte matching is exact.
    let lower = body.to_ascii_lowercase();
    let mut output = String::with_capacity(body.len());
    let mut cursor = 0;
    while cursor < body.len() {
        let match_at = SECRET_PREFIXES
            .iter()
            .filter_map(|prefix| lower[cursor..].find(prefix).map(|offset| cursor + offset))
            .min();
        match match_at {
            Some(start) => {
                output.push_str(&body[cursor..start]);
                output.push_str("[REDACTED]");
                let prefix_len = SECRET_PREFIXES
                    .iter()
                    .filter(|prefix| lower[start..].starts_with(**prefix))
                    .map(|prefix| prefix.len())
                    .max()
                    .unwrap_or(0);
                // Skip whitespace after the prefix — for "Authorization: Basic abc123"
                // the token follows a space, and without this the scan stops at
                // that space and the secret survives.
                let token_start = start
                    + prefix_len
                    + (body[start + prefix_len..].len()
                        - body[start + prefix_len..].trim_start().len());
                // Header-style prefixes ("Authorization:", "api_key") may be
                // followed by a scheme word ("Basic", "Bearer") before the
                // secret, so redact to end-of-line. Opaque token prefixes
                // ("Bearer ", "sk-") redact to end-of-token.
                let matched_prefix = SECRET_PREFIXES
                    .iter()
                    .filter(|prefix| lower[start..].starts_with(**prefix))
                    .max_by_key(|prefix| prefix.len());
                let to_end_of_line =
                    matches!(matched_prefix, Some(p) if p.ends_with(':') || *p == "api_key");
                cursor = if to_end_of_line {
                    body[token_start..]
                        .find('\n')
                        .map(|offset| token_start + offset)
                        .unwrap_or(body.len())
                } else {
                    body[token_start..]
                        .find(char::is_whitespace)
                        .map(|offset| token_start + offset)
                        .unwrap_or(body.len())
                };
            }
            None => {
                output.push_str(&body[cursor..]);
                break;
            }
        }
    }
    output
}

/// Parameterized OpenAI-compatible chat completion.
///
/// `base_url` is the provider API root (the `chat_path` is appended to it).
/// `api_key` is sent as `Authorization: {auth_prefix} {api_key}`.
/// `chat_path` is the URL path appended to `base_url` (e.g., `/v1/chat/completions`).
/// `auth_prefix` is the `Authorization` header prefix (e.g., `"Bearer"` or `"Key"`).
/// `provider_code` is the short provider identifier used in logs and error messages.
///
/// expect: "The system regulates text/image/speech generation through provider membranes"
/// \[P9\] Motivating: Homeostatic Self-Regulation — shared regulated generation for OpenAI-compatible backends
/// pre:  model is a valid provider model name
/// pre:  prompt is non-empty (validated by validate_prompt)
/// pre:  params is a valid LLMParameters
/// post: returns Ok(InferenceResult) with generated text, model, usage stats
/// post: if connection fails → Err(InferenceError::Connection)
/// post: if prompt is empty → Err(InferenceError::Generation)
pub async fn openai_compatible_generate(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    prompt: &str,
    params: &LLMParameters,
    tools: Option<&[ChatToolDefinition]>,
    chat_path: &str,
    auth_prefix: &str,
    provider_code: &str,
) -> Result<InferenceResult, InferenceError> {
    validate_prompt(prompt)?;
    let tools = tools.map(|t| t.to_vec());
    let mut messages = Vec::with_capacity(2);
    if let Some(ref sys) = params.system_prompt {
        messages.push(ChatMessage::system(sys));
    }
    messages.push(ChatMessage::user(prompt));
    let request = build_chat_request_messages(model, messages, params, Some(false), None, tools);

    let response = client
        .post(format!("{}{}", base_url, chat_path))
        .header("Authorization", format!("{} {}", auth_prefix, api_key))
        .json(&request)
        .send()
        .await
        .map_err(|e| InferenceError::Connection(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(InferenceError::Connection(format!(
            "{} status {}: {}",
            provider_code,
            status,
            sanitize_error_body(&error_text)
        )));
    }

    // Capture the raw body before deserializing so parse errors include
    // the actual response text for debugging (reqwest's `.json()` consumes
    // the body and only reports "error decoding response body").
    let body = response
        .text()
        .await
        .map_err(|e| InferenceError::Connection(format!("{} body read: {}", provider_code, e)))?;

    let chat_response: ChatResponse = serde_json::from_str(&body).map_err(|e| {
        InferenceError::Json(format!(
            "{} JSON parse: {} | body: {}",
            provider_code,
            e,
            sanitize_error_body(&body)
        ))
    })?;

    let result = chat_response_to_result(chat_response)?;
    tracing::info!(
        target: "reg.inference",
        provider = provider_code,
        model = %result.model,
        tokens = result.usage.total_tokens,
        finish_reason = %result.finish_reason,
        "{} inference completed",
        provider_code
    );
    Ok(result)
}

/// Parameterized OpenAI-compatible chat completion with an explicit message
/// array.
///
/// This is the multi-turn variant of [`openai_compatible_generate`]: instead of
/// constructing a `[system?, user]` pair from a single prompt string, it passes
/// the caller-supplied `messages` directly to the provider. Each message carries
/// its own role (`"system"`, `"user"`, `"assistant"`), so the provider sees the
/// full conversation history.
///
/// `base_url` is the provider API root (the `chat_path` is appended to it).
/// `api_key` is sent as `Authorization: {auth_prefix} {api_key}`.
/// `chat_path` is the URL path appended to `base_url` (e.g., `/v1/chat/completions`).
/// `auth_prefix` is the `Authorization` header prefix (e.g., `"Bearer"` or `"Key"`).
/// `provider_code` is the short provider identifier used in logs and error messages.
///
/// expect: "The system regulates text/image/speech generation through provider membranes"
/// \[P9\] Motivating: Homeostatic Self-Regulation — shared regulated generation for multi-turn OpenAI-compatible backends
/// pre:  model is a valid provider model name
/// pre:  messages is non-empty
/// pre:  params is a valid LLMParameters
/// post: returns Ok(InferenceResult) with generated text, model, usage stats
/// post: if connection fails → Err(InferenceError::Connection)
pub async fn openai_compatible_generate_messages(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: &[ChatMessage],
    params: &LLMParameters,
    tools: Option<&[ChatToolDefinition]>,
    chat_path: &str,
    auth_prefix: &str,
    provider_code: &str,
) -> Result<InferenceResult, InferenceError> {
    if messages.is_empty() {
        return Err(InferenceError::Generation(
            "messages array is empty".to_string(),
        ));
    }
    let tools = tools.map(|t| t.to_vec());
    let request =
        build_chat_request_messages(model, messages.to_vec(), params, Some(false), None, tools);

    let response = client
        .post(format!("{}{}", base_url, chat_path))
        .header("Authorization", format!("{} {}", auth_prefix, api_key))
        .json(&request)
        .send()
        .await
        .map_err(|e| InferenceError::Connection(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(InferenceError::Connection(format!(
            "{} status {}: {}",
            provider_code,
            status,
            sanitize_error_body(&error_text)
        )));
    }

    let body = response
        .text()
        .await
        .map_err(|e| InferenceError::Connection(format!("{} body read: {}", provider_code, e)))?;

    let chat_response: ChatResponse = serde_json::from_str(&body).map_err(|e| {
        InferenceError::Json(format!(
            "{} JSON parse: {} | body: {}",
            provider_code,
            e,
            sanitize_error_body(&body)
        ))
    })?;

    let result = chat_response_to_result(chat_response)?;
    tracing::info!(
        target: "reg.inference",
        provider = provider_code,
        model = %result.model,
        tokens = result.usage.total_tokens,
        finish_reason = %result.finish_reason,
        "{} inference completed (messages)",
        provider_code
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_error_body_redacts_authorization_bearer_token() {
        let body = "upstream failed. Authorization: Bearer sk-testkey123 was rejected";
        let sanitized = sanitize_error_body(body);
        assert!(!sanitized.contains("sk-testkey123"), "{sanitized}");
        assert!(sanitized.contains("[REDACTED]"), "{sanitized}");
    }

    #[test]
    fn sanitize_error_body_redacts_common_secret_prefixes() {
        let body = "key sk-abc123XYZ rejected; api_key=hunter2";
        let sanitized = sanitize_error_body(body);
        assert!(!sanitized.contains("abc123XYZ"), "{sanitized}");
        assert!(!sanitized.contains("hunter2"), "{sanitized}");
    }

    #[test]
    fn sanitize_error_body_redacts_authorization_without_bearer() {
        // The token may follow the prefix after whitespace without a "Bearer"
        // marker (e.g. "Authorization: Basic abc123" in a proxy error page) —
        // the redaction must skip that whitespace, not stop at it.
        // Header-style prefixes redact to end-of-line, so trailing text on
        // the same line is redacted too (conservative, safe direction).
        let body = "proxy error. Authorization: Basic abc123\nstatus 401";
        let sanitized = sanitize_error_body(body);
        assert!(!sanitized.contains("abc123"), "escaped: {:?}", sanitized);
        assert!(!sanitized.contains("Basic"), "{sanitized}");
        assert!(sanitized.contains("status 401"), "{sanitized}");
    }

    #[test]
    fn sanitize_error_body_truncates_long_body_and_reports_total() {
        let body = "x".repeat(1000);
        let sanitized = sanitize_error_body(&body);
        assert!(sanitized.contains("(1000 bytes total)"), "{sanitized}");
        // 200 chars of body + ellipsis + suffix.
        assert!(sanitized.chars().count() < 240, "{sanitized}");
    }

    #[test]
    fn sanitize_error_body_passes_short_clean_body_through() {
        let body = "model not found";
        assert_eq!(sanitize_error_body(body), body);
    }

    #[test]
    fn sanitize_error_body_truncates_on_char_boundary() {
        // 250 multi-byte chars: byte-based slicing would panic mid-char.
        let body = "é".repeat(250);
        let sanitized = sanitize_error_body(&body);
        assert!(sanitized.starts_with(&"é".repeat(ERROR_BODY_MAX_CHARS)));
    }
}
