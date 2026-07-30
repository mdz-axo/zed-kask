//! `InferencePort` adapter over zed's `LanguageModel`.
//!
//! hKask's `InferencePort` is non-streaming (`generate() -> InferenceResult`).
//! Zed's `LanguageModel` streams (`stream_completion() -> BoxStream<CompletionEvent>`).
//! This adapter collects the stream into a single `InferenceResult`, mapping the
//! event types. Streaming is lost in this adapter — that's acceptable for the
//! ManifestExecutor cascade (which needs complete results for PDCA convergence),
//! and for MCP servers that already use the non-streaming `InferenceRouter`.
//!
//! `AsyncApp` is not `Send` (GPUI's `ForegroundExecutor` holds `Rc`-based state),
//! so the bridge uses a channel: trait methods send a request to a GPUI-side task
//! that holds the `AsyncApp` and executes the streaming completion, collecting the
//! result and sending it back. The adapter struct itself only holds a channel
//! sender (`Send + Sync`).

use std::sync::Arc;

use futures_util::{FutureExt, StreamExt};
use gpui::AsyncApp;
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferencePort, InferenceResult,
    InferenceUsage, StructuredToolCall,
};
use language_model::LanguageModel;
use language_model_core::{
    LanguageModelCompletionEvent, LanguageModelImage, LanguageModelRequest,
    LanguageModelRequestMessage, LanguageModelRequestTool, LanguageModelToolChoice,
    LanguageModelToolUseInput, MessageContent, Role, StopReason,
};
use tokio::sync::oneshot;

/// Request sent from the tokio side (trait method) to the GPUI side (executor).
struct InferenceRequest {
    request: LanguageModelRequest,
    /// Provider-prefixed model name (e.g. "openrouter/z-ai/glm-5.2").
    /// When `Some`, the receiver resolves the model from
    /// `LanguageModelRegistry` and dispatches to it instead of the
    /// default model. When `None` or resolution fails, the default
    /// model is used.
    model_override: Option<String>,
    reply: oneshot::Sender<Result<InferenceResult, InferenceError>>,
}

/// `InferencePort` implementation over zed's `LanguageModel`.
///
/// Collects the streaming completion into a single `InferenceResult`.
/// The model is selected at construction time — one adapter instance per model.
///
/// The adapter holds only a channel sender (`Send + Sync`); the actual inference
/// call happens on the GPUI side via a spawned task that owns the `AsyncApp`.
pub struct LanguageModelInferencePort {
    tx: tokio::sync::mpsc::UnboundedSender<InferenceRequest>,
}

