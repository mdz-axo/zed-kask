//! `InferencePort` adapter over zed's `LanguageModel`.
//!
//! hKask's `InferencePort` is non-streaming (`generate() -> InferenceResult`).
//! Zed's `LanguageModel` streams (`stream_completion() -> BoxStream<CompletionEvent>`).
//! This adapter collects the stream into a single `InferenceResult`, mapping the
//! event types. Streaming is lost in this adapter — that's acceptable for the
//! ManifestExecutor cascade (which needs complete results for PDCA convergence),
//! and for MCP servers that already use the non-streaming `InferencePort`.
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
                        crate::model_resolution::resolve_model_names(
                            registry,
                            std::slice::from_ref(&override_name),
                            cx,
                        )
                        .0
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
                            // Observed per-call USD cost from the provider's
                            // `UsageUpdate` event (zed-kask D20 — the OpenAI-
                            // compatible and OpenRouter provider impls populate
                            // `TokenUsage.cost` from `usage.cost`/
                            // `estimated_cost`/`market_cost`). `None` when the
                            // provider reports no cost (Anthropic, Ollama, local).
                            let mut cost_usd: Option<f64> = None;

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
                                            // Zed's `LanguageModelToolUse.name` is the
                                            // tool name only, not a `server/tool` pair.
                                            // The `server` field is left empty to signal
                                            // "unknown server from zed bridge path".
                                            server: String::new(),
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
                                        cost_usd = token_usage.cost;
                                    }
                                    Ok(_) => {}
                                    Err(e) => {
                                        return Err(InferenceError::Generation(e.to_string()));
                                    }
                                }
                            }
                            let model_name = model.name().0.to_string();
                            // rJoule = USD: `cost_usd` is the observed per-call
                            // cost the provider reported in its `UsageUpdate` event
                            // (zed-kask D20), now that zed's `TokenUsage` carries
                            // `cost`. The manifest executor charges this to the
                            // rJoule budget via `BudgetTracker::charge_rjoule`.
                            // `None` when the provider reports no cost (Anthropic,
                            // Ollama, local) — free, not charged.
                            Ok(InferenceResult {
                                text,
                                model: model_name,
                                usage,
                                finish_reason,
                                token_probabilities: None,
                                tool_calls,
                                reasoning: if reasoning.is_empty() {
                                    None
                                } else {
                                    Some(reasoning)
                                },
                                cost_usd,
                            })
                        }
                    }
                }
                .await;

                if let Err(result) = req.reply.send(result) {
                    tracing::trace!(target: "hkask.inference", "inference reply dropped — caller cancelled");
                    let _ = result;
                }
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
// Embedding generation over OpenAI-compatible provider credentials.
//
// The port takes `(api_url, api_key)` resolved upfront from the bridge's
// `INFERENCE_PROVIDERS` table (env var) and makes raw
// `/embeddings` POSTs through the app's `HttpClient`. No GPUI access is
// needed at request time — credentials are resolved once at construction.
//
// The model string (e.g. `DeepInfra/Qwen/Qwen3-Embedding-0.6B`) is stripped
// of its provider prefix before being sent to the API — DeepInfra expects
// `Qwen/Qwen3-Embedding-0.6B`, not the prefixed form.

use hkask_types::EmbeddingGenerationError;
use http_client::{AsyncBody, HttpClient, Method, Request};
use serde::Deserialize;
use tokio::sync::mpsc;

use futures::AsyncReadExt;

/// Request sent to the tokio-side embedding executor.
struct EmbedRequest {
    /// The provider-prefixed model string (e.g. `DeepInfra/Qwen/Qwen3-Embedding-0.6B`).
    /// The prefix is stripped before the API call.
    model: String,
    /// Texts to embed.
    texts: Vec<String>,
    /// Reply channel.
    reply: oneshot::Sender<Result<Vec<Vec<f32>>, EmbeddingGenerationError>>,
}

/// Embedding generation port over OpenAI-compatible provider credentials.
///
/// Construct with `(api_url, api_key)` resolved from the bridge's
/// `INFERENCE_PROVIDERS` table and the app's `HttpClient`. The port is
/// `Send + Sync` — no GPUI access is needed at request time.
#[derive(Clone)]
pub struct LanguageModelEmbeddingPort {
    tx: mpsc::UnboundedSender<EmbedRequest>,
}

