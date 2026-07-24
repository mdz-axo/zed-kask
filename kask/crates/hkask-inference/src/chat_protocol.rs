//! Shared OpenAI-compatible chat completion protocol types and helpers.
//!
//! All seven chat backends (DeepInfra, Together AI, fal.ai, OpenRouter,
//! KiloCode, Ollama, Cline) speak the same `/v1/chat/completions` wire format.
//! This module provides the shared request/response types and helper functions
//! used by all backends.
//!
//! These are free functions, not a trait — the backends share the wire format
//! but own their HTTP client, auth, and model listing endpoint independently.

use futures_util::StreamExt;
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferenceResult, InferenceStreamChunk,
    InferenceUsage, StructuredToolCall, TokenProb, TokenProbability,
};
use serde::{Deserialize, Serialize};
use tracing::info;

#[allow(dead_code)] // referenced as serde default via string
fn default_enable_thinking() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}

// ── Request types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub top_p: f32,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub top_k: i32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub min_p: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub typical_p: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub frequency_penalty: f32,
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub presence_penalty: f32,
    pub max_tokens: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_probs: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default = "default_enable_thinking", skip_serializing_if = "is_true")]
    pub enable_thinking: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_template_kwargs: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ChatToolDefinition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
}

/// Build an OpenAI-compatible chat completion request from an explicit message
/// array.
///
/// Unlike the prompt-string path, which constructs a `[system?, user]` pair from
/// a single prompt string, this function passes the caller-supplied messages
/// directly to the provider. This is the correct path for multi-turn chat: each
/// message carries its own role (`"system"`, `"user"`, `"assistant"`), so the
/// provider sees the full conversation history instead of a flattened string.
///
/// `stream: false` is explicit in non-streaming calls to prevent chunked
/// transfer encoding from confusing JSON parsers.
///
/// expect: "The system constructs and validates regulated LLM requests"
/// \[P9\] Motivating: Homeostatic Self-Regulation — constructs regulated LLM request payload from message array
/// pre:  model is non-empty, messages is non-empty
/// post: returns ChatRequest with the caller-supplied messages and parameters
#[must_use]
pub fn build_chat_request_messages(
    model: &str,
    messages: Vec<ChatMessage>,
    params: &LLMParameters,
    stream: Option<bool>,
    n_probs: Option<i32>,
    tools: Option<Vec<ChatToolDefinition>>,
) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages,
        temperature: params.temperature,
        top_p: params.top_p,
        top_k: params.top_k as i32,
        min_p: params.min_p,
        typical_p: params.typical_p,
        frequency_penalty: params.frequency_penalty,
        presence_penalty: params.presence_penalty,
        max_tokens: params.max_tokens as i32,
        seed: params.seed,
        n_probs,
        stream,
        enable_thinking: !params.disable_thinking,
        chat_template_kwargs: if params.disable_thinking {
            Some(serde_json::json!({"enable_thinking": false}))
        } else {
            None
        },
        tools,
        tool_choice: None,
    }
}

/// Build an OpenAI-standard multimodal vision request.
///
/// Uses the content-array format (standard across OpenAI, llama.cpp, RunPod):
/// ```json
/// {"messages": [{"role": "user", "content": [
///   {"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}},
///   {"type": "text", "text": "Extract all text..."}
/// ]}]}
/// ```
#[must_use]
pub fn build_vision_request(
    model: &str,
    prompt: &str,
    images: &[String],
    params: &LLMParameters,
) -> serde_json::Value {
    let mut content: Vec<serde_json::Value> = images
        .iter()
        .map(|b64| {
            serde_json::json!({
                "type": "image_url",
                "image_url": {"url": format!("data:image/jpeg;base64,{}", b64)}
            })
        })
        .collect();
    content.push(serde_json::json!({"type": "text", "text": prompt}));

    serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": content}],
        "temperature": params.temperature,
        "top_p": params.top_p,
        "max_tokens": params.max_tokens,
    })
}

