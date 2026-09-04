use anyhow::Result;
use credentials_provider::CredentialsProvider;
use futures::{FutureExt, StreamExt, future::BoxFuture};
use gpui::{App, AppContext, AsyncApp, Context, Entity, Task};
use http_client::{CustomHeaders, HttpClient};
use language_model::{
    AuthenticateError, IconOrSvg, LanguageModel, LanguageModelCompletionError,
    LanguageModelCompletionEvent, LanguageModelEffortLevel, LanguageModelId, LanguageModelName,
    LanguageModelProvider, LanguageModelProviderId, LanguageModelProviderName,
    LanguageModelProviderState, LanguageModelRequest, LanguageModelToolChoice,
    LanguageModelToolSchemaFormat, ProviderSettingsView, RateLimiter, SubPageProviderSettings,
};
use open_ai::{
    ResponseStreamEvent,
    list_models::list_models,
    responses::{Request as ResponseRequest, StreamEvent as ResponsesStreamEvent, stream_response},
    stream_completion,
};
use settings::Settings;
use std::sync::Arc;
use ui::IconName;

use crate::provider::api_compatible::{
    ApiCompatibleProviderConfigurationView, ApiCompatibleProviderSettings,
    ApiCompatibleProviderState,
};
use crate::provider::open_ai::{
    OpenAiEventMapper, OpenAiResponseEventMapper, into_open_ai, into_open_ai_response,
};
pub use settings::OpenAiCompatibleAvailableModel as AvailableModel;
pub use settings::OpenAiCompatibleModelCapabilities as ModelCapabilities;

const API_KEY_PLACEHOLDER: &str = "000000000000000000000000000000000000000000000000000";

#[derive(Default, Clone, Debug, PartialEq)]
pub struct OpenAiCompatibleSettings {
    pub api_url: String,
    pub available_models: Vec<AvailableModel>,
    pub custom_headers: CustomHeaders,
    /// Whether to auto-discover models via the provider's `/v1/models` endpoint.
    /// Defaults to `true` — API key presence is the effective opt-in.
    pub auto_discover: bool,
}

impl ApiCompatibleProviderSettings for OpenAiCompatibleSettings {
    fn api_url(&self) -> &str {
        &self.api_url
    }
}

pub type State = ApiCompatibleProviderState<OpenAiCompatibleSettings>;

/// Discovery state for auto-discovering models via the provider's `/v1/models`
/// endpoint. Held alongside the shared `ApiCompatibleProviderState` so the
/// generic state stays generic (Anthropic-compatible providers don't carry
/// unused discovery fields).
///
/// Mirrors the OpenRouter `State` pattern: a `fetch_models_task` is spawned on
/// authenticate / settings change, and `fetched_models` is merged into
/// `provided_models` by the provider.
pub struct DiscoveryState {
    /// Models discovered via `/v1/models`. Empty until a successful fetch.
    fetched_models: Vec<AvailableModel>,
    /// In-flight fetch task. Replaced on re-authentication or settings change.
    fetch_models_task: Option<Task<Result<()>>>,
}

impl DiscoveryState {
    fn new() -> Self {
        Self {
            fetched_models: Vec::new(),
            fetch_models_task: None,
        }
    }

