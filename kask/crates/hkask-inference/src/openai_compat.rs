//! Shared OpenAI-compatible chat completion logic.
//!
//! All seven chat backends (DeepInfra, Together, OpenRouter, KiloCode, Fal,
//! Ollama, Cline) use [`openai_compatible_generate`] for their `generate()`
//! method. The function parameterizes the chat endpoint path and auth header
//! prefix to accommodate provider-specific differences:
//!
//! | Provider   | Chat path               | Auth header     |
//! |------------|-------------------------|-----------------|
//! | DeepInfra  | `/v1/chat/completions`  | `Bearer`        |
//! | Together   | `/v1/chat/completions`  | `Bearer`        |
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
#[allow(clippy::too_many_arguments)]
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
            provider_code, status, error_text
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
        let preview = if body.len() > 500 {
            format!("{}...", &body[..500])
        } else {
            body.clone()
        };
        InferenceError::Json(format!(
            "{} JSON parse: {} | body: {}",
            provider_code, e, preview
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
#[allow(clippy::too_many_arguments)]
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
            provider_code, status, error_text
        )));
    }

    let body = response
        .text()
        .await
        .map_err(|e| InferenceError::Connection(format!("{} body read: {}", provider_code, e)))?;

    let chat_response: ChatResponse = serde_json::from_str(&body).map_err(|e| {
        let preview = if body.len() > 500 {
            format!("{}...", &body[..500])
        } else {
            body.clone()
        };
        InferenceError::Json(format!(
            "{} JSON parse: {} | body: {}",
            provider_code, e, preview
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
