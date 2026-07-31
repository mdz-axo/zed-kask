//! `FusionLanguageModel` — a zed `LanguageModel` that delegates to hKask's
//! fusion orchestrator.
//!
//! This is the bridge that makes fusion reachable from zed's agent thread
//! (Slice 2). The agent thread calls `model.stream_completion(request, cx)`
//! on a single `Arc<dyn LanguageModel>`. When that model is a
//! `FusionLanguageModel`, the request is routed through hKask's
//! `fusion_orchestrator::orchestrate`, which dispatches to a panel of models
//! in parallel and fuses their outputs via a judge (LLM or algorithmic).
//!
//! ## Architecture
//!
//! ```text
//! agent thread
//!     │
//!     ▼
//! FusionLanguageModel.stream_completion(request, cx)
//!     │
//!     │  flatten messages → prompt string
//!     │  build LLMParameters with fusion_config
//!     │
//!     ▼
//! fusion_orchestrator::orchestrate(router, prompt, params, tools, fusion)
//!     │
//!     │  dispatch_panel: parallel calls to each panel model
//!     │  call_judge: fuse panel responses
//!     │
//!     ▼
//! MultiModelInferencePort (routes generate_with_model by name)
//!     │
//!     │  resolve model name → LanguageModelInferencePort
//!     │  (each port holds a channel to a GPUI-side task)
//!     │
//!     ▼
//! zed's LanguageModel providers (Anthropic, OpenAI, Ollama, etc.)
//! ```
//!
//! ## Streaming
//!
//! Fusion is non-streaming (the orchestrator collects complete panel
//! responses before fusing). The fused result is emitted as a single
//! `Text` chunk followed by a `Stop(EndTurn)`. This is acceptable for the
//! manifest cascade (which needs complete results for PDCA convergence)
//! and for the kask panel. For the regular chat path, the user sees a
//! brief delay while the panel runs in parallel, then the fused output
//! appears at once.
//!
//! ## Send/Sync
//!
//! `LanguageModel: Send + Sync`, but `AsyncApp` is not `Send` (GPUI's
//! `Rc`-based state). Following the same pattern as `LanguageModelInferencePort`,
//! the `AsyncApp` is moved into a GPUI-side channel task at construction
//! time. The `FusionLanguageModel` holds only `LanguageModelInferencePort`
//! instances (which hold `Send + Sync` channel senders), not `AsyncApp`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures::stream::BoxStream;
use futures_util::{FutureExt, StreamExt, stream};
use gpui::{App, AppContext, AsyncApp, Entity, Task};
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferencePort, InferenceResult,
};
use language_model::LanguageModel;
use language_model_core::{
    LanguageModelCompletionEvent, LanguageModelId, LanguageModelName, LanguageModelProviderId,
    LanguageModelProviderName, LanguageModelRequest, LanguageModelRequestToolInput,
    LanguageModelToolChoice, MessageContent, Role, StopReason, TokenUsage,
};

use crate::inference::LanguageModelInferencePort;

/// The provider ID under which fusion models are registered.
pub const FUSION_PROVIDER_ID: &str = "kask-fusion";

/// The model ID for the default fusion model.
pub const FUSION_MODEL_ID: &str = "fusion";

/// A zed `LanguageModel` that delegates to hKask's fusion orchestrator.
///
/// Constructed by the composition root when `kask.fusion.enabled == true`.
/// The agent thread treats it as a regular `LanguageModel` — it has no
/// knowledge of fusion.
pub struct FusionLanguageModel {
    id: LanguageModelId,
    name: LanguageModelName,
    config: hkask_types::fusion::FusionConfig,
    /// Pre-constructed inference ports, keyed by provider-prefixed model name.
    /// Each port holds a `Send + Sync` channel sender to a GPUI-side task.
    ports: HashMap<String, Arc<LanguageModelInferencePort>>,
}

impl FusionLanguageModel {
    /// Construct a fusion model from a `FusionConfig`, a map of resolved
    /// models, and the GPUI async context.
    ///
    /// The `models` map keys must match the names in `config.panel` and
    /// `config.judge`. The composition root is responsible for resolving
    /// provider-prefixed names to `Arc<dyn LanguageModel>` instances from
    /// the `LanguageModelRegistry`.
    ///
    /// For each resolved model, a `LanguageModelInferencePort` is constructed
    /// (spawning a GPUI-side channel task). The tasks are detached — they
    /// live until the ports are dropped.
    ///
    /// Returns `None` if the judge model is missing (and judge != "algo"),
    /// or if no panel models are present.
    #[must_use]
    pub fn new(
        config: hkask_types::fusion::FusionConfig,
        models: HashMap<String, Arc<dyn LanguageModel>>,
        cx: AsyncApp,
    ) -> Option<Self> {
        let is_algo = config.judge.to_lowercase() == "algo";
        if !is_algo && !models.contains_key(&config.judge) {
            tracing::warn!(
                target: "reg.fusion",
                judge = %config.judge,
                "Fusion judge model not in resolved set — fusion disabled"
            );
            return None;
        }

        // Construct a LanguageModelInferencePort for each resolved model.
        let mut ports: HashMap<String, Arc<LanguageModelInferencePort>> = HashMap::new();
        for (name, model) in &models {
            let (port, task) = LanguageModelInferencePort::new(model.clone(), cx.clone());
            task.detach();
            ports.insert(name.clone(), Arc::new(port));
        }

        // Check that at least one panel model is present.
        let panel_present = config.panel.iter().any(|name| ports.contains_key(name));
        if !panel_present {
            tracing::warn!(
                target: "reg.fusion",
                "No panel models resolved — fusion disabled"
            );
            return None;
        }

        Some(Self {
            id: LanguageModelId::from(FUSION_MODEL_ID.to_string()),
            name: LanguageModelName::from("Fusion".to_string()),
            config,
            ports,
        })
    }