impl LanguageModelInferencePort {
    /// Construct the adapter and spawn the GPUI-side receiver task.
    ///
    /// The receiver task runs on the GPUI foreground executor and processes
    /// inference requests. Drop the returned `Task` to stop it.
    pub fn new(model: Arc<dyn LanguageModel>, cx: AsyncApp) -> (Self, gpui::Task<()>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<InferenceRequest>();
        let model_for_task = model.clone();

        let task = cx.spawn(async move |cx| {
            while let Some(req) = rx.recv().await {
                // Resolve the model: use the override if provided, else the default.
                let model = if let Some(ref override_name) = req.model_override {
                    let override_name = override_name.clone();
                    let resolved = cx.update(|cx| {
                        let registry = language_model::LanguageModelRegistry::read_global(cx);
                        crate::resolve_fusion_models(registry, &[override_name.clone()], cx)
                            .into_values()
                            .next()
                    });
                    match resolved {
                        Some(m) => m,
                        None => {
                            tracing::warn!(
                                target: "hkask.inference",
                                model_override = %override_name.as_str(),
                                "model_override could not be resolved from LanguageModelRegistry — \
                                 falling back to the default model. Ensure the model is configured \
                                 in Settings → AI → LLM Providers."
                            );
                            model_for_task.clone()
                        }
                    }
                } else {
                    model_for_task.clone()
                };
                let cx = cx.clone();
                // Run on the foreground executor — stream_completion needs &AsyncApp
                // which is not Send, so it can't go to background_spawn.
                let result = async move {
                    let stream_result = model
                        .stream_completion(req.request, &cx)
                        .await
                        .map_err(|e| InferenceError::Connection(e.to_string()));

                    match stream_result {
                        Err(e) => Err(e),
                        Ok(mut stream) => {
                            let mut text = String::new();
                            let mut reasoning = String::new();
                            let mut tool_calls = Vec::new();
                            let mut finish_reason = "stop".to_string();
                            let mut usage = InferenceUsage::default();

                            while let Some(event) = stream.next().await {
                                match event {
                                    Ok(LanguageModelCompletionEvent::Text(delta)) => {
                                        text.push_str(&delta);
                                    }
                                    Ok(LanguageModelCompletionEvent::Thinking {
                                        text: thinking,
                                        ..
                                    }) => {
                                        reasoning.push_str(&thinking);
                                    }
                                    Ok(LanguageModelCompletionEvent::ToolUse(tool_use))
                                        if tool_use.is_input_complete =>
                                    {
                                        let args = match &tool_use.input {
                                            LanguageModelToolUseInput::Json(json) => json.clone(),
                                            LanguageModelToolUseInput::Text(text) => {
                                                serde_json::from_str(text)
                                                    .unwrap_or(serde_json::Value::Null)
                                            }
                                        };
                                        tool_calls.push(StructuredToolCall {
                                            server: tool_use.name.to_string(),
                                            tool: tool_use.name.to_string(),
                                            args,
                                            call_id: Some(tool_use.id.to_string()),
                                        });
                                    }
                                    Ok(LanguageModelCompletionEvent::Stop(reason)) => {
                                        finish_reason = match reason {
                                            StopReason::EndTurn => "stop",
                                            StopReason::MaxTokens => "length",
                                            StopReason::ToolUse => "tool_calls",
                                            StopReason::Refusal => "refusal",
                                        }
                                        .to_string();
                                    }
                                    Ok(LanguageModelCompletionEvent::UsageUpdate(token_usage)) => {
                                        usage = InferenceUsage {
                                            prompt_tokens: token_usage.input_tokens as u32,
                                            completion_tokens: token_usage.output_tokens as u32,
                                            total_tokens: (token_usage.input_tokens
                                                + token_usage.output_tokens)
                                                as u32,
                                        };
                                    }
                                    Ok(_) => {}
                                    Err(e) => {
                                        return Err(InferenceError::Generation(e.to_string()));
                                    }
                                }
                            }

                            Ok(InferenceResult {
                                text,
                                model: model.name().0.to_string(),
                                usage,
                                finish_reason,
                                token_probabilities: None,
                                tool_calls,
                                reasoning: if reasoning.is_empty() {
                                    None
                                } else {
                                    Some(reasoning)
                                },
                            })
                        }
                    }
                }
                .await;

                let _ = req.reply.send(result);
            }
        });

        (Self { tx }, task)
    }

