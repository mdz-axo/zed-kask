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

use std::collections::HashMap;
use std::sync::Arc;

use futures::stream::BoxStream;
use futures_util::{FutureExt, StreamExt, stream};
use gpui::{App, AppContext, AsyncApp, Entity};
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

        let mode_str = config.mode.as_str();
        Some(Self {
            id: LanguageModelId::from(FUSION_MODEL_ID.to_string()),
            name: LanguageModelName::from(format!(
                "Fusion ({mode_str}, {} panelists)",
                config.panel.len()
            )),
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
            // LanguageModelInferencePort ignores model_override (each port is
            // bound to a single model at construction), so we pass None.
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
            // LanguageModelInferencePort ignores model_override.
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
///    `(provider_id, model_id)` and look up the provider.
/// 2. Search the provider's models for one whose `id()` or `telemetry_id()`
///    matches the model part.
/// 3. If no prefix or no match, search all providers' models by
///    `telemetry_id()`.
///
/// Models that can't be resolved are silently skipped (with a warning).
#[must_use]
pub fn resolve_fusion_models(
    registry: &language_model::LanguageModelRegistry,
    model_names: &[String],
    cx: &App,
) -> HashMap<String, Arc<dyn LanguageModel>> {
    let mut resolved: HashMap<String, Arc<dyn LanguageModel>> = HashMap::new();

    for name in model_names {
        if let Some(model) = resolve_model(registry, name, cx) {
            resolved.insert(name.clone(), model);
        } else {
            tracing::warn!(
                target: "reg.fusion",
                model_name = %name,
                "Could not resolve model from LanguageModelRegistry — dropped from fusion"
            );
        }
    }

    resolved
}

/// Resolve a single provider-prefixed model name.
fn resolve_model(
    registry: &language_model::LanguageModelRegistry,
    prefixed_name: &str,
    cx: &App,
) -> Option<Arc<dyn LanguageModel>> {
    // Try to split on the first `/` to get provider/model.
    if let Some((provider_id_str, model_id)) = prefixed_name.split_once('/') {
        let provider_id = LanguageModelProviderId(provider_id_str.to_string().into());
        if let Some(provider) = registry.provider(&provider_id) {
            // The model ID after the prefix may itself contain a `/` (e.g.
            // "anthropic/claude-sonnet-4.5" under provider "OR"). Search the
            // provider's models for a match on id or telemetry_id.
            for model in provider.provided_models(cx) {
                if model.id().0.as_ref() == model_id || model.telemetry_id() == prefixed_name {
                    return Some(model);
                }
            }
        }
    }

    // No prefix or prefix match failed — search all providers by telemetry_id.
    registry
        .available_models(cx)
        .find(|m| m.telemetry_id() == prefixed_name)
}

// ── FusionLanguageModelProvider ───────────────────────────────────────────────

use gpui::Entity;
use language_model::{LanguageModelProvider, LanguageModelProviderState, ProviderSettingsView};
use ui::{IconName, IconOrSvg};

/// Observable state for the fusion provider.
///
/// Notifies when settings change so the registry can re-enumerate models.
pub struct FusionProviderState {
    _settings_subscription: gpui::Subscription,
}

impl FusionProviderState {
    fn new(cx: &mut gpui::Context<Self>) -> Self {
        Self {
            _settings_subscription: cx.observe_global::<settings::SettingsStore>(|_, cx| {
                cx.notify();
            }),
        }
    }
}

/// A `LanguageModelProvider` that exposes the fusion model in zed's model picker.
///
/// When `kask.fusion.enabled == true`, this provider returns a single
/// `FusionLanguageModel` in `provided_models`. When fusion is disabled, it
/// returns an empty list (so it doesn't appear in the picker).
///
/// The fusion model is constructed at provider construction time (when
/// `AsyncApp` is available) and held for the lifetime of the provider.
/// If the fusion config changes, the user must restart Zed for the new
/// config to take effect (a limitation we accept for Slice 3 — dynamic
/// reconfiguration is a future enhancement).
pub struct FusionLanguageModelProvider {
    state: Entity<FusionProviderState>,
    model: Option<Arc<FusionLanguageModel>>,
}

impl FusionLanguageModelProvider {
    /// Construct the provider.
    ///
    /// Reads `kask.fusion` from settings. When enabled, resolves the panel
    /// and judge models from the registry and constructs a
    /// `FusionLanguageModel`. When disabled or construction fails, `model`
    /// is `None` and the provider returns no models.
    pub fn new(cx: &mut App) -> Self {
        let state = cx.new(|cx| FusionProviderState::new(cx));

        let kask_settings = kask_bridge_settings(cx);
        let model = kask_settings
            .fusion
            .to_fusion_config()
            .and_then(|fc| {
                let registry = language_model::LanguageModelRegistry::read_global(cx);
                let mut names = fc.panel.iter().cloned().collect::<Vec<_>>();
                if fc.judge.to_lowercase() != "algo" {
                    names.push(fc.judge.clone());
                }
                let resolved = resolve_fusion_models(registry, &names, cx);
                FusionLanguageModel::new(fc, resolved, cx.to_async())
            })
            .map(Arc::new);

        Self { state, model }
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

    fn default_model(&self, _cx: &App) -> Option<Arc<dyn LanguageModel>> {
        self.model.clone().map(|m| m as Arc<dyn LanguageModel>)
    }

    fn default_fast_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        self.default_model(cx)
    }

    fn provided_models(&self, _cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        self.model
            .clone()
            .map(|m| vec![m as Arc<dyn LanguageModel>])
            .unwrap_or_default()
    }

    fn is_authenticated(&self, _cx: &App) -> bool {
        // Fusion is "authenticated" when it has a model — i.e., when fusion
        // is enabled and construction succeeded.
        self.model.is_some()
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