    /// Spawn (or replace) the fetch-models task. Reads the api_key and api_url
    /// from the shared state. When `auto_discover` is false, this is a no-op.
    fn restart_fetch_models_task(
        &mut self,
        state: Entity<State>,
        http_client: Arc<dyn HttpClient>,
        provider_name: LanguageModelProviderName,
        cx: &mut Context<Self>,
    ) {
        let state_read = state.read(cx);
        let auto_discover = state_read.settings.auto_discover;
        let api_url = state_read.settings.api_url.clone();
        let extra_headers = state_read.settings.custom_headers.clone();
        let api_key = state_read
            .api_key_state
            .key(&api_url)
            .map(|k| k.to_string());
        // Release the borrow before spawning the task.
        let _ = state_read;

        if !auto_discover {
            return;
        }

        let Some(api_key) = api_key else {
            log::warn!(
                "OpenAI-compatible provider {provider_name}: auto_discover is enabled but no API key is set — set the {provider_name} API key to discover models"
            );
            return;
        };

        let state_for_notify = state.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = list_models(http_client.as_ref(), &api_url, &api_key, &extra_headers)
                .await
                .map(|models| {
                    models
                        .into_iter()
                        .map(|m| {
                            let supports_tools = m.supports_tools();
                            let supports_images = m.supports_images();
                            let display_name = {
                                let name = m.name.clone().unwrap_or_default();
                                if name.is_empty() { None } else { Some(name) }
                            };
                            AvailableModel {
                                name: m.id,
                                display_name,
                                max_tokens: m.context_length.unwrap_or(128_000),
                                max_output_tokens: m.max_output_tokens,
                                max_completion_tokens: None,
                                reasoning_effort: None,
                                capabilities: ModelCapabilities {
                                    tools: supports_tools,
                                    images: supports_images,
                                    parallel_tool_calls: supports_tools,
                                    prompt_cache_key: false,
                                    chat_completions: true,
                                    interleaved_reasoning: false,
                                    max_tokens_parameter: false,
                                },
                            }
                        })
                        .collect::<Vec<_>>()
                });
            match result {
                Ok(models) => {
                    this.update(cx, |this, cx| {
                        this.fetched_models = models;
                        cx.notify();
                    })
                    .ok();
                    // Notify the shared state so the LanguageModelRegistry
                    // re-reads `provided_models` and the picker updates.
                    state_for_notify.update(cx, |_, cx| cx.notify());
                }
                Err(e) => {
                    // zed-kask (D48): `{e:#}` (anyhow's alternate format)
                    // includes the full source chain — connect vs DNS vs TLS
                    // vs timeout. The top-level Display alone logged "error
                    // sending request" with no cause, leaving transport
                    // failures undiagnosable (observed 2026-09-04: DeepInfra
                    // discovery failure whose root cause could not be
                    // recovered from the log).
                    log::warn!(
                        "OpenAI-compatible provider {provider_name}: model discovery from {api_url} failed: {e:#}"
                    );
                }
            }
            Ok(())
        });
        self.fetch_models_task.replace(task);
    }
}

pub struct OpenAiCompatibleLanguageModelProvider {
    id: LanguageModelProviderId,
    name: LanguageModelProviderName,
    http_client: Arc<dyn HttpClient>,
    state: Entity<State>,
    discovery_state: Entity<DiscoveryState>,
}

impl OpenAiCompatibleLanguageModelProvider {
    pub fn new(
        id: Arc<str>,
        http_client: Arc<dyn HttpClient>,
        credentials_provider: Arc<dyn CredentialsProvider>,
        cx: &mut App,
    ) -> Self {
        let state = State::new(
            id.clone(),
            credentials_provider,
            |id, cx| {
                crate::AllLanguageModelSettings::get_global(cx)
                    .openai_compatible
                    .get(id)
            },
            cx,
        );

        let provider_name = LanguageModelProviderName::from(id.clone());
        let http_client_for_discovery = http_client.clone();
        let state_for_discovery = state.clone();
        let discovery_state = cx.new(|cx| {
            // Re-trigger discovery when the shared state notifies (settings or
            // api-key change). The shared state calls cx.notify() in
            // `update_settings` and after `authenticate`/`set_api_key`.
            let observe_http = http_client_for_discovery.clone();
            let observe_name = provider_name.clone();
            cx.observe(
                &state,
                move |this: &mut DiscoveryState, observed_state, cx| {
                    this.restart_fetch_models_task(
                        observed_state,
                        observe_http.clone(),
                        observe_name.clone(),
                        cx,
                    );
                },
            )
            .detach();
            let mut discovery = DiscoveryState::new();
            discovery.restart_fetch_models_task(
                state_for_discovery.clone(),
                http_client_for_discovery.clone(),
                provider_name.clone(),
                cx,
            );
            discovery
        });

        Self {
            id: id.clone().into(),
            name: id.into(),
            http_client,
            state,
            discovery_state,
        }
    }