impl LanguageModelEmbeddingPort {
    /// Construct the port and spawn the receiver task on the tokio runtime.
    ///
    /// `api_url` is the OpenAI-compatible base URL (e.g.
    /// `https://api.deepinfra.com/v1/openai`). `api_key` is the bearer token.
    /// Both are resolved once at construction from `INFERENCE_PROVIDERS` +
    /// env var; no GPUI access is needed at request time. The `tokio_handle`
    /// is used to spawn the receiver task (obtained via
    /// `gpui_tokio::Tokio::handle(cx)` at the call site).
    pub fn new(
        api_url: String,
        api_key: String,
        http_client: Arc<dyn HttpClient>,
        tokio_handle: tokio::runtime::Handle,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<EmbedRequest>();

        // The receiver runs on the tokio runtime — no GPUI access needed.
        tokio_handle.spawn(async move {
            while let Some(req) = rx.recv().await {
                let http_client = http_client.clone();
                let api_url = api_url.clone();
                let api_key = api_key.clone();
                let result = async move {
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

                if let Err(result) = req.reply.send(result) {
                    tracing::trace!(target: "hkask.inference", "embedding reply dropped — caller cancelled");
                    let _ = result;
                }
            }
        });

        Self { tx }
    }

    /// Construct a port with no backing receiver task. Any `embed` call will
    /// return a `Connection` error (the channel is closed). For tests that
    /// construct a `RealMemoryPort` but never call embed.
    #[cfg(test)]
    pub fn for_tests() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel::<EmbedRequest>();
        // Drop `_rx` immediately so `embed` returns a channel-closed error.
        drop(_rx);
        Self { tx }
    }