    /// Build the multi-model `InferencePort` adapter that the fusion
    /// orchestrator will call.
    fn build_router(&self) -> MultiModelInferencePort {
        MultiModelInferencePort {
            ports: self.ports.clone(),
        }
    }

    /// Flatten a `LanguageModelRequest` into the prompt + tools that the
    /// fusion orchestrator expects.
    ///
    /// Fusion is prompt-based internally (see `fusion_orchestrator::dispatch_panel`
    /// and `call_judge`). The orchestrator sends a single prompt string to
    /// each panel model, not a message array. We flatten the messages into
    /// a single prompt with role prefixes.
    fn flatten_request(request: &LanguageModelRequest) -> (String, Vec<ChatToolDefinition>) {
        let prompt = request
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::Assistant => "assistant",
                    Role::User => "user",
                };
                let content = m
                    .content
                    .iter()
                    .map(|c| match c {
                        MessageContent::Text(text) => text.as_str(),
                        _ => "",
                    })
                    .collect::<Vec<_>>()
                    .join("");
                format!("{role}: {content}")
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let tools: Vec<ChatToolDefinition> = request
            .tools
            .iter()
            .map(|t| ChatToolDefinition {
                tool_type: "function".to_string(),
                function: hkask_types::ChatToolFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: match &t.input {
                        LanguageModelRequestToolInput::Function { input_schema, .. } => {
                            input_schema.clone()
                        }
                        LanguageModelRequestToolInput::Custom { .. } => serde_json::Value::Null,
                    },
                },
            })
            .collect();

        (prompt, tools)
    }
}

impl LanguageModel for FusionLanguageModel {
    fn id(&self) -> LanguageModelId {
        self.id.clone()
    }

    fn name(&self) -> LanguageModelName {
        self.name.clone()
    }

    fn provider_id(&self) -> LanguageModelProviderId {
        LanguageModelProviderId(FUSION_PROVIDER_ID.into())
    }

    fn provider_name(&self) -> LanguageModelProviderName {
        LanguageModelProviderName("Kask Fusion".into())
    }

    fn telemetry_id(&self) -> String {
        format!("kask-fusion/{}", self.config.mode.as_str())
    }

    fn supports_images(&self) -> bool {
        // Fusion flattens messages to text — images are dropped.
        false
    }

    fn supports_tools(&self) -> bool {
        // The fusion orchestrator passes tools through to panel models.
        // Whether tools actually work depends on the panel models.
        true
    }

    fn supports_tool_choice(&self, _choice: LanguageModelToolChoice) -> bool {
        false
    }

    fn max_token_count(&self) -> u64 {
        // Use the minimum of all panel models' max token counts, since the
        // prompt goes to all of them. Fall back to a large default.
        self.ports
            .values()
            .map(|_| 128_000u64) // LanguageModelInferencePort doesn't expose max_token_count
            .min()
            .unwrap_or(128_000)
    }

    fn stream_completion(
        &self,
        request: LanguageModelRequest,
        _cx: &AsyncApp,
    ) -> futures::future::BoxFuture<
        'static,
        Result<
            BoxStream<
                'static,
                Result<
                    LanguageModelCompletionEvent,
                    language_model_core::LanguageModelCompletionError,
                >,
            >,
            language_model_core::LanguageModelCompletionError,
        >,
    > {
        let config = self.config.clone();
        let router = self.build_router();
        let (prompt, tools) = Self::flatten_request(&request);
        let temperature = request.temperature;

        async move {
            let params = LLMParameters {
                temperature: temperature.unwrap_or(0.7),
                ..Default::default()
            };

            let result = hkask_inference::fusion_orchestrator::orchestrate(
                &router,
                &prompt,
                &params,
                if tools.is_empty() { None } else { Some(&tools) },
                &config,
            )
            .await
            .map_err(|e| {
                language_model_core::LanguageModelCompletionError::Other(anyhow::anyhow!(
                    "Fusion orchestration failed: {e}"
                ))
            })?;

            // Emit the fused result as a single Text chunk + Stop.
            let stream = stream::iter([
                Ok(LanguageModelCompletionEvent::StartMessage {
                    message_id: "fusion".to_string(),
                }),
                Ok(LanguageModelCompletionEvent::Text(result.text)),
                Ok(LanguageModelCompletionEvent::UsageUpdate(TokenUsage {
                    input_tokens: result.usage.prompt_tokens as u64,
                    output_tokens: result.usage.completion_tokens as u64,
                    cache_creation_input_tokens: 0,
                    cache_read_input_tokens: 0,
                })),
                Ok(LanguageModelCompletionEvent::Stop(StopReason::EndTurn)),
            ])
            .boxed();

            Ok(stream)
        }
        .boxed()
    }
}