    fn create_language_model(&self, model: AvailableModel) -> Arc<dyn LanguageModel> {
        Arc::new(OpenAiCompatibleLanguageModel {
            id: LanguageModelId::from(model.name.clone()),
            provider_id: self.id.clone(),
            provider_name: self.name.clone(),
            model,
            state: self.state.clone(),
            http_client: self.http_client.clone(),
            request_limiter: RateLimiter::new(4),
        })
    }
}

impl LanguageModelProviderState for OpenAiCompatibleLanguageModelProvider {
    type ObservableEntity = State;

    fn observable_entity(&self) -> Option<Entity<Self::ObservableEntity>> {
        Some(self.state.clone())
    }
}

impl LanguageModelProvider for OpenAiCompatibleLanguageModelProvider {
    fn id(&self) -> LanguageModelProviderId {
        self.id.clone()
    }

    fn name(&self) -> LanguageModelProviderName {
        self.name.clone()
    }

    fn icon(&self) -> IconOrSvg {
        IconOrSvg::Icon(IconName::AiOpenAiCompat)
    }

    fn default_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        let settings_models = &self.state.read(cx).settings.available_models;
        if let Some(model) = settings_models.first() {
            return Some(self.create_language_model(model.clone()));
        }
        // Fall back to the first discovered model when no static models are configured.
        self.discovery_state
            .read(cx)
            .fetched_models
            .first()
            .map(|model| self.create_language_model(model.clone()))
    }

    fn default_fast_model(&self, _cx: &App) -> Option<Arc<dyn LanguageModel>> {
        None
    }

    fn provided_models(&self, cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        let settings_models = &self.state.read(cx).settings.available_models;
        let discovered_models = &self.discovery_state.read(cx).fetched_models;

        // Merge: settings models first (user-configured takes precedence),
        // then discovered models not already present by name.
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut models: Vec<AvailableModel> = Vec::new();
        for model in settings_models {
            seen.insert(model.name.clone());
            models.push(model.clone());
        }
        for model in discovered_models {
            if seen.insert(model.name.clone()) {
                models.push(model.clone());
            }
        }
        models
            .into_iter()
            .map(|model| self.create_language_model(model))
            .collect()
    }

    fn is_authenticated(&self, cx: &App) -> bool {
        self.state.read(cx).is_authenticated()
    }

    fn authenticate(&self, cx: &mut App) -> Task<Result<(), AuthenticateError>> {
        self.state.update(cx, |state, cx| state.authenticate(cx))
    }

    fn settings_view(&self, _cx: &mut App) -> Option<ProviderSettingsView> {
        let state = self.state.clone();
        Some(ProviderSettingsView::SubPage(SubPageProviderSettings::new(
            move |window, cx| {
                cx.new(|cx| {
                    ApiCompatibleProviderConfigurationView::new(
                        state.clone(),
                        "OpenAI",
                        API_KEY_PLACEHOLDER,
                        window,
                        cx,
                    )
                })
                .into()
            },
        )))
    }

    fn set_api_key(&self, api_key: Option<String>, cx: &mut App) -> Task<Result<()>> {
        self.state
            .update(cx, |state, cx| state.set_api_key(api_key, cx))
    }
}

pub struct OpenAiCompatibleLanguageModel {
    id: LanguageModelId,
    provider_id: LanguageModelProviderId,
    provider_name: LanguageModelProviderName,
    model: AvailableModel,
    state: Entity<State>,
    http_client: Arc<dyn HttpClient>,
    request_limiter: RateLimiter,
}

