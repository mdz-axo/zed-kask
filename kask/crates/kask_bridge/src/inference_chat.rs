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
use std::time::Duration;

use futures_util::{FutureExt, StreamExt, TryFutureExt};
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
use tokio::sync::{Semaphore, oneshot};

/// The app-wide inference port, published by `wire_kask_inference_stack`.
///
/// Consumers that wire BEFORE the inference stack exists — the memory
/// ingest path is the case that matters: `RealMemoryPort` is constructed
/// before `wire_kask_inference_stack` runs, but its per-turn chunk tagging
/// needs inference — read this lazily per turn instead of holding a
/// construction-time handle. The mutex (not a `OnceLock`) is deliberate:
/// the inference stack re-wires when the default model resolves late, and
/// the re-wire must be able to replace the port.
static GLOBAL_INFERENCE_PORT: std::sync::Mutex<Option<std::sync::Arc<dyn InferencePort>>> =
    std::sync::Mutex::new(None);

/// Publish the app-wide inference port. Called by `wire_kask_inference_stack`
/// on every successful (re)wire. Poisoned-lock recovery mirrors
/// `set_inference_timeout_secs` — a poisoned global is recovered via
/// `into_inner`, never silently dropped.
pub fn set_global_inference_port(port: std::sync::Arc<dyn InferencePort>) {
    let mut guard = match GLOBAL_INFERENCE_PORT.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!(
                target: "hkask.inference",
                "GLOBAL_INFERENCE_PORT mutex poisoned — recovering via into_inner"
            );
            poisoned.into_inner()
        }
    };
    *guard = Some(port);
}

/// Read the app-wide inference port, if the inference stack has wired one.
/// Returns a clone of the `Arc` — the caller holds it only for the duration
/// of its request.
pub fn global_inference_port() -> Option<std::sync::Arc<dyn InferencePort>> {
    match GLOBAL_INFERENCE_PORT.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => {
            tracing::warn!(
                target: "hkask.inference",
                "GLOBAL_INFERENCE_PORT mutex poisoned on read — recovering via into_inner"
            );
            poisoned.into_inner().clone()
        }
    }
}

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
///
/// Health-tracking fields (`in_flight`, `max_concurrency`, `recent_timeouts`)
/// are shared between the adapter and the receiver task via `Arc`. The
/// `InferenceHealthSource` impl reads these so the cybernetics loop can sense
/// inference saturation and timeout storms — closing the blind-feedback-loop
/// gap that caused `signal_count=0` during the 300s timeout storm.
///
/// `Clone` is derived so the composition root can hold one clone for the
/// `InferencePort` trait object and another for the `InferenceHealthSource`
/// trait object — both share the same `Arc`-backed health counters.
#[derive(Clone)]
pub struct LanguageModelInferencePort {
    tx: tokio::sync::mpsc::UnboundedSender<InferenceRequest>,
    stream_tx: tokio::sync::mpsc::UnboundedSender<StreamInferenceRequest>,
    in_flight: Arc<std::sync::atomic::AtomicUsize>,
    max_concurrency: Arc<std::sync::atomic::AtomicUsize>,
    recent_timeouts: Arc<std::sync::Mutex<Vec<std::time::Instant>>>,
}

