//! Shared OpenAI-compatible chat completion logic.
//!
//! All five chat backends (DeepInfra, OpenRouter, KiloCode, AtlasCloud, Ollama)
//! use [`openai_compatible_generate`] for their `generate()`
//! method. The function parameterizes the chat endpoint path and auth header
//! prefix to accommodate provider-specific differences:
//!
//! | Provider   | Chat path               | Auth header     |
//! |------------|-------------------------|-----------------|
//! | DeepInfra  | `/v1/chat/completions`  | `Bearer`        |
//! | OpenRouter | `/v1/chat/completions`  | `Bearer`        |
//! | KiloCode   | `/chat/completions`     | `Bearer`        |
//! | Ollama     | `/v1/chat/completions`  | `Bearer` (ignored) |
//!
//! RunPod does NOT use this function — it is vision/OCR-only (no chat).
//! `base_url` and `api_key` are passed directly (no `ProviderConfig` envelope).

use crate::chat_protocol::build_chat_request_messages;
use crate::chat_protocol::{ChatRequest, ChatResponse, chat_response_to_result, validate_prompt};
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

/// Shared tail of the OpenAI-compatible chat round-trip: send → status check
/// → body read → `serde_json::from_str` + `sanitize_error_body` →
/// `chat_response_to_result` → log. Used by both `openai_compatible_generate`
/// and `openai_compatible_generate_messages` so the error paths and log shape
/// cannot drift between them. `log_suffix` is appended to the completion log
/// message (e.g. `" (messages)"` for the multi-turn variant).
async fn openai_chat_roundtrip(
    client: &Client,
    base_url: &str,
    api_key: &str,
    request: ChatRequest,
    chat_path: &str,
    auth_prefix: &str,
    provider_code: &str,
    log_suffix: &str,
) -> Result<InferenceResult, InferenceError> {
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
        "{} inference completed{log_suffix}",
        provider_code
    );
    Ok(result)
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
    openai_chat_roundtrip(
        client,
        base_url,
        api_key,
        request,
        chat_path,
        auth_prefix,
        provider_code,
        "",
    )
    .await
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
    openai_chat_roundtrip(
        client,
        base_url,
        api_key,
        request,
        chat_path,
        auth_prefix,
        provider_code,
        " (messages)",
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

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

    proptest! {
        /// P1 invariant: for any body containing a prefix from SECRET_PREFIXES,
        /// the redacted output must not contain the secret token that followed
        /// the prefix. Covers all 13 prefixes (sk-, ghp_, AKIA, xoxb-, eyJ, etc.)
        /// and arbitrary secret values — the hardcoded tests only cover 3.
        /// The secret is the token *after* the prefix; the prefix_text before
        /// the prefix may coincidentally contain the same chars (e.g. secret="z"
        /// and prefix_text="z"), so we check the substring after [REDACTED].
        #[test]
        fn sanitize_redacts_every_secret_prefix(
            prefix_idx in 0usize..SECRET_PREFIXES.len(),
            secret in "[A-Za-z0-9+/=_-]{1,40}",
            prefix_text in "[a-z ]{0,20}",
            suffix_text in "[a-z ]{0,20}"
        ) {
            let prefix = SECRET_PREFIXES[prefix_idx];
            let body = format!("{prefix_text}{prefix}{secret}{suffix_text}");
            let sanitized = sanitize_error_body(&body);
            // Assert the prefix itself is redacted. Checking the secret string
            // directly is unsound: the secret alphabet `[A-Za-z0-9+/=_-]`
            // overlaps the `[a-z ]` filler, so the secret can coincidentally
            // reappear in prefix_text/suffix_text (e.g. secret="c" with
            // suffix_text=" c") — a false positive. The redactor consumes
            // prefix+token as a unit, so "prefix gone" entails "secret consumed",
            // and the redactor re-scans the whole body so a prefix coincidentally
            // present in the filler is redacted too (no false positives).
            prop_assert!(
                !sanitized.contains(prefix),
                "prefix '{}' survived redaction: body={:?} sanitized={:?}",
                prefix, body, sanitized
            );
        }

        /// P4 panic-freedom: sanitize_error_body must never panic on any input,
        /// including empty strings, multi-byte UTF-8, control chars, and
        /// bodies that are just a prefix with no following token.
        #[test]
        fn sanitize_never_panics(
            body in proptest::collection::vec(proptest::num::u8::ANY, 0..500)
        ) {
            let body = String::from_utf8_lossy(&body);
            let _ = sanitize_error_body(&body);
        }

        /// P1 invariant: the redacted output never contains any prefix from
        /// SECRET_PREFIXES followed by non-redacted characters. After
        /// redaction, every prefix occurrence must be replaced by [REDACTED].
        #[test]
        fn sanitized_output_has_no_raw_secret_prefixes(
            prefix_idx in 0usize..SECRET_PREFIXES.len(),
            filler in "[a-z0-9 ]{0,50}"
        ) {
            let prefix = SECRET_PREFIXES[prefix_idx];
            let body = format!("{filler} {prefix} secret_value_here {filler}");
            let sanitized = sanitize_error_body(&body);
            prop_assert!(
                !sanitized.contains("secret_value_here"),
                "secret survived for prefix '{}': sanitized={:?}",
                prefix, sanitized
            );
        }
    }
}