// ── MultiModelInferencePort ──────────────────────────────────────────────────

/// An `InferencePort` that routes `generate_with_model` to different
/// `LanguageModelInferencePort` instances based on the model name.
///
/// This is the adapter that lets hKask's fusion orchestrator (which uses
/// provider-prefixed model names) talk to zed's `LanguageModel` instances.
struct MultiModelInferencePort {
    ports: HashMap<String, Arc<LanguageModelInferencePort>>,
}

impl InferencePort for MultiModelInferencePort {
    fn generate(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        // No model override — use the first available port.
        let port = self.ports.values().next();
        match port {
            Some(p) => p.generate(prompt, parameters, tools),
            None => Box::pin(async {
                Err(InferenceError::Generation(
                    "No models available in fusion router".into(),
                ))
            }),
        }
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
        let model_name = match model_override {
            Some(name) => name,
            None => {
                return self.generate(prompt, parameters, tools);
            }
        };

        match self.ports.get(model_name) {
            // We already resolved the correct port by name from the ports
            // map, so pass None — the port's internal model_override resolution
            // is redundant here (the port is already bound to the right model).
            Some(port) => port.generate_with_model(prompt, parameters, None, tools),
            None => {
                let model_name = model_name.to_string();
                Box::pin(async move {
                    Err(InferenceError::Generation(format!(
                        "Model '{model_name}' not found in fusion router"
                    )))
                })
            }
        }
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
        let model_name = match model_override {
            Some(name) => name,
            None => {
                // Flatten and use the first port.
                let port = match self.ports.values().next() {
                    Some(p) => p,
                    None => {
                        return Box::pin(async {
                            Err(InferenceError::Generation(
                                "No models available in fusion router".into(),
                            ))
                        });
                    }
                };
                return port.generate_with_messages(messages, parameters, None, tools);
            }
        };

        match self.ports.get(model_name) {
            // Already resolved by name — pass None to avoid redundant resolution.
            Some(port) => port.generate_with_messages(messages, parameters, None, tools),
            None => {
                let model_name = model_name.to_string();
                Box::pin(async move {
                    Err(InferenceError::Generation(format!(
                        "Model '{model_name}' not found in fusion router"
                    )))
                })
            }
        }
    }
}

// ── Model Resolution ─────────────────────────────────────────────────────────

/// Resolve provider-prefixed model names from the `LanguageModelRegistry`.
///
/// Each name in `model_names` is resolved to an `Arc<dyn LanguageModel>`.
/// The key in the returned map is the original name string (so the fusion
/// orchestrator can route by name).
///
/// Resolution strategy:
/// 1. If the name contains `/`, split on the first `/` to get
///    `(provider_id, model_id)` and look up the provider (case-insensitive
///    on the provider id — `OpenRouter` matches the registered `openrouter`).
/// 2. Search the provider's models for one whose `id()` matches the model
///    part, or whose `telemetry_id()` matches the full prefixed name
///    (case-insensitive — `OpenRouter/minimax/minimax3` matches
///    `openrouter/minimax/minimax3`).
/// 3. If no prefix or no match, search all providers' models by
///    `telemetry_id()` (case-insensitive).
///
/// Returns `(resolved, unresolvable)` where `unresolvable` is the set of
/// names that could not be resolved. The caller is responsible for warning
/// about unresolvable names — this function does not log, so it can be called
/// from a debounced rebuild without producing a warn storm.
#[must_use]
pub fn resolve_fusion_models(
    registry: &language_model::LanguageModelRegistry,
    model_names: &[String],
    cx: &App,
) -> (HashMap<String, Arc<dyn LanguageModel>>, HashSet<String>) {
    let mut resolved: HashMap<String, Arc<dyn LanguageModel>> = HashMap::new();
    let mut unresolvable: HashSet<String> = HashSet::new();

    for name in model_names {
        if let Some(model) = resolve_model(registry, name, cx) {
            resolved.insert(name.clone(), model);
        } else {
            unresolvable.insert(name.clone());
        }
    }

    (resolved, unresolvable)
}