    /// Construct a port whose `embed` calls are answered by `embed_fn`,
    /// which maps each input text to a vector. The receiver task runs on the
    /// provided tokio handle. For tests that exercise the end-to-end
    /// embedding recall path without a real HTTP call — the closure must
    /// produce vectors where similar texts have small cosine distance and
    /// dissimilar texts have large distance, so KNN `search` returns the
    /// right neighbors.
    #[cfg(test)]
    pub fn for_tests_with_embed_fn<F>(
        embed_fn: Arc<F>,
        tokio_handle: tokio::runtime::Handle,
    ) -> Self
    where
        F: Fn(&str) -> Vec<f32> + Send + Sync + 'static,
    {
        let (tx, mut rx) = mpsc::unbounded_channel::<EmbedRequest>();
        tokio_handle.spawn(async move {
            while let Some(req) = rx.recv().await {
                let vectors: Vec<Vec<f32>> = req.texts.iter().map(|t| embed_fn(t)).collect();
                // `embed` returns an error on empty input; mirror that here.
                let result = if vectors.is_empty() {
                    Err(EmbeddingGenerationError::EmptyResponse)
                } else {
                    Ok(vectors)
                };
                let _ = req.reply.send(result);
            }
        });
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
/// `RunPod/`, `KiloCode/`, `ollama/`).
/// Returns the bare model id. If no prefix is recognized, returns the
/// string unchanged (the API will reject it, which surfaces a clear error).
fn strip_provider_prefix(model: &str) -> String {
    // Long-form prefixes (case-insensitive). Order matters only for
    // overlapping prefixes; none overlap here.
    const LONG_FORM: &[&str] = &[
        "DeepInfra/",
        "fal.ai/",
        "RunPod/",
        "OpenRouter/",
        "KiloCode/",
        "ollama/",
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

// ── BridgeEditPredictionPort (D24) ───────────────────────────────────────────
//
// Routes edit-prediction FIM completions through the `LanguageModelRegistry`,
// reusing the same OpenRouter model + credentials the agent uses. Mirrors
// `LanguageModelEmbeddingPort` (raw HTTP POST via `HttpClient`) but targets
// `/completions` instead of `/embeddings`.
//
// Credentials (`api_url`, `api_key`) and the bare model id are resolved once at
// construction from `LanguageModelRegistry::resolve_model_names` + the model's
// `api_url()`/`api_key()` trait accessors (D24 overrides on `OpenRouterLanguageModel`).
// No GPUI access is needed at request time — the port is `Send + Sync`.

use edit_prediction::open_ai_compatible::KaskCompletionPort;
use hkask_inference::model_constants::DEFAULT_FALLBACK_MODEL;
use serde::Serialize;

/// Request sent to the tokio-side completion executor.
struct CompletionRequest {
    prompt: String,
    max_tokens: u32,
    stop_tokens: Vec<String>,
    reply: oneshot::Sender<Result<(String, String), anyhow::Error>>, // (text, request_id)
}

/// OpenAI-compatible completions response (wire format).
#[derive(Debug, Deserialize)]
struct RawCompletionResponseWire {
    id: Option<String>,
    choices: Vec<RawCompletionChoiceWire>,
}

#[derive(Debug, Deserialize)]
struct RawCompletionChoiceWire {
    text: Option<String>,
}

/// Edit-prediction port over OpenAI-compatible provider credentials resolved
/// from the `LanguageModelRegistry`.
///
/// Construct with `(api_url, api_key, model_id)` resolved from the registry
/// and the app's `HttpClient`. The port is `Send + Sync` — no GPUI access is
/// needed at request time.
#[derive(Clone)]
pub struct BridgeEditPredictionPort {
    tx: mpsc::UnboundedSender<CompletionRequest>,
}

impl BridgeEditPredictionPort {
    /// Construct the port and spawn the receiver task on the tokio runtime.
    ///
    /// `api_url` is the OpenAI-compatible base URL (e.g.
    /// `https://openrouter.ai/api/v1`). `api_key` is the bearer token.
    /// `model_id` is the bare model id (prefix stripped, e.g. `z-ai/glm-5.2`).
    pub fn new(
        api_url: String,
        api_key: String,
        model_id: String,
        http_client: Arc<dyn HttpClient>,
        tokio_handle: tokio::runtime::Handle,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<CompletionRequest>();

        tokio_handle.spawn(async move {
            while let Some(req) = rx.recv().await {
                let http_client = http_client.clone();
                let api_url = api_url.clone();
                let api_key = api_key.clone();
                let model_id = model_id.clone();
                let result = async move {
                    #[derive(Serialize)]
                    struct RequestBody<'a> {
                        model: &'a str,
                        prompt: &'a str,
                        max_tokens: u32,
                        stop: &'a [String],
                    }
                    let body = RequestBody {
                        model: &model_id,
                        prompt: &req.prompt,
                        max_tokens: req.max_tokens,
                        stop: &req.stop_tokens,
                    };
                    let body_bytes = serde_json::to_vec(&body).map_err(|e| {
                        anyhow::anyhow!("failed to serialize completion request: {e}")
                    })?;

                    let uri = format!("{api_url}/completions");
                    let request = Request::builder()
                        .method(Method::POST)
                        .uri(&uri)
                        .header("Content-Type", "application/json")
                        .header("Authorization", format!("Bearer {}", api_key.trim()))
                        .body(AsyncBody::from(body_bytes))
                        .map_err(|e| anyhow::anyhow!("failed to build completion request: {e}"))?;

                    let mut response = http_client
                        .send(request)
                        .await
                        .map_err(|e| anyhow::anyhow!("completion HTTP request failed: {e}"))?;
                    let status = response.status();

                    let mut body_text = String::new();
                    response
                        .body_mut()
                        .read_to_string(&mut body_text)
                        .await
                        .map_err(|e| anyhow::anyhow!("failed to read completion response: {e}"))?;

                    if !status.is_success() {
                        return Err(anyhow::anyhow!(
                            "completion request failed: {} - {}",
                            status,
                            body_text
                        ));
                    }

                    let parsed: RawCompletionResponseWire = serde_json::from_str(&body_text)
                        .map_err(|e| anyhow::anyhow!("failed to parse completion response: {e}"))?;
                    let text = parsed
                        .choices
                        .into_iter()
                        .next()
                        .and_then(|c| c.text)
                        .unwrap_or_default();
                    let request_id = parsed.id.unwrap_or_default();
                    Ok((text, request_id))
                }
                .await;

                if let Err(result) = req.reply.send(result) {
                    tracing::trace!(
                        target: "hkask.inference",
                        "completion reply dropped — caller cancelled"
                    );
                    let _ = result;
                }
            }
        });

        Self { tx }
    }

    /// Resolve the port from the `LanguageModelRegistry`.
    ///
    /// Looks up `DEFAULT_FALLBACK_MODEL` (e.g. `OpenRouter/z-ai/glm-5.2`)
    /// in the registry, extracts `api_url()` + `api_key()` from the resolved
    /// model, strips the provider prefix, and constructs the port.
    /// Returns `None` if the model cannot be resolved or has no `api_url`/`api_key`.
    pub fn from_registry(
        registry: &language_model::LanguageModelRegistry,
        http_client: Arc<dyn HttpClient>,
        tokio_handle: tokio::runtime::Handle,
        cx: &gpui::App,
    ) -> Option<Self> {
        let model = crate::model_resolution::resolve_model_names(
            registry,
            &[DEFAULT_FALLBACK_MODEL.to_string()],
            cx,
        )
        .0
        .into_values()
        .next()?;

        let api_url = model.api_url(cx)?;
        let api_key = model.api_key(cx)?;
        let model_id = strip_provider_prefix(DEFAULT_FALLBACK_MODEL);

        Some(Self::new(
            api_url,
            api_key,
            model_id,
            http_client,
            tokio_handle,
        ))
    }
}

impl KaskCompletionPort for BridgeEditPredictionPort {
    fn send_completion(
        &self,
        prompt: String,
        max_tokens: u32,
        stop_tokens: Vec<String>,
    ) -> futures::future::BoxFuture<'static, Result<(String, String), anyhow::Error>> {
        let (tx_reply, rx_reply) = oneshot::channel();
        let result = self
            .tx
            .send(CompletionRequest {
                prompt,
                max_tokens,
                stop_tokens,
                reply: tx_reply,
            })
            .map_err(|e| anyhow::anyhow!("completion port channel closed: {e}"));
        async move {
            result?;
            rx_reply
                .await
                .map_err(|e| anyhow::anyhow!("completion port reply dropped: {e}"))?
        }
        .boxed()
    }
}

// ── NoModelInferencePort ────────────────────────────────────────────────────
//
// An `InferencePort` that returns a clear "no default model configured" error
// on every call. Used to start the `InferenceIpcServer` unconditionally —
// even when no default `LanguageModel` is configured at startup — so MCP
// server child processes receive `HKASK_INFERENCE_SOCKET` and route inference
// through the IPC bridge rather than falling back to a standalone `MediaRouter`.
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

    // ── model_override propagation tests ──
    //
    // These tests verify that generate_with_model and generate_with_messages
    // propagate the model_override parameter into the InferenceRequest that's
    // sent through the channel. The full GPUI-side resolution (looking up the
    // model from LanguageModelRegistry) is tested via integration tests that
    // require a TestAppContext; here we test the contract: the override
    // reaches the channel, not silently dropped.
    //
    // Regression for the audit cycle 7 finding: LanguageModelInferencePort
    // silently dropped model_override (passed None to generate_with_messages).
    // The fix wires model_override through to InferenceRequest; these tests
    // pin that wiring so a future refactor can't silently revert it.

    #[tokio::test]
    async fn generate_with_model_propagates_override_to_channel() {
        // Construct a port with a channel we control. We don't spawn the
        // receiver task — instead we recv from the channel ourselves to
        // inspect the InferenceRequest.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<InferenceRequest>();
        let port = LanguageModelInferencePort { tx };

        // Call generate_with_model with a model override. The receiver task
        // isn't running, so the channel send will succeed (unbounded) and
        // the reply will never come — but we only need to inspect the
        // InferenceRequest, not the result.
        let future = port.generate_with_model(
            "test prompt",
            &LLMParameters::default(),
            Some("openrouter/z-ai/glm-5.2"),
            None,
        );
        // Drive the future far enough to send the request. Since the channel
        // is unbounded, the send succeeds immediately. The future then parks
        // on rx_reply.await — we drop the future before that.
        tokio::select! {
            biased;
            req = rx.recv() => {
                let req = req.expect("should have received an InferenceRequest");
                assert_eq!(
                    req.model_override.as_deref(),
                    Some("openrouter/z-ai/glm-5.2"),
                    "generate_with_model must propagate model_override to the channel"
                );
            }
            _ = future => {
                panic!("future should not complete — receiver task isn't running");
            }
        }
    }

    #[tokio::test]
    async fn generate_with_model_propagates_none_override_to_channel() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<InferenceRequest>();
        let port = LanguageModelInferencePort { tx };

        let future = port.generate_with_model("test prompt", &LLMParameters::default(), None, None);
        tokio::select! {
            biased;
            req = rx.recv() => {
                let req = req.expect("should have received an InferenceRequest");
                assert_eq!(
                    req.model_override, None,
                    "generate_with_model with None override must send None to the channel"
                );
            }
            _ = future => {
                panic!("future should not complete — receiver task isn't running");
            }
        }
    }

    #[tokio::test]
    async fn generate_with_messages_propagates_override_to_channel() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<InferenceRequest>();
        let port = LanguageModelInferencePort { tx };

        let messages = vec![ChatMessage::user("hello".to_string())];
        let future = port.generate_with_messages(
            &messages,
            &LLMParameters::default(),
            Some("DeepInfra/Qwen/Qwen3-Embedding-0.6B"),
            None,
        );
        tokio::select! {
            biased;
            req = rx.recv() => {
                let req = req.expect("should have received an InferenceRequest");
                assert_eq!(
                    req.model_override.as_deref(),
                    Some("DeepInfra/Qwen/Qwen3-Embedding-0.6B"),
                    "generate_with_messages must propagate model_override to the channel"
                );
            }
            _ = future => {
                panic!("future should not complete — receiver task isn't running");
            }
        }
    }
}