    fn build_request(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> LanguageModelRequest {
        self.build_request_with_images(messages, &[], parameters, tools)
    }

    /// Build a multimodal request with optional base64-encoded images.
    ///
    /// When `images` is non-empty, the user message content array includes
    /// `MessageContent::Image` parts alongside the text prompt. This is the
    /// OpenAI multimodal content-array format that zed's `LanguageModel`
    /// implementations (Anthropic, OpenAI, etc.) already handle.
    fn build_request_with_images(
        &self,
        messages: &[ChatMessage],
        images: &[String],
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> LanguageModelRequest {
        // Images should only be attached to the last user message, not every
        // user message in the conversation. This prevents image duplication in
        // multi-turn conversations.
        let last_user_idx = messages
            .iter()
            .rposition(|m| m.role.as_str() != "system" && m.role.as_str() != "assistant");

        let req_messages: Vec<LanguageModelRequestMessage> = messages
            .iter()
            .enumerate()
            .map(|(idx, m)| {
                let role = match m.role.as_str() {
                    "system" => Role::System,
                    "assistant" => Role::Assistant,
                    _ => Role::User,
                };
                // Attach images only to the last user message.
                let content =
                    if role == Role::User && !images.is_empty() && Some(idx) == last_user_idx {
                        let mut parts = Vec::with_capacity(1 + images.len());
                        parts.push(MessageContent::Text(m.content.clone()));
                        for img in images {
                            parts.push(MessageContent::Image(LanguageModelImage {
                                source: img.clone().into(),
                            }));
                        }
                        parts
                    } else {
                        vec![MessageContent::Text(m.content.clone())]
                    };
                LanguageModelRequestMessage {
                    role,
                    content,
                    cache: false,
                    reasoning_details: None,
                }
            })
            .collect();

        let req_tools: Vec<LanguageModelRequestTool> = tools
            .unwrap_or(&[])
            .iter()
            .map(|t| {
                LanguageModelRequestTool::function(
                    t.function.name.clone(),
                    t.function.description.clone(),
                    t.function.parameters.clone(),
                    false,
                )
            })
            .collect();

        LanguageModelRequest {
            messages: req_messages,
            tools: req_tools,
            temperature: Some(parameters.temperature),
            tool_choice: if tools.is_some() {
                Some(LanguageModelToolChoice::Auto)
            } else {
                None
            },
            ..Default::default()
        }
    }
}

impl InferencePort for LanguageModelInferencePort {
    fn generate(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let messages = vec![ChatMessage::user(prompt.to_string())];
        self.generate_with_messages(&messages, parameters, None, tools)
    }

    fn generate_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let messages = vec![ChatMessage::user(prompt.to_string())];
        self.generate_with_messages(&messages, parameters, model_override, tools)
    }

    fn generate_with_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let request = self.build_request(messages, parameters, tools);
        let model_override = model_override.map(|s| s.to_string());
        let (tx_reply, rx_reply) = oneshot::channel();
        async move {
            self.tx
                .send(InferenceRequest {
                    request,
                    model_override,
                    reply: tx_reply,
                })
                .map_err(|e| InferenceError::Connection(e.to_string()))?;
            rx_reply
                .await
                .map_err(|e| InferenceError::Connection(e.to_string()))?
        }
        .boxed()
    }

    /// Vision inference — send base64-encoded images to a multimodal model.
    ///
    /// Builds a multimodal `LanguageModelRequest` with `MessageContent::Image`
    /// parts and dispatches it through the same channel-based path as text
    /// inference. The model must be vision-capable; if it isn't, the upstream
    /// provider will return an error.
    fn generate_vision(
        &self,
        prompt: &str,
        images: &[String],
        parameters: &LLMParameters,
        model_override: Option<&str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let messages = vec![ChatMessage::user(prompt.to_string())];
        let request = self.build_request_with_images(&messages, images, parameters, None);
        let model_override = model_override.map(|s| s.to_string());
        let (tx_reply, rx_reply) = oneshot::channel();
        async move {
            self.tx
                .send(InferenceRequest {
                    request,
                    model_override,
                    reply: tx_reply,
                })
                .map_err(|e| InferenceError::Connection(e.to_string()))?;
            rx_reply
                .await
                .map_err(|e| InferenceError::Connection(e.to_string()))?
        }
        .boxed()
    }
}

// ── LanguageModelEmbeddingPort ───────────────────────────────────────────────
//
// Embedding generation over zed's `LanguageModel` credentials.
//
// zed's `LanguageModel` trait has no `embed` method — only chat completion.
// But OpenAI-compatible providers (DeepInfra, OpenRouter, etc.) expose a
// `/embeddings` endpoint at the same base URL as `/chat/completions`, using
// the same API key. This port resolves `(api_url, api_key)` from the zed
// `LanguageModel` (via the `api_url()` and `api_key()` trait methods added in
// this fork) and makes a raw OpenAI-compatible `/embeddings` POST.
//
// This replaces hKask's standalone `EmbeddingRouter`, which resolved
// credentials from `InferenceConfig::from_env()` (env vars) and bypassed
// zed's `LanguageModelRegistry` + keychain. Routing embeddings through zed's
// credential resolution means a user who configures DeepInfra in
// Settings → AI → LLM Providers gets working embeddings without also
// setting `DEEPINFRA_API_KEY` in the environment.
//
// The model string (e.g. `DeepInfra/Qwen/Qwen3-Embedding-0.6B`) is stripped
// of its provider prefix before being sent to the API — DeepInfra expects
// `Qwen/Qwen3-Embedding-0.6B`, not the prefixed form. Both long-form
// (`DeepInfra/`) and 2-letter (`DI/`) prefixes are accepted, case-insensitive,
// so either convention works.
//
// Like `LanguageModelInferencePort`, this struct holds only a channel sender
// (`Send + Sync`); the actual HTTP call happens on the GPUI side via a
// spawned task that owns the `AsyncApp` (needed to read the model's
// `api_key()` / `api_url()`). Per the `.rules` trap "Cross-thread GPUI
// communication uses channels, not `AsyncApp` handles".