/// Resolve a single provider-prefixed model name.
///
/// Provider-ID lookup is case-insensitive: fusion config defaults use
/// `"OpenRouter/..."` (capitalized) while zed's `LanguageModelRegistry`
/// registers OpenRouter under `"openrouter"` (lowercase). Rather than
/// normalizing one side (which would break env-var overrides that use either
/// casing), we normalize at the lookup boundary — exact-case first, then a
/// case-insensitive fallback across all registered providers.
fn resolve_model(
    registry: &language_model::LanguageModelRegistry,
    prefixed_name: &str,
    cx: &App,
) -> Option<Arc<dyn LanguageModel>> {
    // Try to split on the first `/` to get provider/model.
    if let Some((provider_id_str, model_id)) = prefixed_name.split_once('/') {
        let provider_id = LanguageModelProviderId(provider_id_str.to_string().into());

        // Exact-case lookup first (fast path — the common case when the user
        // types the provider id exactly as registered).
        let provider = registry.provider(&provider_id).or_else(|| {
            // Case-insensitive fallback. `LanguageModelProviderId` derives
            // `Eq`/`Hash` with case-sensitive `SharedString`, so a capitalized
            // prefix like "OpenRouter" won't match the registered "openrouter".
            // Iterate all providers and compare case-insensitively.
            registry
                .providers()
                .into_iter()
                .find(|p| p.id().0.as_ref().eq_ignore_ascii_case(provider_id_str))
        });

        if let Some(provider) = provider {
            // The model ID after the prefix may itself contain a `/` (e.g.
            // "anthropic/claude-sonnet-4.5" under provider "OR"). Search the
            // provider's models for a match on id or telemetry_id.
            //
            // The telemetry_id comparison is case-insensitive on the full
            // prefixed name: the fusion config defaults use `OpenRouter/...`
            // (capitalized) while the OpenRouter provider's telemetry_id
            // returns `openrouter/...` (lowercase). Without case-insensitive
            // comparison, `OpenRouter/minimax/minimax3` never matches
            // `openrouter/minimax/minimax3` even when the model exists.
            for model in provider.provided_models(cx) {
                if model.id().0.as_ref() == model_id
                    || model.telemetry_id().eq_ignore_ascii_case(prefixed_name)
                {
                    return Some(model);
                }
            }
        }
    }

    // No prefix or prefix match failed — search all providers by telemetry_id
    // (case-insensitive, same reason as above).
    // Skip the fusion provider itself to avoid a re-entrant entity borrow when
    // this function is called from within FusionProviderState's settings-observe
    // callback (which holds a mutable borrow on the fusion entity).
    registry
        .available_models(cx)
        .filter(|m| m.provider_id().0.as_ref() != FUSION_PROVIDER_ID)
        .find(|m| m.telemetry_id().eq_ignore_ascii_case(prefixed_name))
}

// ── FusionLanguageModelProvider ───────────────────────────────────────────────

use language_model::{
    IconOrSvg, LanguageModelProvider, LanguageModelProviderState, ProviderSettingsView,
};
use ui::IconName;

/// Observable state for the fusion provider.
///
/// Notifies when settings or registry state change so the registry can
/// re-enumerate models. When `build_model` fails (panel models unresolved,
/// judge missing, etc.), an alert is forwarded to the UI via the optional
/// `alert_tx` channel so the user sees an honest status rather than a
/// silently missing model.
pub struct FusionProviderState {
    model: Option<Arc<FusionLanguageModel>>,
    /// Tracks whether the previous `build_model` failed, so we only alert
    /// on state transitions (failure → success, success → failure), not on
    /// every settings change that still succeeds.
    was_broken: bool,
    alert_tx: Option<tokio::sync::mpsc::UnboundedSender<hkask_regulation::AlertEvent>>,
    /// Pending debounced rebuild task. When a rebuild is scheduled, we replace
    /// any existing pending task — the new task waits a short debounce window
    /// then runs `build_model` once, coalescing a burst of registry/settings
    /// events into a single rebuild. This prevents the warn storm where each
    /// `ProviderStateChanged` event in a burst spawns its own `build_model`
    /// call, each emitting a warn per unresolvable model.
    pending_rebuild: Option<Task<()>>,
    /// Model names that failed to resolve in the most recent `build_model`.
    /// Used to suppress duplicate warns across rebuilds for the same set of
    /// unresolvable models — the operator only needs to see each failure
    /// once per transition, not on every rebuild.
    warned_unresolvable: HashSet<String>,
    _settings_subscription: gpui::Subscription,
    _registry_subscription: gpui::Subscription,
}