/// Shared vision inference — sends OpenAI multimodal request and parses response.
/// Used by DeepInfra, Together, OpenRouter, and KiloCode backends.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn vision_infer(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    label: &str,
    model: &str,
    prompt: &str,
    images: &[String],
    params: &LLMParameters,
) -> Result<InferenceResult, InferenceError> {
    validate_prompt(prompt)?;
    if images.is_empty() {
        return Err(InferenceError::Generation("No images provided".into()));
    }
    let request = build_vision_request(model, prompt, images, params);
    let response = client
        .post(format!("{}/v1/chat/completions", base_url))
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&request)
        .send()
        .await
        .map_err(|e| InferenceError::Connection(format!("{} vision: {}", label, e)))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(InferenceError::Connection(format!(
            "{} vision {}: {}",
            label, status, body
        )));
    }
    let body = response
        .text()
        .await
        .map_err(|e| InferenceError::Connection(format!("{} body read: {}", label, e)))?;
    let chat_response: ChatResponse = serde_json::from_str(&body).map_err(|e| {
        let preview = if body.len() > 500 {
            format!("{}...", &body[..500])
        } else {
            body.clone()
        };
        InferenceError::Json(format!("{} JSON: {} | body: {}", label, e, preview))
    })?;
    let result = chat_response_to_result(chat_response)?;
    info!(target: "reg.inference", provider = label, model = %result.model, tokens = result.usage.total_tokens, "{} vision inference completed", label);
    Ok(result)
}

// ── Response types ───────────────────────────────────────────────────────────

/// OpenAI-compatible chat completion response.
///
/// `usage` is optional — some providers omit it (e.g. when streaming, or on
/// error fallback responses that still return 200 with a partial body).
#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub model: String,
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub message: ChatResponseMessage,
    /// May be `None` on some providers when the response is truncated or
    /// the model returns tool calls without an explicit stop reason.
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default, rename = "token_probs")]
    pub token_probs: Option<Vec<RawTokenProb>>,
}

#[derive(Debug, Deserialize)]
pub struct ChatResponseMessage {
    pub role: String,
    /// May be `None` when the model uses tool calls (OpenAI spec allows
    /// `null`) or when thinking mode exhausts the token budget before
    /// emitting `content` (GLM-5.2, Qwen3).
    #[serde(default)]
    pub content: Option<String>,
    /// Thinking-mode reasoning trace (Qwen3, GLM-5.2 on DeepInfra).
    /// Populated when the model thinks; the final answer lives in `content`.
    /// Captured so callers can recover the answer when `content` is empty
    /// (e.g. thinking exhausted the token budget before emitting `content`).
    #[serde(default)]
    pub reasoning_content: Option<String>,
    /// Thinking-mode reasoning trace (Ollama emits this as `reasoning`).
    /// DeepInfra/Qwen3 use `reasoning_content`; both are captured so the
    /// REPL can surface the chain-of-thought live.
    #[serde(default)]
    pub reasoning: Option<String>,
    /// Tool calls requested by the model (OpenAI function calling).
    /// Per the OpenAI Chat Completions API spec, `tool_calls` lives on the
    /// `message` object, not on the `choice`. When `finish_reason == "tool_calls"`,
    /// this field is populated with the requested tool calls.
    #[serde(default)]
    pub tool_calls: Option<Vec<RawToolCall>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ChatUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

// ── Token probability types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RawTokenProb {
    pub token: String,
    pub prob: f64,
    #[serde(default)]
    pub top_k: Vec<RawTokenProbTopK>,
}

#[derive(Debug, Deserialize)]
pub struct RawTokenProbTopK {
    pub token: String,
    pub prob: f64,
}

// ── Tool call types ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RawToolCall {
    pub id: Option<String>,
    #[serde(rename = "function")]
    pub function: RawFunctionCall,
}