use hkask_types::EmbeddingGenerationError;
use http_client::{AsyncBody, HttpClient, Method, Request};
use serde::Deserialize;
use tokio::sync::mpsc;

use futures::AsyncReadExt;

/// Request sent from the tokio side to the GPUI-side embedding executor.
struct EmbedRequest {
    /// The provider-prefixed model string (e.g. `DeepInfra/Qwen/Qwen3-Embedding-0.6B`).
    /// The prefix is stripped before the API call.
    model: String,
    /// Texts to embed.
    texts: Vec<String>,
    /// Reply channel.
    reply: oneshot::Sender<Result<Vec<Vec<f32>>, EmbeddingGenerationError>>,
}

/// Embedding generation port over zed's `LanguageModel` credentials.
///
/// Construct with a zed `LanguageModel` (any OpenAI-compatible model from
/// the same provider as the embedding model — only its `api_url()` and
/// `api_key()` are used) and the app's `HttpClient`. Drop the returned
/// `Task` to stop the GPUI-side receiver.
#[derive(Clone)]
pub struct LanguageModelEmbeddingPort {
    tx: mpsc::UnboundedSender<EmbedRequest>,
}

impl LanguageModelEmbeddingPort {
    /// Construct the port and spawn the GPUI-side receiver task.
    ///
    /// `credential_model` is a zed `LanguageModel` whose provider matches the
    /// embedding model's provider prefix. Only its `api_url()` and `api_key()`
    /// are read — the model itself is never used for chat. A convenient choice
    /// is the provider's `default_model(cx)`, but any model from the same
    /// provider works.
    pub fn new(
        credential_model: Arc<dyn LanguageModel>,
        http_client: Arc<dyn HttpClient>,
        cx: AsyncApp,
    ) -> (Self, gpui::Task<()>) {
        let (tx, mut rx) = mpsc::unbounded_channel::<EmbedRequest>();

        let task = cx.spawn(async move |cx| {
            while let Some(req) = rx.recv().await {
                let model = credential_model.clone();
                let http_client = http_client.clone();
                let cx = cx.clone();
                let result = async move {
                    // Resolve credentials from the zed LanguageModel. These
                    // read the provider's State entity on the GPUI side.
                    // `AsyncApp::update` returns `R` directly (not Result) —
                    // it panics if the app was dropped, which is fine here
                    // (the port is owned by the app).
                    let (api_url, api_key) = cx.update(|cx| {
                        let api_url = model.api_url(cx);
                        let api_key = model.api_key(cx);
                        (api_url, api_key)
                    });

                    let api_url = api_url.ok_or_else(|| {
                        EmbeddingGenerationError::Connection(format!(
                            "Embedding model '{}' — provider '{}' does not expose an api_url \
                             through zed's LanguageModel trait. Only OpenAI-compatible providers \
                             (DeepInfra, OpenRouter, etc.) support embeddings. Add the provider \
                             in Settings → AI → LLM Providers.",
                            req.model,
                            model.provider_name().0
                        ))
                    })?;

                    let api_key = api_key.ok_or_else(|| {
                        EmbeddingGenerationError::Connection(format!(
                            "Embedding model '{}' — no API key configured for provider '{}' \
                             in zed. Add the API key in Settings → AI → LLM Providers, \
                             or set the corresponding env var (e.g. DEEPINFRA_API_KEY).",
                            req.model,
                            model.provider_name().0
                        ))
                    })?;

                    // Strip the provider prefix (case-insensitive). The
                    // API expects the bare model id.
                    let model_id = strip_provider_prefix(&req.model);

                    // Build and send the OpenAI-compatible /embeddings request.
                    let body = serde_json::json!({
                        "model": model_id,
                        "input": req.texts,
                    });
                    let body_bytes = serde_json::to_vec(&body).map_err(|e| {
                        EmbeddingGenerationError::Json(format!(
                            "failed to serialize embedding request: {e}"
                        ))
                    })?;

                    let uri = format!("{api_url}/embeddings");
                    let request = Request::builder()
                        .method(Method::POST)
                        .uri(&uri)
                        .header("Content-Type", "application/json")
                        .header("Authorization", format!("Bearer {}", api_key.trim()))
                        .body(AsyncBody::from_bytes(body_bytes.into()))
                        .map_err(|e| {
                            EmbeddingGenerationError::Connection(format!(
                                "failed to build embedding request: {e}"
                            ))
                        })?;

                    let mut response = http_client.send(request).await.map_err(|e| {
                        EmbeddingGenerationError::Connection(format!(
                            "embedding HTTP request failed: {e}"
                        ))
                    })?;

                    let status = response.status();
                    let mut body_text = String::new();
                    response
                        .body_mut()
                        .read_to_string(&mut body_text)
                        .await
                        .map_err(|e| {
                            EmbeddingGenerationError::Connection(format!(
                                "failed to read embedding response body: {e}"
                            ))
                        })?;

                    if !status.is_success() {
                        return Err(EmbeddingGenerationError::Api(status.as_u16(), body_text));
                    }

                    let parsed: OpenAiEmbedResponse =
                        serde_json::from_str(&body_text).map_err(|e| {
                            EmbeddingGenerationError::Json(format!(
                                "failed to parse embedding response: {e}"
                            ))
                        })?;

                    let embeddings: Vec<Vec<f32>> =
                        parsed.data.into_iter().map(|d| d.embedding).collect();

                    if embeddings.is_empty() {
                        return Err(EmbeddingGenerationError::EmptyResponse);
                    }

                    Ok(embeddings)
                }
                .await;

                let _ = req.reply.send(result);
            }
        });

        (Self { tx }, task)
    }