impl OpenAiCompatibleLanguageModel {
    fn stream_completion(
        &self,
        request: open_ai::Request,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<
            futures::stream::BoxStream<'static, Result<ResponseStreamEvent>>,
            LanguageModelCompletionError,
        >,
    > {
        let http_client = self.http_client.clone();

        let (api_key, api_url, extra_headers) = self.state.read_with(cx, |state, _cx| {
            let api_url = &state.settings.api_url;
            (
                state.api_key_state.key(api_url),
                state.settings.api_url.clone(),
                state.settings.custom_headers.clone(),
            )
        });

        let provider = self.provider_name.clone();
        let future = self.request_limiter.stream(async move {
            let Some(api_key) = api_key else {
                return Err(LanguageModelCompletionError::NoApiKey { provider });
            };
            let request = stream_completion(
                http_client.as_ref(),
                provider.0.as_str(),
                &api_url,
                &api_key,
                request,
                &extra_headers,
            );
            let response = request.await?;
            Ok(response)
        });

        async move { Ok(future.await?.boxed()) }.boxed()
    }

    fn stream_response(
        &self,
        request: ResponseRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<'static, Result<futures::stream::BoxStream<'static, Result<ResponsesStreamEvent>>>>
    {
        let http_client = self.http_client.clone();

        let (api_key, api_url, extra_headers) = self.state.read_with(cx, |state, _cx| {
            let api_url = &state.settings.api_url;
            (
                state.api_key_state.key(api_url),
                state.settings.api_url.clone(),
                state.settings.custom_headers.clone(),
            )
        });

        let provider = self.provider_name.clone();
        let future = self.request_limiter.stream(async move {
            let Some(api_key) = api_key else {
                return Err(LanguageModelCompletionError::NoApiKey { provider });
            };
            let request = stream_response(
                http_client.as_ref(),
                provider.0.as_str(),
                &api_url,
                &api_key,
                request,
                &extra_headers,
            );
            let response = request.await?;
            Ok(response)
        });

        async move { Ok(future.await?.boxed()) }.boxed()
    }
}

fn default_thinking_reasoning_effort(model: &AvailableModel) -> Option<open_ai::ReasoningEffort> {
    model
        .reasoning_effort
        .filter(|effort| *effort != open_ai::ReasoningEffort::None)
}

fn supported_reasoning_effort_levels(model: &AvailableModel) -> Vec<LanguageModelEffortLevel> {
    let Some(default_effort) = default_thinking_reasoning_effort(model) else {
        return Vec::new();
    };

    open_ai::ReasoningEffort::OPENAI_COMPATIBLE_SELECTABLE
        .into_iter()
        .map(|effort| LanguageModelEffortLevel {
            name: effort.label().into(),
            value: effort.value().into(),
            is_default: effort == default_effort,
        })
        .collect()
}

fn selected_thinking_reasoning_effort(
    request: &LanguageModelRequest,
) -> Option<open_ai::ReasoningEffort> {
    request
        .reasoning_effort
        .as_deref()
        .and_then(|effort| effort.parse::<open_ai::ReasoningEffort>().ok())
        .filter(|effort| *effort != open_ai::ReasoningEffort::None)
}

fn chat_completion_max_tokens_parameter(
    model: &AvailableModel,
) -> crate::provider::open_ai::ChatCompletionMaxTokensParameter {
    if model.capabilities.max_tokens_parameter {
        crate::provider::open_ai::ChatCompletionMaxTokensParameter::MaxTokens
    } else {
        crate::provider::open_ai::ChatCompletionMaxTokensParameter::MaxCompletionTokens
    }
}

fn chat_completion_reasoning_effort(
    request: &LanguageModelRequest,
    model: &AvailableModel,
) -> Option<open_ai::ReasoningEffort> {
    if model.reasoning_effort == Some(open_ai::ReasoningEffort::None) {
        return Some(open_ai::ReasoningEffort::None);
    }

    if request.thinking_allowed {
        selected_thinking_reasoning_effort(request)
            .or_else(|| default_thinking_reasoning_effort(model))
    } else {
        Some(open_ai::ReasoningEffort::None)
    }
}

fn disable_response_thinking_for_none_effort(
    request: &mut LanguageModelRequest,
    model: &AvailableModel,
) {
    if model.reasoning_effort == Some(open_ai::ReasoningEffort::None) {
        request.thinking_allowed = false;
        request.reasoning_effort = None;
    }
}

impl LanguageModel for OpenAiCompatibleLanguageModel {
    fn id(&self) -> LanguageModelId {
        self.id.clone()
    }

    fn name(&self) -> LanguageModelName {
        LanguageModelName::from(
            self.model
                .display_name
                .clone()
                .unwrap_or_else(|| self.model.name.clone()),
        )
    }

    fn provider_id(&self) -> LanguageModelProviderId {
        self.provider_id.clone()
    }

    fn provider_name(&self) -> LanguageModelProviderName {
        self.provider_name.clone()
    }

    fn supports_tools(&self) -> bool {
        self.model.capabilities.tools
    }

    fn tool_input_format(&self) -> LanguageModelToolSchemaFormat {
        LanguageModelToolSchemaFormat::JsonSchemaSubset
    }

    fn supports_images(&self) -> bool {
        self.model.capabilities.images
    }

    fn supports_tool_choice(&self, choice: LanguageModelToolChoice) -> bool {
        match choice {
            LanguageModelToolChoice::Auto => self.model.capabilities.tools,
            LanguageModelToolChoice::Any => self.model.capabilities.tools,
            LanguageModelToolChoice::None => true,
        }
    }

    fn supports_streaming_tools(&self) -> bool {
        true
    }

    fn supports_thinking(&self) -> bool {
        default_thinking_reasoning_effort(&self.model).is_some()
    }

    fn supported_effort_levels(&self) -> Vec<LanguageModelEffortLevel> {
        supported_reasoning_effort_levels(&self.model)
    }

    fn supports_split_token_display(&self) -> bool {
        true
    }

    fn telemetry_id(&self) -> String {
        format!("openai/{}", self.model.name)
    }

    fn api_key(&self, cx: &App) -> Option<String> {
        self.state.read_with(cx, |state, _cx| {
            let api_url = &state.settings.api_url;
            state.api_key_state.key(api_url).map(|key| key.to_string())
        })
    }

    fn api_url(&self, cx: &App) -> Option<String> {
        self.state
            .read_with(cx, |state, _cx| Some(state.settings.api_url.clone()))
    }

    fn max_token_count(&self) -> u64 {
        self.model.max_tokens
    }

    fn max_output_tokens(&self) -> Option<u64> {
        self.model.max_output_tokens
    }

    fn stream_completion(
        &self,
        mut request: LanguageModelRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<
            futures::stream::BoxStream<
                'static,
                Result<LanguageModelCompletionEvent, LanguageModelCompletionError>,
            >,
            LanguageModelCompletionError,
        >,
    > {
        // `speed` can leak in from a parent thread's model; this provider never
        // supports fast mode, and arbitrary compatible endpoints reject `service_tier`.
        if !self.supports_fast_mode() {
            request.speed = None;
        }

        if self.model.capabilities.chat_completions {
            let reasoning_effort = chat_completion_reasoning_effort(&request, &self.model);
            let request = match into_open_ai(
                request,
                &self.model.name,
                self.model.capabilities.parallel_tool_calls,
                self.model.capabilities.prompt_cache_key,
                self.max_output_tokens(),
                chat_completion_max_tokens_parameter(&self.model),
                reasoning_effort,
                self.model.capabilities.interleaved_reasoning,
            ) {
                Ok(request) => request,
                Err(error) => return async move { Err(error.into()) }.boxed(),
            };
            let completions = self.stream_completion(request, cx);
            async move {
                let mapper = OpenAiEventMapper::new();
                Ok(mapper.map_stream(completions.await?).boxed())
            }
            .boxed()
        } else {
            disable_response_thinking_for_none_effort(&mut request, &self.model);
            let request = match into_open_ai_response(
                request,
                &self.model.name,
                self.model.capabilities.parallel_tool_calls,
                self.model.capabilities.prompt_cache_key,
                self.max_output_tokens(),
                default_thinking_reasoning_effort(&self.model),
                &self.provider_id,
            ) {
                Ok(request) => request,
                Err(error) => return async move { Err(error.into()) }.boxed(),
            };
            let completions = self.stream_response(request, cx);
            let compaction_state_owner = self.provider_id.clone();
            async move {
                let mapper = OpenAiResponseEventMapper::new(compaction_state_owner);
                Ok(mapper.map_stream(completions.await?).boxed())
            }
            .boxed()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    fn available_model(reasoning_effort: Option<open_ai::ReasoningEffort>) -> AvailableModel {
        AvailableModel {
            name: "custom-model".to_string(),
            display_name: None,
            max_tokens: 128_000,
            max_output_tokens: None,
            max_completion_tokens: None,
            reasoning_effort,
            capabilities: ModelCapabilities {
                chat_completions: false,
                ..Default::default()
            },
        }
    }

    #[test]
    fn configured_reasoning_effort_supports_thinking() {
        assert_eq!(
            default_thinking_reasoning_effort(&available_model(Some(
                open_ai::ReasoningEffort::High
            ))),
            Some(open_ai::ReasoningEffort::High)
        );
    }

    #[test]
    fn missing_or_none_reasoning_effort_does_not_support_thinking() {
        assert_eq!(
            default_thinking_reasoning_effort(&available_model(None)),
            None
        );
        assert_eq!(
            default_thinking_reasoning_effort(&available_model(Some(
                open_ai::ReasoningEffort::None
            ))),
            None
        );
    }

    #[test]
    fn supported_reasoning_effort_levels_use_configured_effort_as_default() {
        let effort_levels = supported_reasoning_effort_levels(&available_model(Some(
            open_ai::ReasoningEffort::High,
        )));
        let values = effort_levels
            .iter()
            .map(|level| level.value.as_ref())
            .collect::<Vec<_>>();

        assert_eq!(values, ["minimal", "low", "medium", "high", "xhigh", "max"]);
        assert_eq!(
            effort_levels
                .iter()
                .find(|level| level.is_default)
                .map(|level| level.value.as_ref()),
            Some("high")
        );
    }

    #[test]
    fn supported_reasoning_effort_levels_hide_missing_or_none_effort() {
        assert!(supported_reasoning_effort_levels(&available_model(None)).is_empty());
        assert!(
            supported_reasoning_effort_levels(&available_model(Some(
                open_ai::ReasoningEffort::None
            )))
            .is_empty()
        );
    }

    #[test]
    fn chat_completion_reasoning_effort_honors_request_and_configured_effort() {
        let model = available_model(Some(open_ai::ReasoningEffort::Medium));
        let mut request = LanguageModelRequest {
            thinking_allowed: true,
            ..Default::default()
        };

        assert_eq!(
            chat_completion_reasoning_effort(&request, &model),
            Some(open_ai::ReasoningEffort::Medium)
        );

        request.reasoning_effort = Some("high".to_string());
        assert_eq!(
            chat_completion_reasoning_effort(&request, &model),
            Some(open_ai::ReasoningEffort::High)
        );

        request.reasoning_effort = Some("not-supported".to_string());
        assert_eq!(
            chat_completion_reasoning_effort(&request, &model),
            Some(open_ai::ReasoningEffort::Medium)
        );

        request.thinking_allowed = false;
        assert_eq!(
            chat_completion_reasoning_effort(&request, &model),
            Some(open_ai::ReasoningEffort::None)
        );
    }

    #[test]
    fn chat_completion_reasoning_effort_sends_none_when_thinking_disallowed() {
        // Deliberate fork behavior (2026-08-23): when thinking is disallowed,
        // always send `ReasoningEffort::None` rather than omitting the field.
        let model = available_model(None);
        let request = LanguageModelRequest {
            thinking_allowed: false,
            ..Default::default()
        };

        assert_eq!(
            chat_completion_reasoning_effort(&request, &model),
            Some(open_ai::ReasoningEffort::None)
        );
    }

    #[test]
    fn chat_completion_reasoning_effort_preserves_explicit_none() {
        let model = available_model(Some(open_ai::ReasoningEffort::None));
        let request = LanguageModelRequest {
            thinking_allowed: true,
            reasoning_effort: Some("high".to_string()),
            ..Default::default()
        };

        assert_eq!(
            chat_completion_reasoning_effort(&request, &model),
            Some(open_ai::ReasoningEffort::None)
        );
    }

    #[test]
    fn chat_completion_max_tokens_parameter_defaults_to_max_completion_tokens() {
        let model = available_model(Some(open_ai::ReasoningEffort::Medium));

        assert_eq!(
            chat_completion_max_tokens_parameter(&model),
            crate::provider::open_ai::ChatCompletionMaxTokensParameter::MaxCompletionTokens
        );
    }

    #[test]
    fn chat_completion_max_tokens_parameter_uses_max_tokens_when_configured() {
        let mut model = available_model(Some(open_ai::ReasoningEffort::Medium));
        model.capabilities.max_tokens_parameter = true;

        assert_eq!(
            chat_completion_max_tokens_parameter(&model),
            crate::provider::open_ai::ChatCompletionMaxTokensParameter::MaxTokens
        );
    }

    #[test]
    fn response_request_includes_reasoning_when_effort_is_configured() {
        let model = available_model(Some(open_ai::ReasoningEffort::High));
        let request = LanguageModelRequest {
            thinking_allowed: true,
            ..Default::default()
        };

        let request = into_open_ai_response(
            request,
            &model.name,
            model.capabilities.parallel_tool_calls,
            model.capabilities.prompt_cache_key,
            model.max_output_tokens,
            default_thinking_reasoning_effort(&model),
            &LanguageModelProviderId::new("test-compatible-provider"),
        )
        .unwrap();
        let serialized = serde_json::to_value(request).unwrap();

        assert_eq!(
            serialized["reasoning"],
            json!({ "effort": "high", "summary": "auto" })
        );
        assert_eq!(
            serialized["include"],
            json!(["reasoning.encrypted_content"])
        );
    }

    #[test]
    fn response_request_omits_reasoning_when_effort_is_missing() {
        let model = available_model(None);
        let request = LanguageModelRequest {
            thinking_allowed: true,
            ..Default::default()
        };

        let request = into_open_ai_response(
            request,
            &model.name,
            model.capabilities.parallel_tool_calls,
            model.capabilities.prompt_cache_key,
            model.max_output_tokens,
            default_thinking_reasoning_effort(&model),
            &LanguageModelProviderId::new("test-compatible-provider"),
        )
        .unwrap();
        let serialized = serde_json::to_value(request).unwrap();

        assert_eq!(serialized.get("reasoning"), None);
        assert_eq!(serialized.get("include"), None);
    }

    #[test]
    fn chat_completion_request_includes_selected_reasoning_effort() {
        let mut model = available_model(Some(open_ai::ReasoningEffort::Medium));
        model.capabilities.chat_completions = true;
        let request = LanguageModelRequest {
            thinking_allowed: true,
            reasoning_effort: Some("high".to_string()),
            ..Default::default()
        };
        let reasoning_effort = chat_completion_reasoning_effort(&request, &model);

        let request = into_open_ai(
            request,
            &model.name,
            model.capabilities.parallel_tool_calls,
            model.capabilities.prompt_cache_key,
            model.max_output_tokens,
            chat_completion_max_tokens_parameter(&model),
            reasoning_effort,
            model.capabilities.interleaved_reasoning,
        )
        .unwrap();
        let serialized = serde_json::to_value(request).unwrap();

        assert_eq!(serialized["reasoning_effort"], json!("high"));
    }

    #[test]
    fn response_reasoning_effort_preserves_explicit_none() {
        let model = available_model(Some(open_ai::ReasoningEffort::None));
        let mut request = LanguageModelRequest {
            thinking_allowed: true,
            reasoning_effort: Some("high".to_string()),
            ..Default::default()
        };

        disable_response_thinking_for_none_effort(&mut request, &model);
        assert!(!request.thinking_allowed);
        assert_eq!(request.reasoning_effort, None);
    }

    /// D48 pin: the discovery-failure warn must format the error with
    /// anyhow's alternate Display (`{e:#}`) so the source chain (connect
    /// vs DNS vs TLS vs timeout) is logged. The plain `{e}` left transport
    /// failures undiagnosable — "error sending request" with no cause
    /// (observed 2026-09-04: the DeepInfra discovery failure).
    #[test]
    fn discovery_failure_warn_includes_error_source_chain() {
        let source = include_str!("open_ai_compatible.rs");
        let needle = "model discovery from {api_url} failed: {e:#}";
        assert!(
            source.contains(needle),
            "the discovery warn must use {{e:#}} (anyhow source chain), not plain {{e}}"
        );
    }
}