#[derive(Debug, Deserialize)]
pub struct RawFunctionCall {
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

// ── SSE streaming types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct StreamChunk {
    pub choices: Vec<StreamChoice>,
    pub model: String,
    #[serde(default)]
    pub usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
pub struct StreamChoice {
    pub delta: StreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StreamDelta {
    #[serde(default)]
    pub content: Option<String>,
    /// Thinking-mode reasoning delta (DeepInfra/Qwen3/GLM-5.2 streaming).
    #[serde(default)]
    pub reasoning_content: Option<String>,
    /// Thinking-mode reasoning delta (Ollama streaming: `delta.reasoning`).
    #[serde(default)]
    pub reasoning: Option<String>,
    /// Tool calls requested by the model (OpenAI function calling, streaming).
    /// Per the OpenAI Chat Completions API spec, `tool_calls` lives on the
    /// `delta` object in streaming responses.
    #[serde(default)]
    pub tool_calls: Option<Vec<RawToolCall>>,
}

// ── Conversion helpers ──────────────────────────────────────────────────────

/// Map raw tool calls to `StructuredToolCall`.
///
/// Tool call names use `server/tool` convention (e.g., `memory/recall`).
/// If no `/` separator, server is empty and the full name is the tool.
/// Map raw tool calls from API response to structured ToolCall format.
///
/// expect: "The system constructs and validates regulated LLM requests"
/// \[P9\] Motivating: Homeostatic Self-Regulation — structured tool-call results for routing
/// pre:  calls is a valid slice of RawToolCall
/// post: returns `Vec<StructuredToolCall>` with parsed arguments
#[must_use]
pub fn map_tool_calls(calls: &[RawToolCall]) -> Vec<StructuredToolCall> {
    calls
        .iter()
        .map(|tc| {
            let (server, tool) = tc
                .function
                .name
                .split_once('/')
                .map(|(s, t)| (s.to_string(), t.to_string()))
                .unwrap_or_else(|| (String::new(), tc.function.name.clone()));
            StructuredToolCall {
                server,
                tool,
                args: tc.function.arguments.clone(),
                call_id: Some(tc.id.clone().unwrap_or_default()),
            }
        })
        .collect()
}

/// Convert raw token probabilities to `TokenProbability`.
/// Map raw token probabilities to structured TokenProbability format.
///
/// expect: "The system constructs and validates regulated LLM requests"
/// \[P9\] Motivating: Homeostatic Self-Regulation — token probability metadata for monitoring
/// pre:  probs is a valid slice of RawTokenProb
/// post: returns `Vec<TokenProbability>` with mapped fields
#[must_use]
pub fn map_token_probs(probs: &[RawTokenProb]) -> Vec<TokenProbability> {
    probs
        .iter()
        .map(|p| TokenProbability {
            token: p.token.clone(),
            prob: p.prob,
            top_k: p
                .top_k
                .iter()
                .map(|tk| TokenProb {
                    token: tk.token.clone(),
                    prob: tk.prob,
                })
                .collect(),
        })
        .collect()
}

/// Convert a `ChatResponse` into an `InferenceResult`.
/// Convert a chat completion response to InferenceResult.
///
/// expect: "The system normalizes provider responses for monitoring"
/// \[P9\] Motivating: Homeostatic Self-Regulation — normalizes provider response for monitoring
/// pre:  response is a valid ChatResponse
/// post: returns Ok(InferenceResult) with text, usage, finish_reason
/// post: returns Err if no choices in response
#[must_use = "result must be used"]
pub fn chat_response_to_result(response: ChatResponse) -> Result<InferenceResult, InferenceError> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| InferenceError::Generation("Empty response".to_string()))?;

    let token_probabilities = choice.token_probs.as_ref().map(|p| map_token_probs(p));

    let tool_calls = choice
        .message
        .tool_calls
        .as_ref()
        .map(|calls| map_tool_calls(calls))
        .unwrap_or_default();

    // Thinking-mode models (Qwen3, GLM-5.2, DeepSeek-R1) put the final answer
    // in `content` and deliberation in `reasoning_content` (DeepInfra) or
    // `reasoning` (Ollama). When `content` is null/empty (the model spent its
    // token budget reasoning, or returned tool calls without content), fall
    // back to the reasoning trace so downstream JSON extractors can still
    // recover the answer — in that degenerate case the reasoning IS the
    // answer, so it is not also surfaced separately as thinking (avoids the
    // chain-of-thought-as-answer leak).
    let reasoning_raw = choice
        .message
        .reasoning_content
        .clone()
        .or(choice.message.reasoning.clone());
    let content = choice.message.content.unwrap_or_default();
    let (text, reasoning) = if !content.is_empty() {
        (content, reasoning_raw)
    } else {
        (reasoning_raw.unwrap_or_default(), None)
    };

    let usage = response.usage.unwrap_or_default();

    Ok(InferenceResult {
        text,
        model: response.model,
        reasoning,
        usage: InferenceUsage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        },
        finish_reason: choice
            .finish_reason
            .unwrap_or_else(|| "unknown".to_string()),
        token_probabilities,
        tool_calls,
    })
}