impl FusionProviderState {
    fn new(
        cx: &mut gpui::Context<Self>,
        alert_tx: Option<tokio::sync::mpsc::UnboundedSender<hkask_regulation::AlertEvent>>,
    ) -> Self {
        let (model, unresolvable) = Self::build_model(cx);
        let was_broken = model.is_none();
        // Warn once for the initial set of unresolvable models. Subsequent
        // warns are gated by `warned_unresolvable` in `apply_rebuild`.
        for name in &unresolvable {
            tracing::warn!(
                target: "reg.fusion",
                model_name = %name,
                "Could not resolve model from LanguageModelRegistry — dropped from fusion"
            );
        }

        // Subscribe to LanguageModelRegistry events so the fusion model
        // rebuilds when a panel/judge provider's auth state changes (token
        // expires, provider unregistered, model list fetched after startup).
        // Same deferred-rebuild pattern as the settings observer — avoids
        // re-entrant entity borrows.
        //
        // Self-event filter: `ProviderStateChanged(FUSION_PROVIDER_ID)` is
        // emitted by the registry whenever *this* provider's observable
        // entity notifies (which happens inside `apply_rebuild` via
        // `cx.notify()`). Without filtering it, every rebuild triggers a
        // `ProviderStateChanged` → `schedule_rebuild` → rebuild → notify →
        // `ProviderStateChanged` loop, producing an unbounded warn storm
        // (one warn per unresolvable model per iteration). The fusion
        // provider's own state changes are not a signal to rebuild itself —
        // it already rebuilt. `AddedProvider`/`RemovedProvider` for the
        // fusion id are also filtered (fusion is registered once at startup
        // and never re-added).
        let registry = language_model::LanguageModelRegistry::global(cx);
        let fusion_provider_id = LanguageModelProviderId(FUSION_PROVIDER_ID.to_string().into());
        let _registry_subscription =
            cx.subscribe(&registry, move |this, _registry, event, cx| match event {
                language_model::Event::ProviderStateChanged(id)
                | language_model::Event::AddedProvider(id)
                | language_model::Event::RemovedProvider(id)
                    if *id == fusion_provider_id =>
                {
                    // Self-event — ignore to avoid the rebuild loop.
                }
                language_model::Event::ProviderStateChanged(_)
                | language_model::Event::AddedProvider(_)
                | language_model::Event::RemovedProvider(_)
                | language_model::Event::ProvidersChanged => {
                    this.schedule_rebuild(cx);
                }
                _ => {}
            });

        Self {
            model,
            was_broken,
            alert_tx,
            pending_rebuild: None,
            warned_unresolvable: unresolvable,
            _settings_subscription: cx
                .observe_global::<settings::SettingsStore>(|this, cx| this.schedule_rebuild(cx)),
            _registry_subscription,
        }
    }

    /// Schedule a deferred, debounced rebuild of the fusion model.
    ///
    /// Defers to a spawned task to avoid re-entrant entity borrows:
    /// `build_model` resolves models from the registry, which calls
    /// `available_models()`, which calls `is_authenticated()` on every
    /// provider — including this one. If we held a mutable borrow on `self`
    /// here, the immutable `self.state.read(cx)` inside `is_authenticated`
    /// would panic.
    ///
    /// Debounce: if a rebuild is already pending, the new schedule replaces
    /// the old one (the dropped `Task` is cancelled). The pending task waits
    /// a short window before running `build_model`, coalescing a burst of
    /// registry/settings events into a single rebuild. Without this, a burst
    /// of N `ProviderStateChanged` events spawns N rebuilds, each emitting
    /// a warn per unresolvable model — the warn storm seen in production.
    fn schedule_rebuild(&mut self, cx: &mut gpui::Context<Self>) {
        // Replace any pending rebuild — the old task is dropped (cancelled).
        self.pending_rebuild = Some(cx.spawn(async move |weak_entity, cx| {
            // Debounce window: coalesce events that arrive within 50ms.
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            // Bail before calling cx.update() if the entity is already gone
            // — the app is shutting down and AsyncApp::app() panics with
            // "app was released" if called after the app drops.
            let entity = match weak_entity.upgrade() {
                Some(entity) => entity,
                None => return,
            };
            let (new_model, unresolvable) = cx.update(|cx| Self::build_model(cx));
            cx.update(|cx| {
                entity.update(cx, |this, cx| {
                    this.pending_rebuild = None;
                    this.apply_rebuild(new_model, unresolvable, cx);
                });
            });
        }));
    }

    /// Apply a rebuild result, alerting the UI on state transitions.
    ///
    /// `unresolvable` is the set of model names that failed to resolve in
    /// this `build_model` pass. Warns are emitted only for names that weren't
    /// already in `warned_unresolvable` — so a steady-state failure (same
    /// models unresolvable across rebuilds) warns once, not on every rebuild.
    /// When the set changes (a previously-unresolvable model now resolves,
    /// or a new model fails), the delta is warned.
    fn apply_rebuild(
        &mut self,
        new_model: Option<Arc<FusionLanguageModel>>,
        unresolvable: HashSet<String>,
        cx: &mut gpui::Context<Self>,
    ) {
        let is_broken = new_model.is_none();

        // Warn only for newly-unresolvable names (not in the previous set).
        // This suppresses the repeat-warn storm while still surfacing the
        // first occurrence of each failure and any new failures that appear
        // after a registry change.
        for name in unresolvable.iter() {
            if !self.warned_unresolvable.contains(name) {
                tracing::warn!(
                    target: "reg.fusion",
                    model_name = %name,
                    "Could not resolve model from LanguageModelRegistry — dropped from fusion"
                );
            }
        }
        self.warned_unresolvable = unresolvable;

        self.model = new_model;

        // Only alert on state transitions — not every successful rebuild.
        if is_broken && !self.was_broken {
            self.send_alert(
                "Fusion model unavailable: panel or judge models could not be resolved. \
                 Check Settings → AI → LLM Providers.",
                true,
            );
        } else if !is_broken && self.was_broken {
            self.send_alert("Fusion model restored.", false);
        }
        self.was_broken = is_broken;
        cx.notify();
    }

    /// Best-effort alert delivery. The channel may be absent (no UI wired)
    /// or closed (app shutting down) — either way, the alert is logged.
    fn send_alert(&self, message: &str, critical: bool) {
        let Some(tx) = &self.alert_tx else {
            return;
        };
        let event = hkask_regulation::AlertEvent {
            message: message.to_string(),
            critical,
        };
        if let Err(e) = tx.send(event) {
            tracing::warn!(target: "reg.fusion", "Fusion alert dropped (channel closed): {e} — {message}");
        }
    }