impl LanguageModelInferencePort {
    /// Construct the adapter and spawn the GPUI-side receiver task.
    ///
    /// The receiver task runs on the GPUI foreground executor and processes
    /// inference requests. Drop the returned `Task` to stop it.
    ///
    /// `inference_timeout` bounds the wall-clock time for a single inference
    /// call (stream establishment + event drain). A hung provider stalls the
    /// request indefinitely without this — the cybernetics variety check
    /// flagged this as a critical gap (disturbance class D2: provider timeout,
    /// no response). `Duration::ZERO` disables the timeout (legacy behavior).
    ///
    /// `max_concurrency` bounds the number of in-flight inference calls the
    /// receiver will dispatch concurrently. Without this, a caller firing on
    /// a fixed cadence (e.g. the agent thread's retry loop, or a background
    /// curator turn) accumulates unbounded detached tasks on the GPUI
    /// foreground executor. Each task polls `stream_completion`, which needs
    /// the foreground executor to make progress; with 100+ tasks polling, the
    /// executor thrashes and no task makes progress, so they all time out.
    /// This is the deep-module puzzle: the `InferencePort` interface promises
    /// "call generate, get a result" but the implementation leaked unbounded
    /// foreground tasks. The semaphore is the enforcement point for the
    /// `kask.general.max_concurrency` setting (default 96).
    pub fn new(
        model: Arc<dyn LanguageModel>,
        inference_timeout: Duration,
        max_concurrency: usize,
        cx: AsyncApp,
    ) -> (Self, gpui::Task<()>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<InferenceRequest>();
        let (stream_tx, mut stream_rx) =
            tokio::sync::mpsc::unbounded_channel::<StreamInferenceRequest>();
        let model_for_task = model.clone();
        let timeout_for_task = inference_timeout;
        let concurrency_semaphore = Arc::new(Semaphore::new(max_concurrency.max(1)));
        let in_flight = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let max_concurrency_arc =
            Arc::new(std::sync::atomic::AtomicUsize::new(max_concurrency.max(1)));
        let recent_timeouts: Arc<std::sync::Mutex<Vec<std::time::Instant>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));

        let task = cx.spawn({
            // Clone the health counters into the receiver task scope so each
            // spawned request task can increment/decrement in-flight and push
            // timeout timestamps.
            let in_flight = in_flight.clone();
            let recent_timeouts = recent_timeouts.clone();
            async move |cx| {
            // Process both channels on the GPUI foreground executor.
            // `stream_completion` needs `&AsyncApp` which is not `Send`,
            // so both must run here. Streaming requests are spawned as
            // concurrent tasks so multiple skill execution can stream
            // inference concurrently. Awaiting each inline serialized all
            // skill execution behind whichever request the loop picked up first,
            // defeating the parallel fan-out in `skill_bundle`.
            //
            // Each spawned task acquires a permit from `concurrency_semaphore`
            // before dispatching to `stream_completion`. This bounds the
            // in-flight count to `max_concurrency`, preventing the foreground
            // executor congestion that caused the 300s timeout storm. The
            // permit is held for the lifetime of the task (including stream
            // drain) and released on drop.
            loop {
                tokio::select! {
                    Some(req) = rx.recv() => {
                        let model = model_for_task.clone();
                        let timeout = timeout_for_task;
                        let semaphore = concurrency_semaphore.clone();
                        let in_flight = in_flight.clone();
                        let recent_timeouts = recent_timeouts.clone();
                        cx.spawn(async move |cx| {
                            let _permit = match semaphore.acquire().await {
                                Ok(permit) => permit,
                                Err(_) => {
                                    tracing::warn!(
                                        target: "hkask.inference",
                                        "inference concurrency semaphore closed — request dropped"
                                    );
                                    return;
                                }
                            };
                            in_flight.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            Self::handle_non_streaming(req, &model, timeout, cx, &recent_timeouts).await;
                            in_flight.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                        }).detach();
                    }
                    Some(req) = stream_rx.recv() => {
                        let model = model_for_task.clone();
                        let timeout = timeout_for_task;
                        let semaphore = concurrency_semaphore.clone();
                        let in_flight = in_flight.clone();
                        let recent_timeouts = recent_timeouts.clone();
                        cx.spawn(async move |cx| {
                            let _permit = match semaphore.acquire().await {
                                Ok(permit) => permit,
                                Err(_) => {
                                    tracing::warn!(
                                        target: "hkask.inference",
                                        "inference concurrency semaphore closed — stream request dropped"
                                    );
                                    return;
                                }
                            };
                            in_flight.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            Self::handle_streaming(req, &model, timeout, cx, &recent_timeouts).await;
                            in_flight.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                        }).detach();
                    }
                    else => break,
                }
            }
            }
        });

        (
            Self {
                tx,
                stream_tx,
                in_flight,
                max_concurrency: max_concurrency_arc,
                recent_timeouts,
            },
            task,
        )
    }

    /// Resolve a model, using the override if provided, else the default.
    ///
    /// Returns `None` when an explicit override cannot be resolved — the
    /// caller must surface that as a typed error, never silently substitute
    /// the default model: the default is usually a text model, and a vision
    /// override resolved to a text model drops the images and returns
    /// garbage/empty output that reads like an endpoint failure (observed:
    /// ollama OCR overrides "failing" while the local endpoint was fine).
    async fn resolve_model(
        model_for_task: &Arc<dyn LanguageModel>,
        override_name: Option<&str>,
        cx: &AsyncApp,
    ) -> Option<Arc<dyn LanguageModel>> {
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
                Some(m) => Some(m),
                None => {
                    tracing::warn!(
                        target: "hkask.inference",
                        model_override = %override_name.as_str(),
                        "model_override could not be resolved from LanguageModelRegistry — \
                         replying with a typed error instead of substituting the default \
                         model. Ensure the model is configured in Settings → AI → LLM Providers."
                    );
                    None
                }
            }
        } else {
            Some(model_for_task.clone())
        }
    }

    /// Handle a non-streaming inference request — collects the full stream
    /// into a single `InferenceResult` before replying.
    async fn handle_non_streaming(
        req: InferenceRequest,
        model_for_task: &Arc<dyn LanguageModel>,
        inference_timeout: Duration,
        cx: &AsyncApp,
        recent_timeouts: &Arc<std::sync::Mutex<Vec<std::time::Instant>>>,
    ) {
        let model =
            match Self::resolve_model(model_for_task, req.model_override.as_deref(), cx).await {
                Some(model) => model,
                None => {
                    let name = req.model_override.clone().unwrap_or_default();
                    let result = Err(InferenceError::Model(format!(
                        "model_override '{name}' not found in the LanguageModelRegistry — \
                     not falling back to the default model (a vision override resolved to \
                     a text model drops images silently). Ensure the model is configured \
                     in Settings → AI → LLM Providers."
                    )));
                    if req.reply.send(result).is_err() {
                        tracing::trace!(
                            target: "hkask.inference",
                            "inference reply dropped — caller cancelled"
                        );
                    }
                    return;
                }
            };
        let cx = cx.clone();
        let request = req.request;
        let result = async move {
            let stream_future = model
                .stream_completion(request, &cx)
                .map_err(|e| InferenceError::Connection(e.to_string()));

            // Apply the wall-clock timeout if non-zero. A hung provider
            // stalls the request indefinitely without this — the cybernetics
            // variety check flagged this as a critical gap (D2: provider
            // timeout, no response). `Duration::ZERO` disables (legacy).
            //
            // Uses `BackgroundExecutor::timer` (not `tokio::time::timeout`)
            // because this future runs on the GPUI foreground executor, not
            // the tokio runtime. `tokio::time::timeout` creates a
            // `tokio::time::Sleep` that registers with the tokio reactor's
            // timer wheel at poll time — if no tokio reactor is entered on
            // the current thread (which is the case on the GPUI foreground
            // executor), it panics with "there is no reactor running."
            // `BackgroundExecutor::timer` uses the GPUI scheduler's clock,
            // which is the same executor that polls this future.
            let stream_result = if inference_timeout.is_zero() {
                stream_future.await
            } else {
                let timer = cx.background_executor().timer(inference_timeout);
                futures_util::pin_mut!(stream_future);
                futures_util::pin_mut!(timer);
                match futures_util::future::select(timer, stream_future).await {
                    futures_util::future::Either::Left(_) => {
                        tracing::warn!(
                            target: "hkask.inference",
                            timeout_secs = inference_timeout.as_secs(),
                            "Inference stream establishment timed out — returning Connection error"
                        );
                        recent_timeouts
                            .lock()
                            .unwrap()
                            .push(std::time::Instant::now());
                        Err(InferenceError::Connection(format!(
                            "inference timed out after {}s",
                            inference_timeout.as_secs()
                        )))
                    }
                    futures_util::future::Either::Right((result, _)) => result,
                }
            };

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
        inference_timeout: Duration,
        cx: &AsyncApp,
        recent_timeouts: &Arc<std::sync::Mutex<Vec<std::time::Instant>>>,
    ) {
        let model = match Self::resolve_model(model_for_task, req.model_override.as_deref(), cx)
            .await
        {
            Some(model) => model,
            None => {
                let name = req.model_override.clone().unwrap_or_default();
                let error = InferenceError::Model(format!(
                    "model_override '{name}' not found in the LanguageModelRegistry — \
                     not falling back to the default model (a vision override resolved to \
                     a text model drops images silently). Ensure the model is configured \
                     in Settings → AI → LLM Providers."
                ));
                if req.reply.send(Err(error)).is_err() {
                    tracing::trace!(
                        target: "hkask.inference",
                        "streaming inference reply dropped — caller cancelled before first event"
                    );
                }
                return;
            }
        };
        let cx = cx.clone();
        let request = req.request;
        let reply = req.reply;

        async move {
            let stream_future = model
                .stream_completion(request, &cx)
                .map_err(|e| InferenceError::Connection(e.to_string()));

            // Apply the wall-clock timeout if non-zero. Same rationale as
            // `handle_non_streaming` — a hung provider stalls the stream
            // indefinitely without this. Uses `BackgroundExecutor::timer`
            // (not `tokio::time::timeout`) because this future runs on the
            // GPUI foreground executor — see `handle_non_streaming` for
            // the full explanation.
            let stream_result = if inference_timeout.is_zero() {
                stream_future.await
            } else {
                let timer = cx.background_executor().timer(inference_timeout);
                futures_util::pin_mut!(stream_future);
                futures_util::pin_mut!(timer);
                match futures_util::future::select(timer, stream_future).await {
                    futures_util::future::Either::Left(_) => {
                        tracing::warn!(
                            target: "hkask.inference",
                            timeout_secs = inference_timeout.as_secs(),
                            "Streaming inference stream establishment timed out — returning Connection error"
                        );
                        recent_timeouts.lock().unwrap_or_else(|e| e.into_inner()).push(std::time::Instant::now());
                        Err(InferenceError::Connection(format!(
                            "inference timed out after {}s",
                            inference_timeout.as_secs()
                        )))
                    }
                    futures_util::future::Either::Right((result, _)) => result,
                }
            };

            match stream_result {
                Err(e) => {
                    // Send failure means the receiver dropped (caller cancelled
                    // the stream). No point continuing — return early to stop
                    // processing events for a dead consumer and avoid wasting
                    // billed LLM tokens.
                    if reply.send(Err(e)).is_err() {
                        tracing::trace!(
                            target: "hkask.inference",
                            "streaming inference reply dropped — caller cancelled before first event"
                        );
                    }
                }
                Ok(mut stream) => {
                    let model_name = model.name().0.to_string();
                    let mut acc = StreamAccumulator::new(model_name.clone());
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(LanguageModelCompletionEvent::Text(delta)) => {
                                if reply.send(Ok(InferenceStreamChunk {
                                    text_delta: delta,
                                    reasoning_delta: String::new(),
                                    model: model_name.clone(),
                                    finish_reason: None,
                                    usage: None,
                                    tool_calls: Vec::new(),
                                    cost_usd: None,
                                })).is_err() {
                                    tracing::trace!(
                                        target: "hkask.inference",
                                        "streaming inference reply dropped — caller cancelled, stopping event processing"
                                    );
                                    return;
                                }
                            }
                            Ok(LanguageModelCompletionEvent::Thinking {
                                text: thinking, ..
                            }) => {
                                if reply.send(Ok(InferenceStreamChunk {
                                    text_delta: String::new(),
                                    reasoning_delta: thinking,
                                    model: model_name.clone(),
                                    finish_reason: None,
                                    usage: None,
                                    tool_calls: Vec::new(),
                                    cost_usd: None,
                                })).is_err() {
                                    tracing::trace!(
                                        target: "hkask.inference",
                                        "streaming inference reply dropped — caller cancelled, stopping event processing"
                                    );
                                    return;
                                }
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
        // multi-turn conversations. Positive matching on "user" avoids
        // incorrectly attaching images to "tool" role messages (ChatMessage
        // supports 4 roles: system, user, assistant, tool).
        let last_user_idx = messages.iter().rposition(|m| m.role.as_str() == "user");

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
            thinking_allowed: parameters.thinking_allowed,
            // The sole emit_result tool denotes the structured-output protocol.
            // Ordinary tools are capabilities, not a requirement to act again;
            // forcing them would prevent the agent loop from finishing.
            tool_choice: match tools.unwrap_or(&[]) {
                [] => None,
                [tool] if tool.function.name == "emit_result" => Some(LanguageModelToolChoice::Any),
                _ => Some(LanguageModelToolChoice::Auto),
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

/// Window for recent timeout tracking — timeouts older than this are evicted
/// on each read. 5 minutes matches the cybernetics loop's tick cadence (10s)
/// × 30 ticks, so the sensor sees a storm of ~30 timeouts before evicting.
const RECENT_TIMEOUT_WINDOW: Duration = Duration::from_secs(300);

#[async_trait::async_trait]
impl hkask_regulation::InferenceHealthSource for LanguageModelInferencePort {
    async fn in_flight(&self) -> usize {
        self.in_flight.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn max_concurrency(&self) -> usize {
        self.max_concurrency
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn recent_timeout_count(&self) -> u64 {
        let now = std::time::Instant::now();
        let mut timeouts = self
            .recent_timeouts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Evict timeouts older than the window. This keeps the Vec bounded —
        // a long-running storm produces at most (rate × window) entries.
        timeouts.retain(|t| now.duration_since(*t) < RECENT_TIMEOUT_WINDOW);
        timeouts.len() as u64
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
// curator and other MCP servers into the env-var-only fallback path.
// API keys are injected as env vars by `build_mcp_server_env` (which reads
// from zed's `CredentialsProvider` under `kask://credentials/<key>`), so
// without the socket env var the server starts but inference calls fail.
// The result was a silent "IPC bridge not configured" error that operators
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

#[cfg(test)]
mod tests {
    use hkask_types::ChatMessage;
    use hkask_types::ChatToolDefinition;
    use hkask_types::ChatToolFunction;
    use hkask_types::InferencePort;
    use hkask_types::template::LLMParameters;
    use language_model::fake_provider::FakeLanguageModel;
    use language_model_core::LanguageModelToolChoice;
    use std::sync::Arc;
    use std::time::Duration;

    // ── build_request_with_images: image attachment targeting ───────────
    //
    // Images must attach only to the last "user" role message, never to
    // "tool" role messages. The old negation matching (`!= "system" &&
    // != "assistant"`) would match "tool" as a user message and attach
    // images to it. Positive matching on `== "user"` fixes this.
    //
    // We test via `build_request` which delegates to
    // `build_request_with_images` with an empty images slice. To test the
    // image-attachment logic directly, we need to verify the `rposition`
    // predicate. Since `build_request_with_images` is private, we test the
    // behavioral contract: a message array with a "tool" role message
    // followed by a "user" message must attach images only to the "user"
    // message, not the "tool" message.

    #[test]
    fn rposition_user_predicate_excludes_tool_role() {
        // This is a pure logic test for the predicate used in
        // build_request_with_images. The predicate is `m.role == "user"`.
        // A "tool" message must NOT match.
        //
        // The bug scenario: a "tool" message appears AFTER the last "user"
        // message. The old negation predicate (`!= "system" && != "assistant"`)
        // would match the "tool" message as the last non-system/non-assistant,
        // attaching images to it instead of the user message.
        let messages = [
            ChatMessage::system("system prompt"),
            ChatMessage::user("user question"),
            ChatMessage {
                role: "assistant".to_string(),
                content: "assistant response".to_string(),
            },
            ChatMessage {
                role: "tool".to_string(),
                content: "tool output".to_string(),
            },
        ];

        // The new predicate `m.role == "user"` must find index 1 (the user
        // message), not index 3 (the "tool" message).
        let last_user_idx = messages.iter().rposition(|m| m.role.as_str() == "user");
        assert_eq!(last_user_idx, Some(1));

        // The old predicate `m.role != "system" && m.role != "assistant"`
        // would find index 3 (the "tool" message) — this is the bug.
        let old_predicate_idx = messages
            .iter()
            .rposition(|m| m.role.as_str() != "system" && m.role.as_str() != "assistant");
        assert_eq!(old_predicate_idx, Some(3)); // would match "tool" — the bug
    }

    // ── concurrency semaphore bounds in-flight inference calls ─────────
    //
    // Regression test for the 300s timeout storm. Without the semaphore,
    // `LanguageModelInferencePort::new` spawned a detached `cx.spawn` per
    // request with no bound. A caller firing on a fixed cadence accumulated
    // 100+ in-flight tasks on the GPUI foreground executor, which thrashed
    // and timed out. The `max_concurrency` setting (default 96) was dead —
    // read into the struct but never consumed.
    //
    // This test fires 5 requests with `max_concurrency = 2` against a
    // `FakeLanguageModel` that never completes streams on its own. The
    // semaphore must block the 3rd request from reaching `stream_completion`,
    // so `completion_count()` (open stream senders) must never exceed 2.
    #[gpui::test]
    async fn concurrency_semaphore_bounds_in_flight_calls(cx: &mut gpui::TestAppContext) {
        let model: Arc<dyn language_model::LanguageModel> = Arc::new(FakeLanguageModel::default());
        let fake = model.as_fake();

        let (port, _task) = super::LanguageModelInferencePort::new(
            model.clone(),
            Duration::from_secs(300),
            2, // max_concurrency
            cx.to_async(),
        );

        // Fire 5 non-streaming requests. Each returns a future that resolves
        // when the reply arrives — but the FakeLanguageModel never completes
        // streams, so these futures stay pending. The semaphore should block
        // requests 3-5 from reaching `stream_completion`.
        //
        // We join all 5 futures in a single spawned task so they are polled
        // concurrently by the foreground executor. Without spawning, the
        // returned `BoxFuture`s are never polled and the requests never reach
        // the receiver task.
        let _all_requests = cx.spawn(async move |_cx| {
            let futs: Vec<_> = (0..5)
                .map(|_| {
                    port.generate(
                        "test",
                        &hkask_types::template::LLMParameters::default(),
                        None,
                    )
                })
                .collect();
            // Drive all 5 concurrently. They will all stay pending because
            // the FakeLanguageModel never completes streams.
            futures_util::future::join_all(futs).await
        });

        // Let the foreground executor drain the spawned tasks. The first 2
        // acquire permits and reach `stream_completion`; the remaining 3 block
        // on `semaphore.acquire().await`.
        cx.run_until_parked();

        // Only 2 streams should be open — the semaphore blocked the rest.
        assert_eq!(
            fake.completion_count(),
            2,
            "max_concurrency=2 must bound in-flight stream_completion calls to 2; \
             got {} — the semaphore is not enforcing the limit",
            fake.completion_count()
        );

        // Complete one stream — the semaphore releases a permit, allowing
        // the 3rd request through.
        let first_request = fake.pending_completions().into_iter().next().unwrap();
        fake.end_completion_stream(&first_request);
        cx.run_until_parked();

        assert_eq!(
            fake.completion_count(),
            2,
            "after completing one stream, the next queued request should acquire \
             the released permit — expected 2 open streams, got {}",
            fake.completion_count()
        );
    }

    /// expect: "An agent with ordinary tools can finish with an answer" [P3]
    #[gpui::test]
    async fn ordinary_tools_allow_final_answer(cx: &mut gpui::TestAppContext) {
        let model: Arc<dyn language_model::LanguageModel> = Arc::new(FakeLanguageModel::default());
        let fake = model.as_fake();
        let (port, _receiver) = super::LanguageModelInferencePort::new(
            model.clone(),
            Duration::from_secs(300),
            2,
            cx.to_async(),
        );
        let request = cx.spawn(async move |_| {
            let tools = [ChatToolDefinition {
                tool_type: "function".into(),
                function: ChatToolFunction {
                    name: "lookup".into(),
                    description: "Optional lookup".into(),
                    parameters: serde_json::json!({"type":"object"}),
                },
            }];
            port.generate(
                "Answer if no lookup is needed",
                &LLMParameters::default(),
                Some(&tools),
            )
            .await
        });
        cx.run_until_parked();
        let pending = fake
            .pending_completions()
            .into_iter()
            .next()
            .expect("provider request");
        assert!(
            matches!(pending.tool_choice, Some(LanguageModelToolChoice::Auto)),
            "ordinary tools must not require another effect"
        );
        fake.send_completion_stream_text_chunk(&pending, "Finished without another tool call.");
        fake.end_completion_stream(&pending);
        cx.run_until_parked();
        let answer = request.await.expect("final answer");
        assert_eq!(answer.text, "Finished without another tool call.");
        assert!(answer.tool_calls.is_empty());
    }

    // ── tool_choice: Any for the structured result protocol ─────────────
    //
    // When a structured-output tool (emit_result) is offered, the built
    // request must carry tool_choice: Any ("required" in OpenAI's API) so
    // the provider enforces a tool call instead of prose — the executor
    // extracts args from tool_calls[0], and parse_json_response cannot
    // recover JSON from free text. Without tools, tool_choice must stay
    // None: forcing a tool call when no tool was offered is an invalid
    // request.
    #[gpui::test]
    async fn structured_emit_result_remains_required(cx: &mut gpui::TestAppContext) {
        let model: Arc<dyn language_model::LanguageModel> = Arc::new(FakeLanguageModel::default());
        let (port, _task) = super::LanguageModelInferencePort::new(
            model.clone(),
            Duration::from_secs(300),
            2,
            cx.to_async(),
        );

        let messages = [ChatMessage::user(
            "Execute the instructions above.".to_string(),
        )];
        let parameters = LLMParameters::default();
        let tools = [ChatToolDefinition {
            tool_type: "function".to_string(),
            function: ChatToolFunction {
                name: "emit_result".to_string(),
                description: "Emit the structured result.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                }),
            },
        }];

        let with_tools = port.build_request(&messages, &parameters, Some(&tools));
        assert!(
            matches!(with_tools.tool_choice, Some(LanguageModelToolChoice::Any)),
            "a sole structured result tool must remain required"
        );

        let empty_tools = port.build_request(&messages, &parameters, Some(&[]));
        assert!(empty_tools.tool_choice.is_none());
        let without_tools = port.build_request(&messages, &parameters, None);
        assert!(
            without_tools.tool_choice.is_none(),
            "without tools, tool_choice must be None — forcing a tool call \
             with no tools offered would make the request invalid"
        );
    }
}
