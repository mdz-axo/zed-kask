//! `InferencePort` adapter over zed's `LanguageModel`.
//!
//! Zed's `LanguageModel` streams (`stream_completion() -> BoxStream<CompletionEvent>`).
//! This adapter has two paths:
//! - **Non-streaming** (`generate`): collects the stream into a single `InferenceResult`.
//!   Used by MCP servers and code that needs the complete result.
//! - **Streaming** (`generate_stream`): forwards `InferenceStreamChunk`s as they arrive.
//!   Used by skill execution for live thinking traces.
//!
//! `AsyncApp` is not `Send` (GPUI's `ForegroundExecutor` holds `Rc`-based state),
//! so the bridge uses channels: trait methods send a request to a GPUI-side task
//! that holds the `AsyncApp` and executes the streaming completion. The adapter
//! struct itself only holds channel senders (`Send + Sync`).

use std::sync::Arc;

use futures_util::{FutureExt, StreamExt};
use gpui::AsyncApp;
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferencePort, InferenceResult,
    InferenceStreamChunk, InferenceUsage, StructuredToolCall,
};
use language_model::LanguageModel;
use language_model_core::{
    LanguageModelCompletionError, LanguageModelCompletionEvent, LanguageModelImage,
    LanguageModelRequest, LanguageModelRequestMessage, LanguageModelRequestTool,
    LanguageModelToolChoice, LanguageModelToolUseInput, MessageContent, Role, StopReason,
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

/// Streaming request — forwards `InferenceStreamChunk`s as they arrive
/// instead of collecting the full result. Used by `generate_stream`.
struct StreamInferenceRequest {
    request: LanguageModelRequest,
    model_override: Option<String>,
    reply: tokio::sync::mpsc::UnboundedSender<Result<InferenceStreamChunk, InferenceError>>,
}

/// Shared accumulator for stream events — used by both `handle_non_streaming`
/// (which builds an `InferenceResult` from the accumulated state) and
/// `handle_streaming` (which forwards text/thinking deltas immediately but
/// accumulates metadata for the final chunk).
///
/// `Text` and `Thinking` events are handled by the caller (collected or
/// forwarded) — this struct handles `ToolUse`, `Stop`, and `UsageUpdate`,
/// which are identical in both paths.
struct StreamAccumulator {
    model_name: String,
    text: String,
    reasoning: String,
    tool_calls: Vec<StructuredToolCall>,
    finish_reason: String,
    usage: InferenceUsage,
    cost_usd: Option<f64>,
}

impl StreamAccumulator {
    fn new(model_name: String) -> Self {
        Self {
            model_name,
            text: String::new(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
            usage: InferenceUsage::default(),
            cost_usd: None,
        }
    }

    /// Process any `LanguageModelCompletionEvent`. In the streaming path, the
    /// caller filters `Text`/`Thinking` (forwarding them immediately) and passes
    /// only metadata events here; in the non-streaming path, all events are
    /// passed. The `Text`/`Thinking` arms accumulate into `self.text`/
    /// `self.reasoning` for `into_result()` (non-streaming) and are dead in the
    /// streaming path (`into_final_chunk()` doesn't read them). Returns `Err`
    /// on stream errors.
    fn process_event(
        &mut self,
        event: Result<LanguageModelCompletionEvent, LanguageModelCompletionError>,
    ) -> Result<(), InferenceError> {
        match event {
            Ok(LanguageModelCompletionEvent::Text(delta)) => {
                self.text.push_str(&delta);
            }
            Ok(LanguageModelCompletionEvent::Thinking { text, .. }) => {
                self.reasoning.push_str(&text);
            }
            Ok(LanguageModelCompletionEvent::ToolUse(tool_use)) if tool_use.is_input_complete => {
                let args = match &tool_use.input {
                    LanguageModelToolUseInput::Json(json) => json.clone(),
                    LanguageModelToolUseInput::Text(text) => {
                        serde_json::from_str(text).unwrap_or(serde_json::Value::Null)
                    }
                };
                self.tool_calls.push(StructuredToolCall {
                    server: String::new(),
                    tool: tool_use.name.to_string(),
                    args,
                    call_id: Some(tool_use.id.to_string()),
                });
            }
            Ok(LanguageModelCompletionEvent::Stop(reason)) => {
                self.finish_reason = match reason {
                    StopReason::EndTurn => "stop",
                    StopReason::MaxTokens => "length",
                    StopReason::ToolUse => "tool_calls",
                    StopReason::Refusal => "refusal",
                }
                .to_string();
            }
            Ok(LanguageModelCompletionEvent::UsageUpdate(token_usage)) => {
                self.usage = InferenceUsage {
                    prompt_tokens: token_usage.input_tokens as u32,
                    completion_tokens: token_usage.output_tokens as u32,
                    total_tokens: (token_usage.input_tokens + token_usage.output_tokens) as u32,
                };
                self.cost_usd = token_usage.cost;
            }
            Ok(_) => {}
            Err(e) => return Err(InferenceError::Generation(e.to_string())),
        }
        Ok(())
    }

    /// Build a complete `InferenceResult` from the accumulated state.
    fn into_result(self) -> InferenceResult {
        InferenceResult {
            text: self.text,
            model: self.model_name,
            usage: self.usage,
            finish_reason: self.finish_reason,
            tool_calls: self.tool_calls,
            reasoning: if self.reasoning.is_empty() {
                None
            } else {
                Some(self.reasoning)
            },
            cost_usd: self.cost_usd,
        }
    }

    /// Build a final `InferenceStreamChunk` carrying accumulated metadata.
    fn into_final_chunk(self) -> InferenceStreamChunk {
        InferenceStreamChunk {
            text_delta: String::new(),
            reasoning_delta: String::new(),
            model: self.model_name,
            finish_reason: Some(self.finish_reason),
            usage: Some(self.usage),
            tool_calls: self.tool_calls,
            cost_usd: self.cost_usd,
        }
    }
}

/// `InferencePort` implementation over zed's `LanguageModel`.
///
/// Has two paths: non-streaming (`generate`) collects the stream into a
/// single `InferenceResult`; streaming (`generate_stream`) forwards chunks
/// as they arrive for live thinking traces. The model is selected at
/// construction time — one adapter instance per model.
///
/// The adapter holds only channel senders (`Send + Sync`); the actual inference
/// call happens on the GPUI side via a spawned task that owns the `AsyncApp`.
pub struct LanguageModelInferencePort {
    tx: tokio::sync::mpsc::UnboundedSender<InferenceRequest>,
    stream_tx: tokio::sync::mpsc::UnboundedSender<StreamInferenceRequest>,
}

impl LanguageModelInferencePort {
    /// Construct the adapter and spawn the GPUI-side receiver task.
    ///
    /// The receiver task runs on the GPUI foreground executor and processes
    /// inference requests. Drop the returned `Task` to stop it.
    pub fn new(model: Arc<dyn LanguageModel>, cx: AsyncApp) -> (Self, gpui::Task<()>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<InferenceRequest>();
        let (stream_tx, mut stream_rx) =
            tokio::sync::mpsc::unbounded_channel::<StreamInferenceRequest>();
        let model_for_task = model.clone();

        let task = cx.spawn(async move |cx| {
            // Process both channels on the GPUI foreground executor.
            // `stream_completion` needs `&AsyncApp` which is not `Send`,
            // so both must run here. Streaming requests are spawned as
            // concurrent tasks so multiple skill execution can stream
            // inference concurrently. Awaiting each inline serialized all
            // skill execution behind whichever request the loop picked up first,
            // defeating the parallel fan-out in `skill_bundle`.
            loop {
                tokio::select! {
                    Some(req) = rx.recv() => {
                        let model = model_for_task.clone();
                        cx.spawn(async move |cx| {
                            Self::handle_non_streaming(req, &model, cx).await;
                        }).detach();
                    }
                    Some(req) = stream_rx.recv() => {
                        let model = model_for_task.clone();
                        cx.spawn(async move |cx| {
                            Self::handle_streaming(req, &model, cx).await;
                        }).detach();
                    }
                    else => break,
                }
            }
        });

        (Self { tx, stream_tx }, task)
    }

    /// Resolve a model, using the override if provided, else the default.
    async fn resolve_model(
        model_for_task: &Arc<dyn LanguageModel>,
        override_name: Option<&str>,
        cx: &AsyncApp,
    ) -> Arc<dyn LanguageModel> {
        if let Some(override_name) = override_name {
            let override_name = override_name.to_string();
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
        }
    }

    /// Handle a non-streaming inference request — collects the full stream
    /// into a single `InferenceResult` before replying.
    async fn handle_non_streaming(
        req: InferenceRequest,
        model_for_task: &Arc<dyn LanguageModel>,
        cx: &AsyncApp,
    ) {
        let model = Self::resolve_model(model_for_task, req.model_override.as_deref(), cx).await;
        let cx = cx.clone();
        let request = req.request;
        let result = async move {
            let stream_result = model
                .stream_completion(request, &cx)
                .await
                .map_err(|e| InferenceError::Connection(e.to_string()));

            match stream_result {
                Err(e) => Err(e),
                Ok(mut stream) => {
                    let mut acc = StreamAccumulator::new(model.name().0.to_string());
                    while let Some(event) = stream.next().await {
                        if let Err(e) = acc.process_event(event) {
                            return Err(e);
                        }
                    }
                    Ok(acc.into_result())
                }
            }
        }
        .await;

        if let Err(result) = req.reply.send(result) {
            tracing::trace!(target: "hkask.inference", "inference reply dropped — caller cancelled");
            let _ = result;
        }
    }

    /// Handle a streaming inference request — forwards `InferenceStreamChunk`s
    /// as they arrive so the caller (skill execution) can emit live thinking
    /// traces. The final chunk carries the accumulated `usage`, `cost_usd`,
    /// and `finish_reason`.
    async fn handle_streaming(
        req: StreamInferenceRequest,
        model_for_task: &Arc<dyn LanguageModel>,
        cx: &AsyncApp,
    ) {
        let model = Self::resolve_model(model_for_task, req.model_override.as_deref(), cx).await;
        let cx = cx.clone();
        let request = req.request;
        let reply = req.reply;

        async move {
            let stream_result = model
                .stream_completion(request, &cx)
                .await
                .map_err(|e| InferenceError::Connection(e.to_string()));

            match stream_result {
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
                Ok(mut stream) => {
                    let model_name = model.name().0.to_string();
                    let mut acc = StreamAccumulator::new(model_name.clone());
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(LanguageModelCompletionEvent::Text(delta)) => {
                                let _ = reply.send(Ok(InferenceStreamChunk {
                                    text_delta: delta,
                                    reasoning_delta: String::new(),
                                    model: model_name.clone(),
                                    finish_reason: None,
                                    usage: None,
                                    tool_calls: Vec::new(),
                                    cost_usd: None,
                                }));
                            }
                            Ok(LanguageModelCompletionEvent::Thinking {
                                text: thinking, ..
                            }) => {
                                let _ = reply.send(Ok(InferenceStreamChunk {
                                    text_delta: String::new(),
                                    reasoning_delta: thinking,
                                    model: model_name.clone(),
                                    finish_reason: None,
                                    usage: None,
                                    tool_calls: Vec::new(),
                                    cost_usd: None,
                                }));
                            }
                            Ok(other) => {
                                if let Err(e) = acc.process_event(Ok(other)) {
                                    let _ = reply.send(Err(e));
                                    return;
                                }
                            }
                            Err(e) => {
                                let _ = reply.send(Err(InferenceError::Generation(e.to_string())));
                                return;
                            }
                        }
                    }

                    // Final chunk carries the accumulated metadata.
                    let _ = reply.send(Ok(acc.into_final_chunk()));
                }
            }
        }
        .await;
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
            max_tokens: None,
            // zed-kask: D25 — when a structured-output tool (emit_result) is offered,
            // force the model to call it via tool_choice: Any ("required" in
            // OpenAI's API). With Auto, the model may return prose instead of
            // calling the tool, and parse_json_response fails on the non-JSON
            // text. Any guarantees the model emits a tool call, so the executor
            // extracts args from tool_calls[0] instead of parsing free text.
            // This is the LangGraph/Swarm enforce-at-the-API-layer pattern:
            // the output contract is enforced by the provider, not by a
            // best-effort JSON extractor.
            tool_choice: if tools.is_some() {
                Some(LanguageModelToolChoice::Any)
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
        // The rendered template is a system prompt (role definition, output
        // format, constraints) — not a user message. Sending it as `system`
        // gives it the semantic weight providers reserve for system-level
        // directives (stronger instruction adherence, better tool-call
        // compliance). The minimal user message triggers generation — some
        // providers require at least one user message to produce output.
        let messages = vec![
            ChatMessage::system(prompt.to_string()),
            ChatMessage::user("Execute the instructions above.".to_string()),
        ];
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
        let messages = vec![
            ChatMessage::system(prompt.to_string()),
            ChatMessage::user("Execute the instructions above.".to_string()),
        ];
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
        let messages = vec![
            ChatMessage::system(prompt.to_string()),
            ChatMessage::user("Execute the instructions above.".to_string()),
        ];
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

    /// Streaming override — forwards `InferenceStreamChunk`s as they arrive
    /// from zed's `LanguageModel::stream_completion`. This is the live
    /// thinking-trace path used by skill execution: `reasoning_delta` chunks
    /// appear in the thinking trace in real time, not after the full response
    /// completes.
    ///
    /// Without this override, the default trait impl wraps `generate()` in
    /// `stream::once` — the entire response is collected before any chunk is
    /// emitted, so the thinking trace appears all at once after the LLM
    /// finishes, not live during generation.
    fn generate_stream(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn futures_util::Stream<Item = Result<InferenceStreamChunk, InferenceError>>
                + Send
                + '_,
        >,
    > {
        let messages = vec![
            ChatMessage::system(prompt.to_string()),
            ChatMessage::user("Execute the instructions above.".to_string()),
        ];
        let request = self.build_request(&messages, parameters, tools);
        let (tx_stream, rx_stream) =
            tokio::sync::mpsc::unbounded_channel::<Result<InferenceStreamChunk, InferenceError>>();

        let stream_tx = self.stream_tx.clone();
        let send_result = stream_tx.send(StreamInferenceRequest {
            request,
            model_override: None,
            reply: tx_stream,
        });

        if send_result.is_err() {
            return Box::pin(futures_util::stream::once(async {
                Err(InferenceError::Connection(
                    "inference stream channel closed".to_string(),
                ))
            }));
        }

        // Convert the tokio mpsc receiver into a futures_util::Stream by
        // polling it asynchronously. This avoids adding a tokio-stream
        // dependency.
        Box::pin(futures_util::stream::unfold(
            rx_stream,
            |mut rx| async move {
                match rx.recv().await {
                    Some(chunk) => Some((chunk, rx)),
                    None => None,
                }
            },
        ))
    }

    /// Stream with optional model override.
    ///
    /// Overrides the default trait impl so a `model_override` threads through
    /// the streaming channel (`StreamInferenceRequest.model_override`) instead
    /// of falling back to non-streaming `generate_with_model` — the default impl
    /// collects the full response before emitting any chunk, losing the live
    /// thinking trace the cascade relies on. When `model_override` is `None`,
    /// this delegates to `generate_stream` (the common path).
    fn generate_stream_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn futures_util::Stream<Item = Result<InferenceStreamChunk, InferenceError>>
                + Send
                + '_,
        >,
    > {
        let Some(model_override) = model_override else {
            return self.generate_stream(prompt, parameters, tools);
        };
        let messages = vec![
            ChatMessage::system(prompt.to_string()),
            ChatMessage::user("Execute the instructions above.".to_string()),
        ];
        let request = self.build_request(&messages, parameters, tools);
        let model_override = model_override.to_string();
        let (tx_stream, rx_stream) =
            tokio::sync::mpsc::unbounded_channel::<Result<InferenceStreamChunk, InferenceError>>();
        let stream_tx = self.stream_tx.clone();
        let send_result = stream_tx.send(StreamInferenceRequest {
            request,
            model_override: Some(model_override),
            reply: tx_stream,
        });
        if send_result.is_err() {
            return Box::pin(futures_util::stream::once(async {
                Err(InferenceError::Connection(
                    "inference stream channel closed".to_string(),
                ))
            }));
        }
        Box::pin(futures_util::stream::unfold(
            rx_stream,
            |mut rx| async move {
                match rx.recv().await {
                    Some(chunk) => Some((chunk, rx)),
                    None => None,
                }
            },
        ))
    }

    /// F11: Streaming variant of `generate_with_messages`.
    ///
    /// The cascade calls `generate_stream_with_messages` (not
    /// `generate_stream`) to pass the full message array — prior turns,
    /// memory snippets, and the rendered template as a system message.
    /// Without this override, the default trait impl wraps
    /// `generate_with_messages` in `stream::once`, collecting the full
    /// response before emitting any chunk. That loses the live thinking
    /// trace the cascade relies on for user feedback and steering.
    ///
    /// This override routes through the same streaming channel as
    /// `generate_stream` and `generate_stream_with_model`.
    fn generate_stream_with_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn futures_util::Stream<Item = Result<InferenceStreamChunk, InferenceError>>
                + Send
                + '_,
        >,
    > {
        let request = self.build_request(messages, parameters, tools);
        let model_override = model_override.map(|s| s.to_string());
        let (tx_stream, rx_stream) =
            tokio::sync::mpsc::unbounded_channel::<Result<InferenceStreamChunk, InferenceError>>();
        let stream_tx = self.stream_tx.clone();
        let send_result = stream_tx.send(StreamInferenceRequest {
            request,
            model_override,
            reply: tx_stream,
        });
        if send_result.is_err() {
            return Box::pin(futures_util::stream::once(async {
                Err(InferenceError::Connection(
                    "inference stream channel closed".to_string(),
                ))
            }));
        }
        Box::pin(futures_util::stream::unfold(
            rx_stream,
            |mut rx| async move {
                match rx.recv().await {
                    Some(chunk) => Some((chunk, rx)),
                    None => None,
                }
            },
        ))
    }
}
//
// Embedding generation over OpenAI-compatible provider credentials.
//
// The port takes `(api_url, api_key)` resolved upfront from the bridge's
// `INFERENCE_PROVIDERS` table (env var) and makes raw
// `/embeddings` POSTs through the app's `HttpClient`. No GPUI access is
// needed at request time — credentials are resolved once at construction.
//
// The model string (e.g. `OpenRouter/qwen/qwen3-embedding`) is stripped
// of its provider prefix before being sent to the API — the provider expects
// the bare model id, not the prefixed form.

use hkask_types::EmbeddingGenerationError;
use http_client::{AsyncBody, HttpClient, Method, Request};
use serde::Deserialize;
use tokio::sync::mpsc;

use futures::AsyncReadExt;

/// Request sent to the tokio-side embedding executor.
struct EmbedRequest {
    /// The provider-prefixed model string (e.g. `OpenRouter/qwen/qwen3-embedding`).
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
    /// `https://openrouter.ai/api/v1`). `api_key` is the bearer token.
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
    /// `DEFAULT_EMBEDDING_MODEL`). The prefix is stripped
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
/// Accepts long-form prefixes (`OpenRouter/`,
/// `RunPod/`, `ollama/`).
/// Returns the bare model id. If no prefix is recognized, returns the
/// string unchanged (the API will reject it, which surfaces a clear error).
fn strip_provider_prefix(model: &str) -> String {
    // Long-form prefixes (case-insensitive). Order matters only for
    // overlapping prefixes; none overlap here.
    const LONG_FORM: &[&str] = &["RunPod/", "OpenRouter/", "ollama/"];
    for prefix in LONG_FORM {
        if let Some(rest) = model.strip_prefix(prefix) {
            return rest.to_string();
        }
        // Case-insensitive match (e.g. "openrouter/..." or "OPENROUTER/...").
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