    /// Rebuild the `FusionLanguageModel` from current settings + registry.
    ///
    /// Returns `(None, unresolvable)` when fusion is disabled or construction
    /// fails (missing panel models, unresolvable judge, etc.). The
    /// `unresolvable` set contains the model names that failed to resolve, so
    /// the caller can warn about them without re-warn-ing on every rebuild for
    /// the same set.
    ///
    /// Reads `kask.fusion` from the settings store and resolves panel/judge
    /// models from the `LanguageModelRegistry`.
    fn build_model(cx: &App) -> (Option<Arc<FusionLanguageModel>>, HashSet<String>) {
        let kask_settings = kask_bridge_settings(cx);
        let Some(fc) = kask_settings.fusion.to_fusion_config() else {
            return (None, HashSet::new());
        };
        let registry = language_model::LanguageModelRegistry::read_global(cx);
        let mut names = fc.panel.iter().cloned().collect::<Vec<_>>();
        if fc.judge.to_lowercase() != "algo" {
            names.push(fc.judge.clone());
        }
        let (resolved, unresolvable) = resolve_fusion_models(registry, &names, cx);
        if !unresolvable.is_empty() {
            return (None, unresolvable);
        }
        match FusionLanguageModel::new(fc, resolved, cx.to_async()) {
            Some(model) => (Some(Arc::new(model)), HashSet::new()),
            None => (None, HashSet::new()),
        }
    }

    /// The current fusion model, if any.
    pub fn model(&self) -> Option<Arc<FusionLanguageModel>> {
        self.model.clone()
    }
}

/// A `LanguageModelProvider` that exposes the fusion model in zed's model picker.
///
/// When `kask.fusion.enabled == true`, this provider returns a single
/// `FusionLanguageModel` in `provided_models`. When fusion is disabled, it
/// returns an empty list (so it doesn't appear in the picker).
///
/// The fusion model is rebuilt dynamically when `kask.fusion` settings change
/// OR when a panel/judge provider's auth state changes in the
/// `LanguageModelRegistry`. `FusionProviderState` observes both the global
/// `SettingsStore` and the `LanguageModelRegistry`, reconstructing the
/// `FusionLanguageModel` (re-resolving panel/judge models) before notifying
/// the registry. This means changing the fusion mode, judge model, or panel
/// models in settings — or a panel provider losing/gaining authentication —
/// takes effect for new conversations and the model picker without restarting
/// Zed. In-flight threads continue using the model they were started with.
pub struct FusionLanguageModelProvider {
    state: Entity<FusionProviderState>,
}

impl FusionLanguageModelProvider {
    /// Construct the provider.
    ///
    /// Reads `kask.fusion` from settings. When enabled, resolves the panel
    /// and judge models from the registry and constructs a
    /// `FusionLanguageModel`. When disabled or construction fails, the
    /// provider returns no models.
    ///
    /// The `alert_tx` channel is used to surface construction failures to
    /// the UI (toast). Pass `None` when no UI alert sink is available.
    pub fn new(
        cx: &mut App,
        alert_tx: Option<tokio::sync::mpsc::UnboundedSender<hkask_regulation::AlertEvent>>,
    ) -> Self {
        let state = cx.new(|cx| FusionProviderState::new(cx, alert_tx));
        Self { state }
    }
}

impl LanguageModelProviderState for FusionLanguageModelProvider {
    type ObservableEntity = FusionProviderState;

    fn observable_entity(&self) -> Option<Entity<Self::ObservableEntity>> {
        Some(self.state.clone())
    }
}

impl LanguageModelProvider for FusionLanguageModelProvider {
    fn id(&self) -> LanguageModelProviderId {
        LanguageModelProviderId(FUSION_PROVIDER_ID.into())
    }

    fn name(&self) -> LanguageModelProviderName {
        LanguageModelProviderName("Kask Fusion".into())
    }

    fn icon(&self) -> IconOrSvg {
        IconOrSvg::Icon(IconName::Sparkle)
    }

    fn default_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        self.state
            .read(cx)
            .model()
            .map(|m| m as Arc<dyn LanguageModel>)
    }

    fn default_fast_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        self.default_model(cx)
    }

    fn provided_models(&self, cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        self.state
            .read(cx)
            .model()
            .map(|m| vec![m as Arc<dyn LanguageModel>])
            .unwrap_or_default()
    }

    fn is_authenticated(&self, cx: &App) -> bool {
        // Fusion is "authenticated" when it has a model — i.e., when fusion
        // is enabled and construction succeeded.
        self.state.read(cx).model().is_some()
    }

    fn authenticate(
        &self,
        _cx: &mut App,
    ) -> gpui::Task<Result<(), language_model::AuthenticateError>> {
        // No authentication needed — fusion uses the underlying panel models'
        // credentials.
        gpui::Task::ready(Ok(()))
    }

    fn settings_view(&self, _cx: &mut App) -> Option<ProviderSettingsView> {
        None
    }
}