    /// Construct a port with no backing GPUI task. Any `embed` call will
    /// return a `Connection` error (the channel is closed). For tests that
    /// construct a `RealMemoryPort` but never call embed.
    #[cfg(test)]
    pub fn for_tests() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel::<EmbedRequest>();
        // Drop `_rx` immediately so `embed` returns a channel-closed error.
        drop(_rx);
        Self { tx }
    }

    /// Generate embeddings for a batch of texts.
    ///
    /// `model` is the provider-prefixed model string (e.g.
    /// `DeepInfra/Qwen/Qwen3-Embedding-0.6B`). The prefix is stripped
    /// before the API call.
    pub async fn embed(
        &self,
        model: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, EmbeddingGenerationError> {
        if texts.is_empty() {
            return Err(EmbeddingGenerationError::EmptyResponse);
        }
        let (tx_reply, rx_reply) = oneshot::channel();
        self.tx
            .send(EmbedRequest {
                model: model.to_string(),
                texts: texts.to_vec(),
                reply: tx_reply,
            })
            .map_err(|e| {
                EmbeddingGenerationError::Connection(format!("embedding port channel closed: {e}"))
            })?;
        rx_reply.await.map_err(|e| {
            EmbeddingGenerationError::Connection(format!("embedding port reply dropped: {e}"))
        })?
    }
}

/// Strip the provider prefix from a model string, case-insensitive.
///
/// Accepts long-form prefixes (`DeepInfra/`, `OpenRouter/`, `fal.ai/`,
/// `Together AI/`, `RunPod/`, `KiloCode/`, `ollama/`, `Cline/`).
/// Returns the bare model id. If no prefix is recognized, returns the
/// string unchanged (the API will reject it, which surfaces a clear error).
fn strip_provider_prefix(model: &str) -> String {
    // Long-form prefixes (case-insensitive). Order matters only for
    // overlapping prefixes; none overlap here.
    const LONG_FORM: &[&str] = &[
        "DeepInfra/",
        "fal.ai/",
        "Together AI/",
        "RunPod/",
        "OpenRouter/",
        "KiloCode/",
        "ollama/",
        "Cline/",
    ];
    for prefix in LONG_FORM {
        if let Some(rest) = model.strip_prefix(prefix) {
            return rest.to_string();
        }
        // Case-insensitive match (e.g. "deepinfra/..." or "DEEPINFRA/...").
        if model.len() >= prefix.len() && model[..prefix.len()].eq_ignore_ascii_case(prefix) {
            return model[prefix.len()..].to_string();
        }
    }

    // No recognized prefix — return as-is. The API will reject an unknown
    // model, which surfaces a clear error to the operator.
    model.to_string()
}