/// Parse SSE stream lines into `InferenceStreamChunk` vec.
/// Parse an SSE stream into InferenceStreamChunks.
///
/// expect: "The system constructs and validates regulated LLM requests"
/// \[P9\] Motivating: Homeostatic Self-Regulation — parses streaming response chunks for regulated output
/// pre:  stream is a valid SSE byte stream
/// post: returns stream of InferenceStreamChunk parsed from SSE data lines
#[must_use]
pub fn parse_sse_stream(
    body: &str,
    model_id: &str,
) -> Vec<Result<InferenceStreamChunk, InferenceError>> {
    let mut chunks: Vec<Result<InferenceStreamChunk, InferenceError>> = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line == "data: [DONE]" {
            continue;
        }
        let json_str = line.strip_prefix("data: ").unwrap_or(line);
        let chunk: StreamChunk = match serde_json::from_str(json_str) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let choice = match chunk.choices.first() {
            Some(c) => c,
            None => continue,
        };

        let text_delta = choice.delta.content.clone().unwrap_or_default();
        let reasoning_delta = choice
            .delta
            .reasoning_content
            .clone()
            .or_else(|| choice.delta.reasoning.clone())
            .unwrap_or_default();
        let finish_reason = choice.finish_reason.clone();
        let tool_calls = choice
            .delta
            .tool_calls
            .as_ref()
            .map(|calls| map_tool_calls(calls))
            .unwrap_or_default();
        let usage = chunk.usage.map(|u| InferenceUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        chunks.push(Ok(InferenceStreamChunk {
            text_delta,
            reasoning_delta,
            model: chunk.model.clone(),
            finish_reason: finish_reason.clone(),
            usage: if finish_reason.is_some() { usage } else { None },
            tool_calls: if finish_reason.is_some() {
                tool_calls
            } else {
                vec![]
            },
        }));
    }

    if chunks.is_empty() {
        chunks.push(Ok(InferenceStreamChunk {
            text_delta: String::new(),
            reasoning_delta: String::new(),
            model: model_id.to_string(),
            finish_reason: Some("stop".to_string()),
            usage: None,
            tool_calls: vec![],
        }));
    }

    chunks
}

/// Validate a prompt string.
///
/// expect: "The system constructs and validates regulated LLM requests"
/// \[P9\] Motivating: Homeostatic Self-Regulation — input validation prevents token overconsumption
/// pre:  prompt is a valid &str
/// post: returns Err(Generation) if prompt is empty
/// post: returns Err(Generation) if prompt.len() > 1_000_000
#[must_use = "result must be used"]
pub fn validate_prompt(prompt: &str) -> Result<(), InferenceError> {
    if prompt.is_empty() {
        return Err(InferenceError::Generation("Prompt is empty".to_string()));
    }
    if prompt.len() > 1_000_000 {
        return Err(InferenceError::Generation("Prompt too long".to_string()));
    }
    Ok(())
}

/// Stream a chat completion from an OpenAI-compatible endpoint via SSE.
///
/// Shared helper used by all backends. Manages HTTP request, status handling,
/// and SSE parsing. Backends differ only in their Authorization header value
/// (Bearer vs Key) and base URL.
///
/// expect: "The system constructs and validates regulated LLM requests"
/// \[P9\] Motivating: Homeostatic Self-Regulation — shared streaming helper for all providers
/// pre:  client is a configured reqwest::Client
/// pre:  base_url and auth_header_value are non-empty
/// pre:  model and prompt are non-empty
/// post: returns Pin<Box<Stream<Item = `Result<InferenceStreamChunk, InferenceError>`> + Send>>
#[must_use]
pub fn stream_chat_completion(
    client: std::sync::Arc<reqwest::Client>,
    base_url: String,
    auth_header_value: String,
    model: String,
    prompt: String,
    params: LLMParameters,
    tools: Option<Vec<ChatToolDefinition>>,
) -> std::pin::Pin<
    Box<dyn futures_util::Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send>,