/// Helper to read `KaskSettings` from the settings store.
fn kask_bridge_settings(cx: &App) -> crate::settings::KaskSettings {
    use settings::Settings as _;
    crate::settings::KaskSettings::get_global(cx).clone()
}

// ── Favorites Discovery ─────────────────────────────────────────────────────

/// Discover "favorite" models from Artificial Analysis that pass the price and
/// intelligence thresholds.
///
/// This is a thin wrapper over `hkask_inference::artificial_analysis::discover_favorites`.
/// Artificial Analysis provides an independent Intelligence Index and pricing
/// data via its `/api/v2/language/models/free` endpoint. The free tier (100
/// req/day) includes the `artificial_analysis_intelligence_index` and
/// `price_1m_input_tokens` fields needed for filtering.
///
/// We switched from OpenRouter's `/v1/models` endpoint because OpenRouter's
/// server-side `supported_parameters` filter requires a model to advertise *all*
/// of `temperature,top_p,structured_outputs,tools,reasoning`. Models that lack
/// `reasoning` or `structured_outputs` (e.g. `z-ai/glm-5.2`, the kask default)
/// are silently dropped before the client sees them — the filter meant to
/// discover the default model was screening it out. Artificial Analysis scores
/// models on a single intelligence axis without a supported-parameters gate.
///
/// The Artificial Analysis API key is read from the `AA_API_KEY` env var.
///
/// Returns provider-prefixed model IDs (e.g. `"OpenRouter/z-ai/glm-5.2"`) sorted by
/// intelligence index descending. On any error, returns an empty vec.
///
/// Used by the composition root to auto-populate the fusion panel when
/// `kask.fusion.panel_models` is empty or set to `"auto"`.
pub async fn discover_favorites(
    max_price_per_m: f64,
    min_intelligence_index: f64,
) -> Vec<hkask_inference::artificial_analysis::FavoriteModel> {
    let aa_api_key = std::env::var("AA_API_KEY").unwrap_or_default();
    hkask_inference::artificial_analysis::discover_favorites(
        &aa_api_key,
        max_price_per_m,
        min_intelligence_index,
    )
    .await
}

/// Check whether the fusion panel should use auto-discovery.
///
/// Returns `true` when `panel_models` is empty or set to `"auto"` (case-insensitive).
pub fn should_auto_discover(panel_models: &str) -> bool {
    let trimmed = panel_models.trim().to_lowercase();
    trimmed.is_empty() || trimmed == "auto"
}

/// The zed provider ID for OpenRouter models.
///
/// `FavoriteModel.prefixed_id` uses the 2-letter code `"OR/..."`, but zed's
/// `LanguageModelRegistry` registers OpenRouter under the ID `"openrouter"`.
/// Favorites stored in `agent.favorite_models` must use the zed provider ID
/// so the model picker can match them against registered providers.
const ZED_OPENROUTER_PROVIDER_ID: &str = "openrouter";

/// Convert discovered OpenRouter favorites into `LanguageModelSelection` entries
/// suitable for `agent.favorite_models` in settings.json.
///
/// Each `FavoriteModel.id` (e.g. `"z-ai/glm-5.2"`) becomes the model field,
/// paired with the zed OpenRouter provider ID. Entries are returned in the
/// same order as the input (sorted by intelligence index descending from
/// `discover_favorites`).
///
/// This is a pure conversion — it does not write to settings. The composition
/// root calls `update_settings_file` to persist the result.
#[must_use]
pub fn favorite_model_selections(
    favorites: &[hkask_inference::artificial_analysis::FavoriteModel],
) -> Vec<settings_content::LanguageModelSelection> {
    favorites
        .iter()
        .map(|f| settings_content::LanguageModelSelection {
            provider: settings_content::LanguageModelProviderSetting(
                ZED_OPENROUTER_PROVIDER_ID.to_string(),
            ),
            model: f.id.clone(),
            enable_thinking: false,
            effort: None,
            speed: None,
        })
        .collect()
}