/// OpenAI-compatible embedding response (wire format).
#[derive(Debug, Deserialize)]
struct OpenAiEmbedResponse {
    data: Vec<OpenAiEmbeddingData>,
}

// ── NoModelInferencePort ────────────────────────────────────────────────────
//
// An `InferencePort` that returns a clear "no default model configured" error
// on every call. Used to start the `InferenceIpcServer` unconditionally —
// even when no default `LanguageModel` is configured at startup — so MCP
// server child processes receive `HKASK_INFERENCE_SOCKET` and route inference
// through the IPC bridge rather than falling back to `InferenceRouter::from_env()`.
//
// Without this, the `else` branch of the model-dependent wiring block (in
// `crates/zed/src/main.rs`) left `INFERENCE_SOCKET_PATH` unset, forcing the
// curator and other MCP servers into the env-var/keychain fallback path.
// That fallback reads from the `hkask` keychain namespace (via
// `hkask_keystore::resolve`), which is empty in zed-kask because inference
// keys live in zed's `CredentialsProvider` under `kask://credentials/<key>`.
// The result was a silent "API key not configured" error that operators
// could not trace back to the missing IPC socket.
//
// This port closes that gap: the IPC server starts with a no-op port, MCP
// servers connect to the socket, and any inference request returns a
// diagnostic error naming the remediation (configure a default model). When
// the deferred task later observes a default model, it replaces this port
// with a real `LanguageModelInferencePort` (the `OnceLock`-based hooks are
// not used here — the IPC server holds an `Arc<dyn InferencePort>` that
// can be swapped on re-wiring). For the initial implementation, the port
// is constructed once at startup; a future enhancement can make it
// upgradeable when the model registry populates.

/// An `InferencePort` that rejects every request with a "no default model"
/// error. See the module-level comment for the rationale.
pub struct NoModelInferencePort;

impl InferencePort for NoModelInferencePort {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &LLMParameters,
        _tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        Box::pin(async {
            Err(InferenceError::Generation(
                "No default LanguageModel configured — configure one in Settings → AI \
                 so inference routed through the zed IPC bridge can dispatch to it. \
                 Until then, this MCP server cannot run inference."
                    .to_string(),
            ))
        })
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingData {
    embedding: Vec<f32>,
}

#[cfg(test)]
mod embedding_tests {
    use super::*;

    #[test]
    fn strip_prefix_long_form() {
        assert_eq!(
            strip_provider_prefix("DeepInfra/Qwen/Qwen3-Embedding-0.6B"),
            "Qwen/Qwen3-Embedding-0.6B"
        );
        assert_eq!(
            strip_provider_prefix("OpenRouter/qwen/qwen3-embedding-0.6b"),
            "qwen/qwen3-embedding-0.6b"
        );
    }

    #[test]
    fn strip_prefix_case_insensitive() {
        assert_eq!(
            strip_provider_prefix("deepinfra/Qwen/Qwen3-Embedding-0.6B"),
            "Qwen/Qwen3-Embedding-0.6B"
        );
        assert_eq!(
            strip_provider_prefix("DEEPINFRA/Qwen/Qwen3-Embedding-0.6B"),
            "Qwen/Qwen3-Embedding-0.6B"
        );
    }

    #[test]
    fn strip_prefix_no_prefix_returns_unchanged() {
        assert_eq!(
            strip_provider_prefix("Qwen/Qwen3-Embedding-0.6B"),
            "Qwen/Qwen3-Embedding-0.6B"
        );
        assert_eq!(strip_provider_prefix("qwen3:8b"), "qwen3:8b");
    }

    #[test]
    fn strip_prefix_unknown_prefix_returns_unchanged() {
        assert_eq!(strip_provider_prefix("XX/some-model"), "XX/some-model");
    }
}