> {
    Box::pin(
        futures_util::stream::once(async move {
            let mut messages = Vec::with_capacity(2);
            if let Some(ref sys) = params.system_prompt {
                messages.push(ChatMessage::system(sys));
            }
            messages.push(ChatMessage::user(&prompt));
            let request =
                build_chat_request_messages(&model, messages, &params, Some(true), None, tools);

            let response = match client
                .post(format!("{}/v1/chat/completions", base_url))
                .header("Authorization", &auth_header_value)
                .json(&request)
                .send()
                .await
                .map_err(|e| InferenceError::Connection(e.to_string()))
            {
                Ok(r) => r,
                Err(e) => return vec![Err(e)],
            };

            let status = response.status();
            if !status.is_success() {
                let error_text = response.text().await.unwrap_or_default();
                return vec![Err(InferenceError::Connection(format!(
                    "streaming status {}: {}",
                    status, error_text
                )))];
            }

            let body = match response
                .text()
                .await
                .map_err(|e| InferenceError::Connection(e.to_string()))
            {
                Ok(b) => b,
                Err(e) => return vec![Err(e)],
            };

            parse_sse_stream(&body, &model)
        })
        .map(futures_util::stream::iter)
        .flatten(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: "Inference chat response deserialization works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates response normalization
    #[test]
    fn chat_response_deserializes_openai_format() {
        let raw = r#"{
            "id": "chatcmpl-847",
            "object": "chat.completion",
            "created": 1781219013,
            "model": "DI/google/gemma-4-9b-it",
            "system_fingerprint": "fp_deepinfra",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "The sun beat down."
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 11,
                "completion_tokens": 5,
                "total_tokens": 16
            }
        }"#;

        let resp: ChatResponse =
            serde_json::from_str(raw).expect("ChatResponse must deserialize OpenAI format");

        assert_eq!(resp.model, crate::model_constants::TEST_MODEL_SMALL);
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("The sun beat down.")
        );
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
        let usage = resp.usage.expect("usage present");
        assert_eq!(usage.prompt_tokens, 11);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 16);
    }

    /// expect: "ChatResponse handles null content (GLM-5.2 thinking mode, tool-call responses)"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — tolerates OpenAI spec-compliant null content
    #[test]
    fn chat_response_deserializes_null_content() {
        let raw = r#"{
            "model": "KC/z-ai/glm-5.2",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "reasoning_content": "The answer is 42."
                },
                "finish_reason": null
            }]
        }"#;

        let resp: ChatResponse =
            serde_json::from_str(raw).expect("ChatResponse must deserialize null content");

        assert_eq!(resp.choices.len(), 1);
        assert!(resp.choices[0].message.content.is_none());
        assert!(resp.choices[0].finish_reason.is_none());
        assert!(resp.usage.is_none());

        // chat_response_to_result should fall back to reasoning_content.
        let result = chat_response_to_result(resp).expect("result from null-content response");
        assert_eq!(result.text, "The answer is 42.");
        assert_eq!(result.finish_reason, "unknown");
        assert_eq!(result.usage.prompt_tokens, 0);
    }

    /// Anti-regression: when a thinking model emits BOTH `content` (the answer)
    /// and `reasoning_content` (the chain-of-thought), the reasoning must be
    /// carried on `InferenceResult.reasoning` separately and MUST NOT be folded
    /// into `text` (which would surface private CoT as the public answer and
    /// re-feed it into memory as the response).
    #[test]
    fn chat_result_surfaces_reasoning_separately_from_answer() {
        let raw = r#"{
            "model": "KC/z-ai/glm-5.2",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Paris",
                    "reasoning_content": "The user asked for France's capital."
                },
                "finish_reason": "stop"
            }]
        }"#;
        let resp: ChatResponse = serde_json::from_str(raw).expect("deserialize");
        let result = chat_response_to_result(resp).expect("result");
        assert_eq!(result.text, "Paris", "content is the answer, not the CoT");
        assert_eq!(
            result.reasoning.as_deref(),
            Some("The user asked for France's capital."),
            "reasoning_content must surface on .reasoning, not be dropped"
        );
    }

    /// Anti-regression (Cline #8636 class): reasoning deltas in an SSE stream
    /// must land on `InferenceStreamChunk.reasoning_delta`, never be silently
    /// dropped, and never leak into `text_delta`. Covers both DeepInfra-style
    /// `delta.reasoning_content` and Ollama-style `delta.reasoning`.
    #[test]
    fn stream_carries_reasoning_delta() {
        let body = r#"data: {"model":"KC/glm-5.2","choices":[{"delta":{"content":"","reasoning_content":"Let me think."}}]}
data: {"model":"KC/glm-5.2","choices":[{"delta":{"content":"Paris"}}]}
data: {"model":"KC/glm-5.2","choices":[{"delta":{},"finish_reason":"stop"}]}
data: [DONE]
"#;
        let chunks = parse_sse_stream(body, "KC/glm-5.2");
        let reasoning: String = chunks
            .iter()
            .filter_map(|c| c.as_ref().ok())
            .map(|c| c.reasoning_delta.clone())
            .collect();
        let text: String = chunks
            .iter()
            .filter_map(|c| c.as_ref().ok())
            .map(|c| c.text_delta.clone())
            .collect();
        assert!(
            reasoning.contains("Let me think."),
            "reasoning delta must survive"
        );
        assert!(
            !text.contains("Let me think."),
            "reasoning must not leak into text_delta"
        );
        assert!(text.contains("Paris"), "answer text must survive");

        // Ollama-style `delta.reasoning` must also map to reasoning_delta.
        let ollama = r#"data: {"model":"OM/qwen3","choices":[{"delta":{"reasoning":"deliberating"}}]}
data: {"model":"OM/qwen3","choices":[{"delta":{"content":"42"},"finish_reason":"stop"}]}
data: [DONE]
"#;
        let chunks = parse_sse_stream(ollama, "OM/qwen3");
        let reasoning: String = chunks
            .iter()
            .filter_map(|c| c.as_ref().ok())
            .map(|c| c.reasoning_delta.clone())
            .collect();
        assert!(
            reasoning.contains("deliberating"),
            "Ollama delta.reasoning must map to reasoning_delta"
        );
    }

    /// expect: "Inference chat request building works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates non-streaming request payload
    #[test]
    fn build_chat_request_stream_false() {
        let params = LLMParameters {
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            min_p: 0.0,
            typical_p: 0.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            max_tokens: 512,
            seed: None,
            disable_thinking: false,
            adapter: None,
            bypass_fusion: false,
            fusion_config: None,
            system_prompt: None,
        };
        let messages = vec![ChatMessage::user("Write a sentence.")];
        let req = build_chat_request_messages(
            crate::model_constants::TEST_MODEL_SMALL,
            messages,
            &params,
            Some(false),
            None,
            None::<Vec<ChatToolDefinition>>,
        );
        let json = serde_json::to_value(&req).expect("serialization must succeed");
        assert_eq!(json["stream"], serde_json::json!(false));
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Write a sentence.");
    }

    /// expect: "Inference prompt validation works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates prompt guardrails
    #[test]
    fn validate_prompt_rejects_invalid() {
        assert!(validate_prompt("").is_err());
        assert!(validate_prompt("hello").is_ok());
    }

    /// expect: "Inference thinking mode wire format works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates reasoning-mode suppression
    #[test]
    fn disable_thinking_maps_to_wire_format() {
        let params = LLMParameters {
            temperature: 0.3,
            top_p: 0.9,
            top_k: 40,
            min_p: 0.0,
            typical_p: 0.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            max_tokens: 256,
            seed: None,
            disable_thinking: true,
            adapter: None,
            bypass_fusion: false,
            fusion_config: None,
            system_prompt: None,
        };
        let messages = vec![ChatMessage::user("Summarize.")];
        let req = build_chat_request_messages(
            crate::model_constants::TEST_MODEL_SMALL,
            messages,
            &params,
            Some(true),
            None,
            None::<Vec<ChatToolDefinition>>,
        );
        let json = serde_json::to_value(&req).expect("serialization must succeed");
        assert_eq!(json["enable_thinking"], serde_json::json!(false));
    }

    /// expect: "Inference thinking mode omission works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates default reasoning-mode omission
    #[test]
    fn enable_thinking_omitted_when_true() {
        let params = LLMParameters {
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            min_p: 0.0,
            typical_p: 0.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            max_tokens: 512,
            seed: None,
            disable_thinking: false,
            adapter: None,
            bypass_fusion: false,
            fusion_config: None,
            system_prompt: None,
        };
        let messages = vec![ChatMessage::user("Hello.")];
        let req = build_chat_request_messages(
            crate::model_constants::TEST_MODEL_SMALL,
            messages,
            &params,
            Some(true),
            None,
            None::<Vec<ChatToolDefinition>>,
        );
        let json = serde_json::to_value(&req).expect("serialization must succeed");
        // enable_thinking should NOT appear in JSON when true (skip_serializing_if)
        assert!(json.get("enable_thinking").is_none());
    }

    // [P9] Motivating: Homeostatic Self-Regulation — input validation prevents token overconsumption
    // For any non-empty string ≤ 1_000_000 chars, validate_prompt returns Ok(()).
    // For empty string, returns Err. For strings > 1_000_000, returns Err.
    #[test]
    fn validate_prompt_contract() {
        // Empty → error
        assert!(validate_prompt("").is_err());

        // Normal prompts → ok
        assert!(validate_prompt("hello").is_ok());
        assert!(validate_prompt("a").is_ok());
        assert!(validate_prompt(&"x".repeat(1000)).is_ok());
        assert!(validate_prompt(&"x".repeat(1_000_000)).is_ok());

        // Overlong → error
        assert!(validate_prompt(&"x".repeat(1_000_001)).is_err());
    }
}