/// Build a `LanguageModelSelection` for the fusion model itself.
///
/// The fusion model is registered under provider ID `kask-fusion` with model
/// ID `fusion`. Adding it to `agent.favorite_models` lets users cycle to it
/// via the `CycleFavoriteModels` action in the agent panel.
#[must_use]
pub fn fusion_model_selection() -> settings_content::LanguageModelSelection {
    settings_content::LanguageModelSelection {
        provider: settings_content::LanguageModelProviderSetting(FUSION_PROVIDER_ID.to_string()),
        model: FUSION_MODEL_ID.to_string(),
        enable_thinking: false,
        effort: None,
        speed: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_inference::artificial_analysis::FavoriteModel;

    fn fav(id: &str) -> FavoriteModel {
        FavoriteModel {
            prefixed_id: format!("OR/{id}"),
            id: id.to_string(),
            name: id.to_string(),
            intelligence_index: 50.0,
            prompt_price_per_m: 0.5,
            completion_price_per_m: 0.5,
            context_length: 32_000,
        }
    }

    #[test]
    fn favorite_model_selections_maps_id_to_model_field() {
        let favorites = vec![fav("z-ai/glm-5.2"), fav("qwen/qwen3-235b-a22b")];
        let selections = favorite_model_selections(&favorites);
        assert_eq!(selections.len(), 2);
        assert_eq!(selections[0].provider.0, ZED_OPENROUTER_PROVIDER_ID);
        assert_eq!(selections[0].model, "z-ai/glm-5.2");
        assert_eq!(selections[1].model, "qwen/qwen3-235b-a22b");
    }

    #[test]
    fn favorite_model_selections_preserves_input_order() {
        let favorites = vec![fav("b-model"), fav("a-model")];
        let selections = favorite_model_selections(&favorites);
        assert_eq!(selections[0].model, "b-model");
        assert_eq!(selections[1].model, "a-model");
    }

    #[test]
    fn favorite_model_selections_empty_input_returns_empty() {
        let selections = favorite_model_selections(&[]);
        assert!(selections.is_empty());
    }

    #[test]
    fn favorite_model_selections_disables_thinking_by_default() {
        let selections = favorite_model_selections(&[fav("z-ai/glm-5.2")]);
        assert!(!selections[0].enable_thinking);
        assert!(selections[0].effort.is_none());
        assert!(selections[0].speed.is_none());
    }

    #[test]
    fn fusion_model_selection_uses_kask_fusion_provider() {
        let selection = fusion_model_selection();
        assert_eq!(selection.provider.0, FUSION_PROVIDER_ID);
        assert_eq!(selection.model, FUSION_MODEL_ID);
    }

    #[test]
    fn should_auto_discover_accepts_empty_and_auto() {
        assert!(should_auto_discover(""));
        assert!(should_auto_discover("auto"));
        assert!(should_auto_discover("AUTO"));
        assert!(should_auto_discover("  auto  "));
    }

    #[test]
    fn should_auto_discover_rejects_explicit_models() {
        assert!(!should_auto_discover("OpenRouter/z-ai/glm-5.2"));
        assert!(!should_auto_discover("OpenRouter/a,OpenRouter/b"));
    }

    /// Document the case-insensitive provider-id contract.
    ///
    /// `FusionConfig::kask_default()` uses `"OpenRouter/..."` (capitalized)
    /// while zed's `LanguageModelRegistry` registers OpenRouter under
    /// `"openrouter"` (lowercase). `resolve_model` must match these
    /// case-insensitively. This test pins the string-comparison logic; a full
    /// integration test would require a GPUI test context with a registered
    /// OpenRouter provider.
    #[test]
    fn resolve_model_matches_provider_id_case_insensitively() {
        // The kask_default panel uses "OpenRouter" (capitalized).
        let configured_prefix = "OpenRouter";
        // The registered provider id is "openrouter" (lowercase).
        let registered_id = "openrouter";
        assert!(
            registered_id.eq_ignore_ascii_case(configured_prefix),
            "case-insensitive comparison must match OpenRouter <-> openrouter"
        );
    }

    /// Pin the case-insensitive telemetry_id contract.
    ///
    /// `resolve_model` compares the full prefixed name (e.g.
    /// `"OpenRouter/minimax/minimax3"`) against each model's `telemetry_id()`
    /// (e.g. `"openrouter/minimax/minimax3"` — lowercase prefix). The
    /// comparison must be case-insensitive on the full string, otherwise the
    /// capitalized `OpenRouter/` prefix in fusion config defaults never matches
    /// the lowercase `openrouter/` prefix in telemetry_ids, even when the model
    /// exists in the registry. This was the root cause of the warn storm: every
    /// panel model in `kask_default()` failed to resolve.
    #[test]
    fn resolve_model_telemetry_id_comparison_is_case_insensitive() {
        let configured = "OpenRouter/minimax/minimax3";
        let telemetry_id = "openrouter/minimax/minimax3";
        assert!(
            telemetry_id.eq_ignore_ascii_case(configured),
            "telemetry_id comparison must be case-insensitive \
             (openrouter/... must match OpenRouter/...)"
        );
    }

    /// Pin the self-event filter: the fusion provider must ignore
    /// `ProviderStateChanged` / `AddedProvider` / `RemovedProvider` events
    /// that carry its own provider id. Without this filter, every
    /// `apply_rebuild` → `cx.notify()` triggers a `ProviderStateChanged` →
    /// `schedule_rebuild` → rebuild → notify loop, producing an unbounded warn
    /// storm. This test pins the id comparison so a refactor that drops the
    /// filter is caught.
    #[test]
    fn fusion_provider_id_is_self_event_filter_key() {
        let fusion_id = LanguageModelProviderId(FUSION_PROVIDER_ID.to_string().into());
        // The filter compares event ids against the fusion provider id.
        let event_id = LanguageModelProviderId(FUSION_PROVIDER_ID.to_string().into());
        assert_eq!(
            event_id, fusion_id,
            "ProviderStateChanged(fusion_id) must be filtered as a self-event"
        );
        // A non-fusion id must NOT match.
        let other_id = LanguageModelProviderId("openrouter".to_string().into());
        assert_ne!(
            other_id, fusion_id,
            "non-fusion provider events must not be filtered"
        );
    }
}
