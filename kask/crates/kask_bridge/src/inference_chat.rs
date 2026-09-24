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
    LanguageModelToolChoice, LanguageModelToolUseInput, MessageContent, ProviderErrorCategory,
    Role, StopReason,
};
use tokio::sync::{Semaphore, oneshot};

use crate::inference_resilience::{
    InferenceCircuitPermit, InferenceResilience, InferenceResilienceConfig,
};

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

/// Clear the app-wide inference port. Production never clears — the port is
/// replaced on re-wire, never removed; this exists so tests that install a
/// stub port can restore the pre-test state and parallel tests never
/// observe a stale stub.
#[cfg(test)]
pub fn clear_global_inference_port() {
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
    *guard = None;
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
struct RequestLifetime {
    _admission: tokio::sync::OwnedSemaphorePermit,
    circuit: InferenceCircuitPermit,
    deadline: Option<RequestDeadline>,
}

struct RequestDeadline {
    expires_at: std::time::Instant,
    executor: gpui::BackgroundExecutor,
    timer: gpui::Task<()>,
}

struct InFlightGuard(Arc<std::sync::atomic::AtomicUsize>);
impl InFlightGuard {
    fn new(counter: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self(counter)
    }
}
impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

struct InferenceRequest {
    lifetime: RequestLifetime,
    request: LanguageModelRequest,
    /// Provider-prefixed model name (e.g. "openrouter/z-ai/glm-5.2").
    /// When `Some`, the receiver resolves the model from
    /// `LanguageModelRegistry` and dispatches to it instead of the
    /// default model. `None` selects the default; a failed explicit resolution
    /// returns a typed error without substitution.
    model_override: Option<String>,
    reply: oneshot::Sender<Result<InferenceResult, InferenceError>>,
}

/// Streaming request — forwards `InferenceStreamChunk`s as they arrive
/// instead of collecting the full result. Used by `generate_stream`.
struct StreamInferenceRequest {
    lifetime: RequestLifetime,
    request: LanguageModelRequest,
    model_override: Option<String>,
    reply: tokio::sync::mpsc::UnboundedSender<Result<InferenceStreamChunk, InferenceError>>,
}

/// Render a completion error for an `InferenceError` string payload,
/// preserving the wire fields a `ProviderRejection` carries.
///
/// `InferenceError` (hkask-types) is deliberately string-carrying: hkask
/// crates must not depend on zed-side types (`StatusCode`,
/// `ProviderErrorCategory`), so the variant stays the programmatic
/// classification and this function is where the diagnostic detail is
/// attached — at the bridge boundary, the one place that sees both worlds.
/// Without it, every provider rejection flattens to its Display (the wire
/// message only), which made the 2026-09-10 "Provider returned error"
/// incident unrootable from MCP-server inference logs: credit exhaustion
/// (402), rate limiting (429), upstream outage (502) and statusless
/// mid-stream rejections are indistinguishable yet demand different
/// operator action. Mirrors the agent-side D43 helper
/// (`KaskThreadState::provider_rejection_turn_end_warning`); the
/// classification verdict here states the class only — retry behavior is
/// the caller's policy, not the bridge's to claim.
fn completion_error_detail(error: &LanguageModelCompletionError) -> String {
    let LanguageModelCompletionError::ProviderRejection {
        provider,
        status,
        code,
        message,
        retry_after,
        category,
    } = error
    else {
        return error.to_string();
    };
    let status = status
        .map(|status| status.as_u16().to_string())
        .unwrap_or_else(|| "none (mid-stream rejection)".to_string());
    let code = code.clone().unwrap_or_else(|| "none".to_string());
    let retry_after = retry_after
        .map(|delay| format!("{delay:?}"))
        .unwrap_or_else(|| "none".to_string());
    let classification = if error.is_transient() {
        "transient"
    } else {
        "permanent"
    };
    format!(
        "provider rejection — provider: {}, status: {status}, code: {code}, \
         category: {category:?}, retry_after: {retry_after}, classification: {classification}. \
         Wire message: {message:?}",
        provider.0
    )
}

fn inference_error_from_completion(error: LanguageModelCompletionError) -> InferenceError {
    let detail = completion_error_detail(&error);
    match &error {
        LanguageModelCompletionError::NoApiKey { .. }
        | LanguageModelCompletionError::ProviderRejection {
            category: ProviderErrorCategory::Authentication | ProviderErrorCategory::Permission,
            ..
        } => InferenceError::Auth(detail),
        LanguageModelCompletionError::DataRetentionConsentRequired { .. }
        | LanguageModelCompletionError::SerializeRequest { .. }
        | LanguageModelCompletionError::BuildRequestBody { .. } => {
            InferenceError::NotConfigured(detail)
        }
        LanguageModelCompletionError::ProviderRejection {
            category: ProviderErrorCategory::EndpointNotFound,
            ..
        } => InferenceError::Model(detail),
        LanguageModelCompletionError::ApiReadResponseError { .. }
        | LanguageModelCompletionError::HttpSend { .. } => InferenceError::Connection(detail),
        LanguageModelCompletionError::ProviderRejection { .. }
        | LanguageModelCompletionError::DeserializeResponse { .. }
        | LanguageModelCompletionError::StreamEndedUnexpectedly { .. }
        | LanguageModelCompletionError::Other(_) => InferenceError::Generation(detail),
    }
}

fn is_transient_inference_failure(error: &InferenceError) -> bool {
    match error {
        InferenceError::Timeout(_) | InferenceError::Connection(_) => true,
        InferenceError::Generation(detail) => detail.contains("classification: transient"),
        InferenceError::Overloaded(_)
        | InferenceError::Model(_)
        | InferenceError::Json(_)
        | InferenceError::CircuitOpen(_)
        | InferenceError::VisionUnsupported(_)
        | InferenceError::NotConfigured(_)
        | InferenceError::Auth(_) => false,
    }
}

fn permanent_inference_failure(
    error: &InferenceError,
) -> Option<(hkask_regulation::InferencePermanentFailureKind, String)> {
    use hkask_regulation::InferencePermanentFailureKind;
    let kind = match error {
        InferenceError::Auth(_) => InferencePermanentFailureKind::Authorization,
        InferenceError::NotConfigured(_) => InferencePermanentFailureKind::Configuration,
        InferenceError::Model(_) => InferencePermanentFailureKind::Model,
        InferenceError::Generation(detail) if !detail.contains("classification: transient") => {
            InferencePermanentFailureKind::Provider
        }
        InferenceError::Overloaded(_)
        | InferenceError::Timeout(_)
        | InferenceError::Connection(_)
        | InferenceError::Generation(_)
        | InferenceError::Json(_)
        | InferenceError::CircuitOpen(_)
        | InferenceError::VisionUnsupported(_) => return None,
    };
    Some((kind, error.to_string()))
}

/// Shared accumulator for `collect_completion`: non-streaming calls collect
/// all events; streaming calls forward text/thinking deltas immediately and
/// accumulate metadata for the final chunk.
///
/// `Text` and `Thinking` events are handled by the caller (collected or
/// forwarded) — this struct handles `ToolUse`, `Stop`, and `UsageUpdate`,
/// which are identical in both paths.
struct StreamAccumulator {
    model_name: String,
    text: String,
    reasoning: String,
    tool_calls: Vec<StructuredToolCall>,
    finish_reason: Option<String>,
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
            finish_reason: None,
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
                self.finish_reason = Some(
                    match reason {
                        StopReason::EndTurn => "stop",
                        StopReason::MaxTokens => "length",
                        StopReason::ToolUse => "tool_calls",
                        StopReason::Refusal => "refusal",
                    }
                    .to_string(),
                );
            }
            Ok(LanguageModelCompletionEvent::UsageUpdate(token_usage)) => {
                let prompt = token_usage
                    .input_tokens
                    .checked_add(token_usage.cache_creation_input_tokens)
                    .and_then(|n| n.checked_add(token_usage.cache_read_input_tokens));
                let total = prompt
                    .and_then(|n| n.checked_add(token_usage.output_tokens))
                    .and_then(|n| u32::try_from(n).ok());
                self.usage = InferenceUsage {
                    prompt_tokens: prompt.and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                    completion_tokens: u32::try_from(token_usage.output_tokens).unwrap_or(0),
                    total_tokens: total.unwrap_or(0),
                    // Unrepresentable totals are unknown, never wrapped or truncated.
                    reported: total.is_some(),
                };
                self.cost_usd = token_usage.cost;
            }
            Ok(_) => {}
            Err(error) => return Err(inference_error_from_completion(error)),
        }
        Ok(())
    }

    fn ensure_terminal_stop(&self) -> Result<(), InferenceError> {
        if self.finish_reason.is_some() {
            Ok(())
        } else {
            Err(InferenceError::Connection(
                "provider stream ended without a terminal Stop event".to_string(),
            ))
        }
    }

    /// Build an unchecked `InferenceResult` from accumulated state.
    /// `collect_completion` enforces the terminal event before this conversion.
    fn into_result(self) -> InferenceResult {
        InferenceResult {
            text: self.text,
            model: self.model_name,
            usage: self.usage,
            finish_reason: self
                .finish_reason
                .unwrap_or_else(|| "missing_stop".to_string()),
            tool_calls: self.tool_calls,
            reasoning: if self.reasoning.is_empty() {
                None
            } else {
                Some(self.reasoning)
            },
            cost_usd: self.cost_usd,
        }
    }

    /// Build an unchecked final chunk carrying accumulated metadata.
    /// `collect_completion` enforces the terminal event before this conversion.
    fn into_final_chunk(self) -> InferenceStreamChunk {
        InferenceStreamChunk {
            text_delta: String::new(),
            reasoning_delta: String::new(),
            model: self.model_name,
            finish_reason: Some(
                self.finish_reason
                    .unwrap_or_else(|| "missing_stop".to_string()),
            ),
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
/// The `InferenceResilienceSource` impl exposes these counters with circuit
/// transitions as one coherent observation. `Clone` shares the same runtime
/// state between request dispatch and regulation observation.
#[derive(Clone)]
pub struct LanguageModelInferencePort {
    admission: Arc<Semaphore>,
    background_executor: gpui::BackgroundExecutor,
    inference_timeout: Duration,
    tx: tokio::sync::mpsc::UnboundedSender<InferenceRequest>,
    stream_tx: tokio::sync::mpsc::UnboundedSender<StreamInferenceRequest>,
    in_flight: Arc<std::sync::atomic::AtomicUsize>,
    max_concurrency: Arc<std::sync::atomic::AtomicUsize>,
    recent_timeouts: Arc<std::sync::Mutex<Vec<std::time::Instant>>>,
    resilience: InferenceResilience,
}

impl LanguageModelInferencePort {
    /// Construct the adapter and spawn the GPUI-side receiver task.
    ///
    /// The receiver task runs on the GPUI foreground executor and processes
    /// inference requests. Drop the returned `Task` to stop it.
    ///
    /// `inference_timeout` bounds the wall-clock time for a single inference
    /// call from admission (queue wait + model resolution + establishment + drain). A hung provider stalls the
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
        resilience_config: InferenceResilienceConfig,
        cx: AsyncApp,
    ) -> (Self, gpui::Task<()>) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<InferenceRequest>();
        let (stream_tx, mut stream_rx) =
            tokio::sync::mpsc::unbounded_channel::<StreamInferenceRequest>();
        let model_for_task = model.clone();
        let admission = Arc::new(Semaphore::new(
            max_concurrency
                .max(1)
                .saturating_mul(2)
                .min(Semaphore::MAX_PERMITS),
        ));
        let background_executor = cx.background_executor().clone();
        let concurrency_semaphore = Arc::new(Semaphore::new(max_concurrency.max(1)));
        let in_flight = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let max_concurrency_arc =
            Arc::new(std::sync::atomic::AtomicUsize::new(max_concurrency.max(1)));
        let recent_timeouts: Arc<std::sync::Mutex<Vec<std::time::Instant>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let resilience = InferenceResilience::new(resilience_config);

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
                        let semaphore = concurrency_semaphore.clone();
                        let in_flight = in_flight.clone();
                        let recent_timeouts = recent_timeouts.clone();
                        cx.spawn(async move |cx| {
                            let InferenceRequest { request, model_override, mut reply, lifetime } = req;
                            let RequestLifetime { _admission, circuit, deadline } = lifetime;
                            let work = async {
                                let _permit = semaphore.acquire().await.map_err(|error| InferenceError::Connection(error.to_string()))?;
                                let _in_flight = InFlightGuard::new(in_flight);
                                Self::collect_completion(request, model_override, &model, cx, None).await
                            };
                            let result = tokio::select! {
                                biased;
                                _ = reply.closed() => return,
                                _ = Self::wait_deadline(deadline) => {
                                    recent_timeouts.lock().unwrap_or_else(|error| error.into_inner()).push(std::time::Instant::now());
                                    Err(InferenceError::Timeout("inference admission-to-completion deadline exceeded".into()))
                                },
                                result = work => result,
                            };
                            let transient_failure = result
                                .as_ref()
                                .err()
                                .is_some_and(is_transient_inference_failure);
                            let permanent_failure = result
                                .as_ref()
                                .err()
                                .and_then(permanent_inference_failure);
                            circuit.complete(
                                result.is_ok(),
                                transient_failure,
                                permanent_failure,
                            );
                            if reply.send(result.map(StreamAccumulator::into_result)).is_err() {
                                tracing::trace!(target: "hkask.inference", "inference caller cancelled");
                            }
                        }).detach();
                    }
                    Some(req) = stream_rx.recv() => {
                        let model = model_for_task.clone();
                        let semaphore = concurrency_semaphore.clone();
                        let in_flight = in_flight.clone();
                        let recent_timeouts = recent_timeouts.clone();
                        cx.spawn(async move |cx| {
                            let StreamInferenceRequest { request, model_override, reply, lifetime } = req;
                            let RequestLifetime { _admission, circuit, deadline } = lifetime;
                            let work = async {
                                let _permit = semaphore.acquire().await.map_err(|error| InferenceError::Connection(error.to_string()))?;
                                let _in_flight = InFlightGuard::new(in_flight);
                                Self::collect_completion(request, model_override, &model, cx, Some(&reply)).await
                            };
                            let result = tokio::select! {
                                biased;
                                _ = reply.closed() => return,
                                _ = Self::wait_deadline(deadline) => {
                                    recent_timeouts.lock().unwrap_or_else(|error| error.into_inner()).push(std::time::Instant::now());
                                    Err(InferenceError::Timeout("inference admission-to-completion deadline exceeded".into()))
                                },
                                result = work => result,
                            };
                            let transient_failure = result
                                .as_ref()
                                .err()
                                .is_some_and(is_transient_inference_failure);
                            let permanent_failure = result
                                .as_ref()
                                .err()
                                .and_then(permanent_inference_failure);
                            circuit.complete(
                                result.is_ok(),
                                transient_failure,
                                permanent_failure,
                            );
                            if reply.send(result.map(StreamAccumulator::into_final_chunk)).is_err() {
                                tracing::trace!(target: "hkask.inference", "streaming caller cancelled");
                            }
                        }).detach();
                    }
                    else => break,
                }
            }
            }
        });

        (
            Self {
                admission,
                background_executor,
                inference_timeout,
                tx,
                stream_tx,
                in_flight,
                max_concurrency: max_concurrency_arc,
                recent_timeouts,
                resilience,
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

    fn admit(&self) -> Result<RequestLifetime, InferenceError> {
        let circuit = self.resilience.admit().map_err(|retry_after| {
            InferenceError::CircuitOpen(format!(
                "transient inference failure threshold reached; retry after {retry_after:?}"
            ))
        })?;
        let permit = self.admission.clone().try_acquire_owned().map_err(|_| {
            InferenceError::Overloaded(
                "inference admission capacity reached; request was not dispatched".into(),
            )
        })?;
        let deadline = (!self.inference_timeout.is_zero()).then(|| RequestDeadline {
            expires_at: self.background_executor.now() + self.inference_timeout,
            executor: self.background_executor.clone(),
            timer: self.background_executor.timer(self.inference_timeout),
        });
        Ok(RequestLifetime {
            _admission: permit,
            circuit,
            deadline,
        })
    }

    fn recent_timeout_count_now(&self) -> u64 {
        let now = std::time::Instant::now();
        let mut timeouts = self
            .recent_timeouts
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        timeouts.retain(|instant| now.duration_since(*instant) < RECENT_TIMEOUT_WINDOW);
        timeouts.len() as u64
    }

    async fn wait_deadline(deadline: Option<RequestDeadline>) {
        match deadline {
            // A timer task may not have run yet even when its clock has expired.
            // The biased select must deny dispatch before polling work in that case.
            Some(mut deadline) => {
                futures::future::poll_fn(|context| {
                    if deadline.executor.now() >= deadline.expires_at {
                        std::task::Poll::Ready(())
                    } else {
                        deadline.timer.poll_unpin(context)
                    }
                })
                .await
            }
            None => futures::future::pending().await,
        }
    }

    async fn collect_completion(
        request: LanguageModelRequest,
        model_override: Option<String>,
        model_for_task: &Arc<dyn LanguageModel>,
        cx: &AsyncApp,
        progress: Option<
            &tokio::sync::mpsc::UnboundedSender<Result<InferenceStreamChunk, InferenceError>>,
        >,
    ) -> Result<StreamAccumulator, InferenceError> {
        let model = Self::resolve_model(model_for_task, model_override.as_deref(), cx)
            .await
            .ok_or_else(|| {
                InferenceError::Model(format!(
                    "model_override '{}' not found; no default substitution",
                    model_override.as_deref().unwrap_or("")
                ))
            })?;
        let mut stream = model
            .stream_completion(request, cx)
            .await
            .map_err(inference_error_from_completion)?;
        let model_name = model.name().0.to_string();
        let mut accumulator = StreamAccumulator::new(model_name.clone());
        while let Some(event) = stream.next().await {
            let delta = match (progress, event) {
                (Some(_), Ok(LanguageModelCompletionEvent::Text(text))) => {
                    Some((text, String::new()))
                }
                (Some(_), Ok(LanguageModelCompletionEvent::Thinking { text, .. })) => {
                    Some((String::new(), text))
                }
                (_, other) => {
                    accumulator.process_event(other)?;
                    None
                }
            };
            if let (Some(reply), Some((text_delta, reasoning_delta))) = (progress, delta) {
                reply
                    .send(Ok(InferenceStreamChunk {
                        text_delta,
                        reasoning_delta,
                        model: model_name.clone(),
                        finish_reason: None,
                        usage: None,
                        tool_calls: Vec::new(),
                        cost_usd: None,
                    }))
                    .map_err(|_| {
                        InferenceError::Connection("inference stream receiver closed".into())
                    })?;
            }
        }
        accumulator.ensure_terminal_stop()?;
        Ok(accumulator)
    }

    fn stream_request(
        &self,
        request: LanguageModelRequest,
        model_override: Option<String>,
    ) -> std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send>,
    > {
        let lifetime = match self.admit() {
            Ok(lifetime) => lifetime,
            Err(error) => return Box::pin(futures_util::stream::once(async { Err(error) })),
        };
        let (reply, receiver) = tokio::sync::mpsc::unbounded_channel();
        if self
            .stream_tx
            .send(StreamInferenceRequest {
                lifetime,
                request,
                model_override,
                reply,
            })
            .is_err()
        {
            return Box::pin(futures_util::stream::once(async {
                Err(InferenceError::Connection(
                    "inference stream channel closed".into(),
                ))
            }));
        }
        Box::pin(futures_util::stream::unfold(
            receiver,
            |mut receiver| async move { receiver.recv().await.map(|chunk| (chunk, receiver)) },
        ))
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
                    lifetime: self.admit()?,
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
        // Vision models receive the task with its image, matching their multimodal
        // user-message contract rather than relying on a separate system instruction.
        let messages = vec![ChatMessage::user(prompt.to_string())];
        let request = self.build_request_with_images(&messages, images, parameters, None);
        let model_override = model_override.map(|s| s.to_string());
        let (tx_reply, rx_reply) = oneshot::channel();
        async move {
            self.tx
                .send(InferenceRequest {
                    lifetime: self.admit()?,
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
        self.stream_request(request, None)
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
        self.stream_request(request, Some(model_override))
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
        self.stream_request(request, model_override)
    }
}

/// Window for recent timeout tracking — timeouts older than this are evicted
/// on each read. 5 minutes matches the cybernetics loop's tick cadence (10s)
/// × 30 ticks, so the sensor sees a storm of ~30 timeouts before evicting.
const RECENT_TIMEOUT_WINDOW: Duration = Duration::from_secs(300);

#[async_trait::async_trait]
impl hkask_regulation::InferenceResilienceSource for LanguageModelInferencePort {
    async fn observe_since(
        &self,
        cursor: u64,
    ) -> Result<hkask_regulation::InferenceObservation, hkask_regulation::InferenceObservationError>
    {
        use crate::inference_resilience::{CircuitState, CircuitTransition};
        use hkask_regulation::{
            InferenceCircuitState, InferenceInterventionKind, InferenceInterventionReceipt,
            InferenceObservation, InferenceSnapshot,
        };

        let circuit = self.resilience.observe_since(cursor);
        let circuit_state = match circuit.state {
            CircuitState::Closed => InferenceCircuitState::Closed,
            CircuitState::Open { .. } => InferenceCircuitState::Open,
            CircuitState::HalfOpen { .. } => InferenceCircuitState::HalfOpen,
        };
        let interventions: Vec<_> = circuit
            .receipts
            .into_iter()
            .map(|receipt| InferenceInterventionReceipt {
                id: receipt.id,
                kind: match receipt.transition {
                    CircuitTransition::Opened => InferenceInterventionKind::CircuitOpened,
                    CircuitTransition::HalfOpened => InferenceInterventionKind::CircuitHalfOpened,
                    CircuitTransition::Closed => InferenceInterventionKind::CircuitClosed,
                    CircuitTransition::Reopened => InferenceInterventionKind::CircuitReopened,
                },
                occurred_at: receipt.occurred_at,
            })
            .collect();
        let permanent_failures = circuit.permanent_failures;
        let next_cursor = circuit.next_cursor;

        Ok(InferenceObservation {
            snapshot: InferenceSnapshot {
                observed_at: chrono::Utc::now(),
                in_flight: self.in_flight.load(std::sync::atomic::Ordering::Relaxed),
                max_concurrency: self
                    .max_concurrency
                    .load(std::sync::atomic::Ordering::Relaxed),
                recent_timeout_count: self.recent_timeout_count_now(),
                circuit_state,
            },
            interventions,
            permanent_failures,
            next_cursor,
        })
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
    use hkask_types::template::LLMParameters;
    use hkask_types::{InferenceError, InferencePort};
    use language_model::fake_provider::FakeLanguageModel;
    use language_model_core::LanguageModelToolChoice;
    use language_model_core::ProviderErrorCategory;
    use language_model_core::{LanguageModelCompletionError, LanguageModelProviderName};
    use std::sync::Arc;
    use std::time::Duration;

    fn test_resilience_config() -> crate::InferenceResilienceConfig {
        crate::InferenceResilienceConfig {
            transient_failure_threshold: 3,
            open_duration: Duration::from_secs(30),
        }
    }

    /// expect: "Inference reports include cached prompt categories and preserve provider cost" [P8]
    #[test]
    fn completion_usage_includes_cached_tokens() {
        let mut accumulator = super::StreamAccumulator::new("fixture".into());
        accumulator
            .process_event(Ok(
                language_model_core::LanguageModelCompletionEvent::UsageUpdate(
                    language_model_core::TokenUsage {
                        input_tokens: 4,
                        output_tokens: 7,
                        cache_creation_input_tokens: 3,
                        cache_read_input_tokens: 5,
                        cost: Some(0.01),
                    },
                ),
            ))
            .expect("usage event");
        let result = accumulator.into_result();
        assert_eq!(result.usage.prompt_tokens, 12);
        assert_eq!(result.usage.total_tokens, 19);
        assert!(result.usage.reported);
        assert_eq!(result.cost_usd, Some(0.01));
    }

    #[test]
    fn completion_requires_explicit_terminal_stop() {
        let mut incomplete = super::StreamAccumulator::new("test-model".to_string());
        incomplete
            .process_event(Ok(language_model_core::LanguageModelCompletionEvent::Text(
                "partial JSON".to_string(),
            )))
            .expect("text event");
        assert!(matches!(
            incomplete.ensure_terminal_stop(),
            Err(InferenceError::Connection(_))
        ));
        assert_eq!(incomplete.into_result().finish_reason, "missing_stop");

        let mut complete = super::StreamAccumulator::new("test-model".to_string());
        complete
            .process_event(Ok(language_model_core::LanguageModelCompletionEvent::Stop(
                language_model_core::StopReason::EndTurn,
            )))
            .expect("terminal stop event");
        complete
            .ensure_terminal_stop()
            .expect("explicit terminal stop");
        assert_eq!(complete.into_result().finish_reason, "stop");
    }

    // ── Provider-rejection detail preservation (D43-adjacent, bridge path) ──

    #[test]
    fn completion_error_detail_carries_rejection_wire_fields() {
        // The 2026-09-10 incident class: without enrichment, a rejection's
        // Display is the wire message only — 402/429/502/statusless all read
        // "Provider returned error". The detail must carry every preserved
        // wire field plus the retryability class.
        let rejection = LanguageModelCompletionError::ProviderRejection {
            provider: LanguageModelProviderName::new("OpenRouter"),
            status: Some(http_client::StatusCode::BAD_GATEWAY),
            code: Some("502".to_string()),
            message: "Provider returned error".to_string(),
            retry_after: None,
            category: ProviderErrorCategory::InternalServer,
        };
        let detail = super::completion_error_detail(&rejection);
        assert!(detail.contains("OpenRouter"));
        assert!(detail.contains("502"));
        assert!(detail.contains("InternalServer"));
        assert!(detail.contains("transient"));
        assert!(detail.contains("Provider returned error"));
    }

    #[test]
    fn completion_error_detail_marks_statusless_rejections_permanent() {
        // Mid-stream rejections carry no HTTP status; a code mapping to no
        // known category is permanent-class — the sub-second failure
        // signature from the incident log.
        let rejection = LanguageModelCompletionError::ProviderRejection {
            provider: LanguageModelProviderName::new("OpenRouter"),
            status: None,
            code: Some("499".to_string()),
            message: "Provider returned error".to_string(),
            retry_after: None,
            category: ProviderErrorCategory::Other,
        };
        let detail = super::completion_error_detail(&rejection);
        assert!(detail.contains("none (mid-stream rejection)"));
        assert!(detail.contains("499"));
        assert!(detail.contains("Other"));
        assert!(detail.contains("permanent"));
    }

    #[test]
    fn completion_auth_rejection_maps_to_typed_auth_error() {
        let rejection = LanguageModelCompletionError::ProviderRejection {
            provider: LanguageModelProviderName::new("OpenRouter"),
            status: Some(http_client::StatusCode::UNAUTHORIZED),
            code: Some("401".to_string()),
            message: "invalid credential".to_string(),
            retry_after: None,
            category: ProviderErrorCategory::Authentication,
        };
        assert!(matches!(
            super::inference_error_from_completion(rejection),
            InferenceError::Auth(_)
        ));
    }

    /// expect: "Permanent completion failures preserve the operator action they require"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: completion fails through configuration, model, or non-retryable provider paths
    /// post: each path maps to its distinct permanent-failure category
    #[test]
    fn permanent_completion_errors_preserve_typed_categories() {
        let configuration = super::inference_error_from_completion(
            LanguageModelCompletionError::DataRetentionConsentRequired {
                model_name: "retained-model".to_string(),
            },
        );
        assert!(matches!(configuration, InferenceError::NotConfigured(_)));
        assert_eq!(
            super::permanent_inference_failure(&configuration).map(|(kind, _)| kind),
            Some(hkask_regulation::InferencePermanentFailureKind::Configuration)
        );

        let model = super::inference_error_from_completion(
            LanguageModelCompletionError::ProviderRejection {
                provider: LanguageModelProviderName::new("OpenRouter"),
                status: Some(http_client::StatusCode::NOT_FOUND),
                code: Some("404".to_string()),
                message: "model not found".to_string(),
                retry_after: None,
                category: ProviderErrorCategory::EndpointNotFound,
            },
        );
        assert!(matches!(model, InferenceError::Model(_)));
        assert_eq!(
            super::permanent_inference_failure(&model).map(|(kind, _)| kind),
            Some(hkask_regulation::InferencePermanentFailureKind::Model)
        );

        let provider = super::inference_error_from_completion(LanguageModelCompletionError::Other(
            anyhow::anyhow!("permanent provider failure"),
        ));
        assert!(matches!(provider, InferenceError::Generation(_)));
        assert_eq!(
            super::permanent_inference_failure(&provider).map(|(kind, _)| kind),
            Some(hkask_regulation::InferencePermanentFailureKind::Provider)
        );
    }

    /// expect: "A missing provider credential remains an authorization failure and never quarantines inference"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: three sequential provider streams fail because their API key is absent
    /// post: the circuit remains closed and all three failures are observable as authorization receipts
    #[gpui::test]
    async fn missing_api_key_failures_do_not_open_the_transient_circuit(
        cx: &mut gpui::TestAppContext,
    ) {
        let model: Arc<dyn language_model::LanguageModel> = Arc::new(FakeLanguageModel::default());
        let fake = model.as_fake();
        let (port, _task) = super::LanguageModelInferencePort::new(
            model.clone(),
            Duration::from_secs(300),
            1,
            test_resilience_config(),
            cx.to_async(),
        );

        for attempt_index in 0..3 {
            let attempt_port = port.clone();
            let prompt = format!("missing-key-{attempt_index}");
            let attempt = cx.spawn(async move |_cx| {
                attempt_port
                    .generate(&prompt, &LLMParameters::default(), None)
                    .await
            });
            cx.run_until_parked();
            fake.send_last_completion_stream_error(LanguageModelCompletionError::NoApiKey {
                provider: LanguageModelProviderName::new("OpenRouter"),
            });
            cx.run_until_parked();
            assert!(matches!(attempt.await, Err(InferenceError::Auth(_))));
        }

        let observation = hkask_regulation::InferenceResilienceSource::observe_since(&port, 0)
            .await
            .expect("in-process resilience observation");
        assert_eq!(
            observation.snapshot.circuit_state,
            hkask_regulation::InferenceCircuitState::Closed
        );
        assert_eq!(observation.permanent_failures.len(), 3);
        assert!(observation.permanent_failures.iter().all(|receipt| {
            receipt.kind == hkask_regulation::InferencePermanentFailureKind::Authorization
        }));
    }

    #[test]
    fn completion_error_detail_passes_non_rejections_through() {
        let transport = LanguageModelCompletionError::Other(anyhow::anyhow!("transport error"));
        assert_eq!(
            super::completion_error_detail(&transport),
            "transport error"
        );
    }

    #[gpui::test]
    async fn mid_stream_rejection_surfaces_wire_detail_through_the_port(
        cx: &mut gpui::TestAppContext,
    ) {
        // Behavioral pin of the mid-stream conversion site: a ProviderRejection
        // arriving mid-stream must reach the MCP-server caller as
        // InferenceError::Generation carrying the wire fields, not the bare
        // "Provider returned error" Display.
        let model: Arc<dyn language_model::LanguageModel> = Arc::new(FakeLanguageModel::default());
        let fake = model.as_fake();
        let (port, _task) = super::LanguageModelInferencePort::new(
            model.clone(),
            Duration::from_secs(300),
            2, // max_concurrency
            test_resilience_config(),
            cx.to_async(),
        );

        let generate =
            cx.spawn(async move |_cx| port.generate("test", &LLMParameters::default(), None).await);
        // Let the request reach the fake and open its stream.
        cx.run_until_parked();
        fake.send_last_completion_stream_error(LanguageModelCompletionError::ProviderRejection {
            provider: LanguageModelProviderName::new("OpenRouter"),
            status: Some(http_client::StatusCode::BAD_GATEWAY),
            code: Some("502".to_string()),
            message: "Provider returned error".to_string(),
            retry_after: None,
            category: ProviderErrorCategory::InternalServer,
        });
        cx.run_until_parked();

        let Err(error) = generate.await else {
            panic!("the injected stream error must fail the generate call");
        };
        let InferenceError::Generation(detail) = error else {
            panic!("mid-stream rejection must surface as Generation, got: {error:?}");
        };
        assert!(detail.contains("OpenRouter"));
        assert!(detail.contains("502"));
        assert!(detail.contains("InternalServer"));
        assert!(detail.contains("Provider returned error"));
    }

    /// expect: "Repeated transient provider failures open the live inference circuit"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: three sequential requests receive retryable provider rejections
    /// post: the next request fails before dispatch with `InferenceError::CircuitOpen`
    /// [P2] Constraining: automatic control is bounded to reversible admission denial
    #[gpui::test]
    async fn transient_provider_storm_opens_live_inference_circuit(cx: &mut gpui::TestAppContext) {
        let model: Arc<dyn language_model::LanguageModel> = Arc::new(FakeLanguageModel::default());
        let fake = model.as_fake();
        let (port, _task) = super::LanguageModelInferencePort::new(
            model.clone(),
            Duration::from_secs(300),
            2,
            test_resilience_config(),
            cx.to_async(),
        );

        for attempt_index in 0..3 {
            let attempt_port = port.clone();
            let prompt = format!("test-{attempt_index}");
            let attempt = cx.spawn(async move |_cx| {
                attempt_port
                    .generate(&prompt, &LLMParameters::default(), None)
                    .await
            });
            cx.run_until_parked();
            fake.send_last_completion_stream_error(
                LanguageModelCompletionError::ProviderRejection {
                    provider: LanguageModelProviderName::new("OpenRouter"),
                    status: Some(http_client::StatusCode::BAD_GATEWAY),
                    code: Some("502".to_string()),
                    message: "Provider returned error".to_string(),
                    retry_after: None,
                    category: ProviderErrorCategory::InternalServer,
                },
            );
            cx.run_until_parked();
            assert!(matches!(attempt.await, Err(InferenceError::Generation(_))));
        }

        assert!(matches!(
            port.generate("blocked", &LLMParameters::default(), None)
                .await,
            Err(InferenceError::CircuitOpen(_))
        ));
    }

    /// expect: "A successful half-open inference probe restores live service"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: one transient failure opens a zero-delay test circuit
    /// post: one probe reaches the provider, succeeds, and closes the observed circuit
    #[gpui::test]
    async fn successful_live_half_open_probe_closes_circuit(cx: &mut gpui::TestAppContext) {
        let model: Arc<dyn language_model::LanguageModel> = Arc::new(FakeLanguageModel::default());
        let fake = model.as_fake();
        let (port, _task) = super::LanguageModelInferencePort::new(
            model.clone(),
            Duration::from_secs(300),
            1,
            crate::InferenceResilienceConfig {
                transient_failure_threshold: 1,
                open_duration: Duration::ZERO,
            },
            cx.to_async(),
        );

        let failed_port = port.clone();
        let failed = cx.spawn(async move |_cx| {
            failed_port
                .generate("failure", &LLMParameters::default(), None)
                .await
        });
        cx.run_until_parked();
        fake.send_last_completion_stream_error(LanguageModelCompletionError::ProviderRejection {
            provider: LanguageModelProviderName::new("OpenRouter"),
            status: Some(http_client::StatusCode::BAD_GATEWAY),
            code: Some("502".to_string()),
            message: "Provider returned error".to_string(),
            retry_after: None,
            category: ProviderErrorCategory::InternalServer,
        });
        cx.run_until_parked();
        assert!(matches!(failed.await, Err(InferenceError::Generation(_))));

        let probe_port = port.clone();
        let probe = cx.spawn(async move |_cx| {
            probe_port
                .generate("probe", &LLMParameters::default(), None)
                .await
        });
        cx.run_until_parked();
        fake.send_last_completion_stream_event(
            language_model_core::LanguageModelCompletionEvent::Stop(
                language_model_core::StopReason::EndTurn,
            ),
        );
        fake.end_last_completion_stream();
        cx.run_until_parked();
        assert!(probe.await.is_ok());

        let observation = hkask_regulation::InferenceResilienceSource::observe_since(&port, 0)
            .await
            .expect("in-process resilience observation");
        assert_eq!(
            observation.snapshot.circuit_state,
            hkask_regulation::InferenceCircuitState::Closed
        );
        let transitions: Vec<_> = observation
            .interventions
            .iter()
            .map(|receipt| receipt.kind)
            .collect();
        assert_eq!(
            transitions,
            vec![
                hkask_regulation::InferenceInterventionKind::CircuitOpened,
                hkask_regulation::InferenceInterventionKind::CircuitHalfOpened,
                hkask_regulation::InferenceInterventionKind::CircuitClosed,
            ]
        );
    }

    /// expect: "Repeated inference deadlines open the live circuit before more work dispatches"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: three sequential requests exceed the admission-to-completion deadline
    /// post: the next request fails before dispatch with `InferenceError::CircuitOpen`
    #[gpui::test]
    async fn timeout_storm_opens_live_inference_circuit(cx: &mut gpui::TestAppContext) {
        let model: Arc<dyn language_model::LanguageModel> = Arc::new(FakeLanguageModel::default());
        let (port, _task) = super::LanguageModelInferencePort::new(
            model,
            Duration::from_secs(2),
            1,
            test_resilience_config(),
            cx.to_async(),
        );

        for attempt_index in 0..3 {
            let attempt_port = port.clone();
            let prompt = format!("timeout-{attempt_index}");
            let attempt = cx.spawn(async move |_cx| {
                attempt_port
                    .generate(&prompt, &LLMParameters::default(), None)
                    .await
            });
            cx.run_until_parked();
            cx.executor().advance_clock(Duration::from_secs(3));
            cx.run_until_parked();
            assert!(matches!(attempt.await, Err(InferenceError::Timeout(_))));
        }

        assert!(matches!(
            port.generate("blocked", &LLMParameters::default(), None)
                .await,
            Err(InferenceError::CircuitOpen(_))
        ));
    }

    /// expect: "Permanent provider failures remain visible without tripping transient resilience"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: a provider returns a non-retryable rejection
    /// post: the circuit stays closed and a permanent-failure receipt is observable
    #[gpui::test]
    async fn permanent_provider_failure_is_observed_without_opening_circuit(
        cx: &mut gpui::TestAppContext,
    ) {
        let model: Arc<dyn language_model::LanguageModel> = Arc::new(FakeLanguageModel::default());
        let fake = model.as_fake();
        let (port, _task) = super::LanguageModelInferencePort::new(
            model.clone(),
            Duration::from_secs(300),
            2,
            test_resilience_config(),
            cx.to_async(),
        );
        let attempt_port = port.clone();
        let attempt = cx.spawn(async move |_cx| {
            attempt_port
                .generate("permanent", &LLMParameters::default(), None)
                .await
        });
        cx.run_until_parked();
        fake.send_last_completion_stream_error(LanguageModelCompletionError::ProviderRejection {
            provider: LanguageModelProviderName::new("OpenRouter"),
            status: None,
            code: Some("499".to_string()),
            message: "Provider returned error".to_string(),
            retry_after: None,
            category: ProviderErrorCategory::Other,
        });
        cx.run_until_parked();
        assert!(matches!(attempt.await, Err(InferenceError::Generation(_))));

        let observation = hkask_regulation::InferenceResilienceSource::observe_since(&port, 0)
            .await
            .expect("in-process resilience observation");
        assert_eq!(
            observation.snapshot.circuit_state,
            hkask_regulation::InferenceCircuitState::Closed
        );
        assert_eq!(observation.permanent_failures.len(), 1);
        assert_eq!(
            observation
                .permanent_failures
                .first()
                .map(|receipt| receipt.kind),
            Some(hkask_regulation::InferencePermanentFailureKind::Provider)
        );
    }

    #[test]
    fn establishment_errors_route_through_the_detail_helper() {
        // The establishment path (stream_completion returning Err before any
        // events) cannot be driven with a ProviderRejection through
        // FakeLanguageModel (its forbidden-request path hardcodes an `Other`
        // error), so the wiring is pinned structurally. The needle
        // is assembled from pieces so this test's own source cannot satisfy
        // it.
        let source = include_str!("inference_chat.rs");
        let needle = concat!("map_err(inference_error_from_", "completion)");
        assert!(
            source.contains(needle),
            "the stream-establishment path must preserve typed provider classification"
        );
    }

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
            test_resilience_config(),
            cx.to_async(),
        );

        // Start 4 calls
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

        // Close one fake stream — regardless of its terminal validity, dropping
        // the request releases a permit and allows the 3rd request through.
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

    /// expect: "Queue wait and stalled stream drain share the admission deadline" [P1]
    #[gpui::test]
    async fn queued_and_established_requests_share_deadline(cx: &mut gpui::TestAppContext) {
        let model: Arc<dyn language_model::LanguageModel> = Arc::new(FakeLanguageModel::default());
        let fake = model.as_fake();
        let (port, _receiver) = super::LanguageModelInferencePort::new(
            model.clone(),
            Duration::from_secs(2),
            1,
            test_resilience_config(),
            cx.to_async(),
        );
        let first = {
            let port = port.clone();
            cx.spawn(async move |_| {
                port.generate("first", &LLMParameters::default(), None)
                    .await
            })
        };
        let queued = {
            let port = port.clone();
            cx.spawn(async move |_| {
                port.generate("queued", &LLMParameters::default(), None)
                    .await
            })
        };
        cx.run_until_parked();
        assert_eq!(fake.completion_count(), 1);
        cx.executor().advance_clock(Duration::from_secs(3));
        cx.run_until_parked();
        assert!(matches!(first.await, Err(InferenceError::Timeout(_))));
        assert!(matches!(queued.await, Err(InferenceError::Timeout(_))));
        assert_eq!(
            fake.completion_count(),
            1,
            "expired queued request must never dispatch"
        );
        assert_eq!(port.in_flight.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(port.admission.available_permits(), 2);
    }

    /// expect: "Streaming deadlines start at admission, even before the receiver polls, and cover the drain" [P1]
    #[gpui::test]
    async fn streaming_deadline_covers_unpolled_queue_and_drain(cx: &mut gpui::TestAppContext) {
        use futures_util::StreamExt;
        for poll_receiver in [false, true] {
            let model: Arc<dyn language_model::LanguageModel> =
                Arc::new(FakeLanguageModel::default());
            let fake = model.as_fake();
            let (port, _receiver) = super::LanguageModelInferencePort::new(
                model.clone(),
                Duration::from_secs(2),
                1,
                test_resilience_config(),
                cx.to_async(),
            );
            let mut stream = port.generate_stream("stream", &LLMParameters::default(), None);
            assert_eq!(port.admission.available_permits(), 1);
            if poll_receiver {
                cx.run_until_parked();
                assert_eq!(fake.completion_count(), 1);
            }
            // advance_clock runs ready tasks first; advance only the clock to
            // exercise admission before the receiver's first poll.
            cx.dispatcher
                .scheduler()
                .clock()
                .advance(Duration::from_secs(3));
            cx.run_until_parked();
            assert!(matches!(
                stream.next().await,
                Some(Err(InferenceError::Timeout(_)))
            ));
            assert!(stream.next().await.is_none());
            assert_eq!(fake.completion_count(), usize::from(poll_receiver));
            assert_eq!(port.in_flight.load(std::sync::atomic::Ordering::Relaxed), 0);
            assert_eq!(port.admission.available_permits(), 2);
        }
    }

    /// expect: "Disabled deadlines still allow cancellation to release active work" [P1]
    #[gpui::test]
    async fn streaming_cancellation_releases_permits_with_disabled_deadline(
        cx: &mut gpui::TestAppContext,
    ) {
        let model: Arc<dyn language_model::LanguageModel> = Arc::new(FakeLanguageModel::default());
        let (port, _receiver) = super::LanguageModelInferencePort::new(
            model,
            Duration::ZERO,
            1,
            test_resilience_config(),
            cx.to_async(),
        );
        let stream = port.generate_stream("stream", &LLMParameters::default(), None);
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(3600));
        cx.run_until_parked();
        assert_eq!(port.in_flight.load(std::sync::atomic::Ordering::Relaxed), 1);
        drop(stream);
        cx.run_until_parked();
        assert_eq!(port.in_flight.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(port.admission.available_permits(), 2);
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
            test_resilience_config(),
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
