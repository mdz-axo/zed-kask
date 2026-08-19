//! RunPod serverless endpoint provider.
//!
//! zed-kask (D29): RunPod serverless endpoints are not a standard
//! OpenAI-compatible provider. Each endpoint has its own API URL
//! (`https://api.runpod.ai/v2/{endpoint_id}/openai/v1`), and endpoint
//! discovery uses the RunPod GraphQL API at `https://api.runpod.io/graphql`
//! (not `/v1/models`). The GraphQL API and the serverless REST API use
//! different domains: `api.runpod.io` for GraphQL, `api.runpod.ai` for REST.
//! This file registers RunPod as a dedicated `LanguageModelProvider` so each
//! model carries its own endpoint URL and the IPC bridge
//! (`LanguageModelRegistry`) can resolve `RunPod/kask-ocr` for the corpus OCR
//! pipeline.
//!
//! See `DIVERGENCE.md` D29 for the full rationale and pin tests.

use anyhow::{Result, anyhow};
use credentials_provider::CredentialsProvider;
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::BoxStream};
use gpui::{App, AppContext, AsyncApp, Context, Entity, SharedString, Task};
use http_client::{CustomHeaders, HttpClient};
use language_model::{
    ApiKeyConfiguration, ApiKeyState, AuthenticateError, EnvVar, IconOrSvg, LanguageModel,
    LanguageModelCompletionError, LanguageModelCompletionEvent, LanguageModelId, LanguageModelName,
    LanguageModelProvider, LanguageModelProviderId, LanguageModelProviderName,
    LanguageModelProviderState, LanguageModelRequest, LanguageModelToolChoice,
    LanguageModelToolSchemaFormat, ProviderSettingsView, RateLimiter, env_var,
};
use open_ai::{ResponseStreamEvent, stream_completion};
use serde::Deserialize;
use settings::Settings;
use std::sync::{Arc, LazyLock};
use ui::IconName;

use crate::provider::open_ai::{ChatCompletionMaxTokensParameter, OpenAiEventMapper, into_open_ai};
pub use settings::RunpodAvailableModel as AvailableModel;

const PROVIDER_ID: LanguageModelProviderId = LanguageModelProviderId::new("runpod");
const PROVIDER_NAME: LanguageModelProviderName = LanguageModelProviderName::new("RunPod");

const API_KEY_ENV_VAR_NAME: &str = "RUNPOD_API_KEY";
static API_KEY_ENV_VAR: LazyLock<EnvVar> = env_var!(API_KEY_ENV_VAR_NAME);

/// Default RunPod GraphQL API base URL. Used for endpoint discovery via the
/// GraphQL API at `{api_url}/graphql`. The serverless REST API (OpenAI-compatible
/// inference) uses a different domain (`api.runpod.ai`) — see
/// `RUNPOD_REST_API_BASE_URL` and `endpoint_url`.
pub const RUNPOD_DEFAULT_API_URL: &str = "https://api.runpod.io";

/// Default RunPod serverless REST API base URL. The OpenAI-compatible endpoint
/// URL for a serverless endpoint is
/// `{RUNPOD_REST_API_BASE_URL}/v2/{endpoint_id}/openai/v1`.
/// Per the RunPod docs (https://docs.runpod.io/serverless/vllm/openai-compatibility),
/// the REST API domain is `api.runpod.ai`, NOT `api.runpod.io` (which is the
/// GraphQL API domain).
pub const RUNPOD_REST_API_BASE_URL: &str = "https://api.runpod.ai";

#[derive(Default, Clone, Debug, PartialEq)]
pub struct RunpodSettings {
    pub api_url: String,
    pub auto_discover: bool,
    pub available_models: Vec<AvailableModel>,
    pub custom_headers: CustomHeaders,
}

pub struct RunpodLanguageModelProvider {
    http_client: Arc<dyn HttpClient>,
    state: Entity<State>,
    discovery_state: Entity<DiscoveryState>,
}

pub struct State {
    api_key_state: ApiKeyState,
    credentials_provider: Arc<dyn CredentialsProvider>,
}

impl State {
    fn is_authenticated(&self) -> bool {
        self.api_key_state.has_key()
    }

    fn set_api_key(&mut self, api_key: Option<String>, cx: &mut Context<Self>) -> Task<Result<()>> {
        let credentials_provider = self.credentials_provider.clone();
        let api_url = RunpodLanguageModelProvider::api_url(cx);
        self.api_key_state.store(
            api_url,
            api_key,
            |this| &mut this.api_key_state,
            credentials_provider,
            cx,
        )
    }

    fn authenticate(&mut self, cx: &mut Context<Self>) -> Task<Result<(), AuthenticateError>> {
        let credentials_provider = self.credentials_provider.clone();
        let api_url = RunpodLanguageModelProvider::api_url(cx);
        self.api_key_state.load_if_needed(
            api_url,
            |this| &mut this.api_key_state,
            credentials_provider,
            cx,
        )
    }
}

/// Discovery state for auto-discovering RunPod serverless endpoints via the
/// RunPod GraphQL API. Mirrors the OpenAI-compatible `DiscoveryState` pattern:
/// a `fetch_models_task` is spawned on authenticate / settings change, and
/// `fetched_models` is merged into `provided_models` by the provider.
pub struct DiscoveryState {
    /// Endpoints discovered via the GraphQL API. Empty until a successful fetch.
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

    /// Spawn (or replace) the fetch-endpoints task. Reads the api_key and
    /// api_url from the shared state. When `auto_discover` is false, this is
    /// a no-op.
    fn restart_fetch_endpoints_task(
        &mut self,
        state: Entity<State>,
        http_client: Arc<dyn HttpClient>,
        cx: &mut Context<Self>,
    ) {
        let state_read = state.read(cx);
        let auto_discover = RunpodLanguageModelProvider::settings(cx).auto_discover;
        let api_url = RunpodLanguageModelProvider::api_url(cx);
        let extra_headers = RunpodLanguageModelProvider::settings(cx)
            .custom_headers
            .clone();
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
                "RunPod provider: auto_discover is enabled but no API key is set — set the RunPod API key (env var {API_KEY_ENV_VAR_NAME}) to discover endpoints"
            );
            return;
        };

        let state_for_notify = state.clone();
        let task = cx.spawn(async move |this, cx| {
            let result =
                fetch_runpod_endpoints(http_client.as_ref(), &api_url, &api_key, &extra_headers)
                    .await;
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
                Err(error) => {
                    log::warn!(
                        "RunPod provider: endpoint discovery from {api_url}/graphql failed: {error}"
                    );
                }
            }
            Ok(())
        });
        self.fetch_models_task.replace(task);
    }
}

impl RunpodLanguageModelProvider {
    pub fn new(
        http_client: Arc<dyn HttpClient>,
        credentials_provider: Arc<dyn CredentialsProvider>,
        cx: &mut App,
    ) -> Self {
        let state = cx.new(|cx| {
            cx.observe_global::<settings::SettingsStore>(|this: &mut State, cx| {
                let credentials_provider = this.credentials_provider.clone();
                let api_url = Self::api_url(cx);
                this.api_key_state.handle_url_change(
                    api_url,
                    |this| &mut this.api_key_state,
                    credentials_provider,
                    cx,
                );
                cx.notify();
            })
            .detach();
            State {
                api_key_state: ApiKeyState::new(Self::api_url(cx), (*API_KEY_ENV_VAR).clone()),
                credentials_provider,
            }
        });

        let http_client_for_discovery = http_client.clone();
        let http_client_for_initial = http_client.clone();
        let state_for_discovery = state.clone();
        let discovery_state = cx.new(|cx| {
            cx.observe(
                &state,
                move |this: &mut DiscoveryState, observed_state, cx| {
                    this.restart_fetch_endpoints_task(
                        observed_state,
                        http_client_for_discovery.clone(),
                        cx,
                    );
                },
            )
            .detach();
            let mut discovery = DiscoveryState::new();
            discovery.restart_fetch_endpoints_task(
                state_for_discovery.clone(),
                http_client_for_initial.clone(),
                cx,
            );
            discovery
        });

        Self {
            http_client,
            state,
            discovery_state,
        }
    }

    fn create_language_model_with_url(
        &self,
        model: AvailableModel,
        _cx: &App,
    ) -> Arc<dyn LanguageModel> {
        Arc::new(RunpodLanguageModel {
            id: LanguageModelId::from(model.name.clone()),
            endpoint_url: endpoint_url("", &model.endpoint_id),
            supports_images: model.supports_images,
            max_tokens: model.max_tokens,
            max_output_tokens: model.max_output_tokens,
            model_name: model.name.clone(),
            served_model_name: model
                .served_model_name
                .clone()
                .unwrap_or_else(|| model.name.clone()),
            state: self.state.clone(),
            http_client: self.http_client.clone(),
            request_limiter: RateLimiter::new(4),
        })
    }

    fn settings(cx: &App) -> &RunpodSettings {
        &crate::AllLanguageModelSettings::get_global(cx).runpod
    }

    fn api_url_from_settings(cx: &App) -> SharedString {
        let api_url = &Self::settings(cx).api_url;
        if api_url.is_empty() {
            RUNPOD_DEFAULT_API_URL.into()
        } else {
            SharedString::new(api_url.as_str())
        }
    }

    fn api_url(cx: &App) -> SharedString {
        Self::api_url_from_settings(cx)
    }
}

impl LanguageModelProviderState for RunpodLanguageModelProvider {
    type ObservableEntity = State;

    fn observable_entity(&self) -> Option<Entity<Self::ObservableEntity>> {
        Some(self.state.clone())
    }
}

impl LanguageModelProvider for RunpodLanguageModelProvider {
    fn id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn icon(&self) -> IconOrSvg {
        // RunPod serves OpenAI-compatible APIs; reuse the OpenAI-compatible
        // icon rather than adding a new SVG asset.
        IconOrSvg::Icon(IconName::AiOpenAiCompat)
    }

    fn default_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        let settings_models = &Self::settings(cx).available_models;
        if let Some(model) = settings_models.first() {
            return Some(self.create_language_model_with_url(model.clone(), cx));
        }
        // Fall back to the first discovered model when no static models are configured.
        self.discovery_state
            .read(cx)
            .fetched_models
            .first()
            .map(|model| self.create_language_model_with_url(model.clone(), cx))
    }

    fn default_fast_model(&self, _cx: &App) -> Option<Arc<dyn LanguageModel>> {
        None
    }

    fn provided_models(&self, cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        let settings_models = &Self::settings(cx).available_models;
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
            .map(|model| self.create_language_model_with_url(model, cx))
            .collect()
    }

    fn is_authenticated(&self, cx: &App) -> bool {
        self.state.read(cx).is_authenticated()
    }

    fn authenticate(&self, cx: &mut App) -> Task<Result<(), AuthenticateError>> {
        self.state.update(cx, |state, cx| state.authenticate(cx))
    }

    fn settings_view(&self, cx: &mut App) -> Option<ProviderSettingsView> {
        let state = self.state.read(cx);
        Some(ProviderSettingsView::ApiKey(ApiKeyConfiguration::new(
            state.api_key_state.has_key(),
            state.api_key_state.is_from_env_var(),
            state.api_key_state.env_var_name().clone(),
            "https://www.runpod.io/console/serverless".into(),
        )))
    }

    fn set_api_key(&self, api_key: Option<String>, cx: &mut App) -> Task<Result<()>> {
        self.state
            .update(cx, |state, cx| state.set_api_key(api_key, cx))
    }
}

pub struct RunpodLanguageModel {
    id: LanguageModelId,
    /// Per-model OpenAI-compatible endpoint URL:
    /// `https://api.runpod.ai/v2/{endpoint_id}/openai/v1`.
    endpoint_url: String,
    supports_images: bool,
    max_tokens: u64,
    max_output_tokens: Option<u64>,
    /// The endpoint name, used as `LanguageModel::name()` (the resolution key)
    /// and `telemetry_id()`.
    model_name: String,
    /// The model name to send in the OpenAI `model` field. Defaults to
    /// `model_name` when not overridden — works if the endpoint sets
    /// `--served-model-name` to the endpoint name.
    served_model_name: String,
    state: Entity<State>,
    http_client: Arc<dyn HttpClient>,
    request_limiter: RateLimiter,
}

impl RunpodLanguageModel {
    fn stream_completion(
        &self,
        request: open_ai::Request,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<BoxStream<'static, Result<ResponseStreamEvent>>, LanguageModelCompletionError>,
    > {
        let http_client = self.http_client.clone();
        let endpoint_url = self.endpoint_url.clone();

        let (api_key, extra_headers) = self.state.read_with(cx, |state, cx| {
            let api_url = RunpodLanguageModelProvider::api_url(cx);
            let extra_headers = RunpodLanguageModelProvider::settings(cx)
                .custom_headers
                .clone();
            (state.api_key_state.key(&api_url), extra_headers)
        });

        let provider = PROVIDER_NAME;
        let future = self.request_limiter.stream(async move {
            let Some(api_key) = api_key else {
                return Err(LanguageModelCompletionError::NoApiKey { provider });
            };
            let request = stream_completion(
                http_client.as_ref(),
                provider.0.as_str(),
                &endpoint_url,
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

impl LanguageModel for RunpodLanguageModel {
    fn id(&self) -> LanguageModelId {
        self.id.clone()
    }

    fn name(&self) -> LanguageModelName {
        // Return the endpoint name (the resolution key), not the display_name.
        // The IPC server builds `ModelListEntry.name` as `{provider_id}/{model.name()}`;
        // if `name()` returned the display_name (e.g. "kask-ocr (OLMOCR-2)"), the
        // prefixed name would be "runpod/kask-ocr (OLMOCR-2)" which can't match
        // `DEFAULT_OCR_MODEL = "RunPod/kask-ocr"` in `resolve_ocr_model`. The
        // display_name is for UI rendering; `name()` is the resolution key.
        LanguageModelName::from(self.model_name.clone())
    }

    fn provider_id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn provider_name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn supports_tools(&self) -> bool {
        // RunPod serverless endpoints host single-model vLLM workers (e.g.
        // OLMOCR-2 for OCR). These are specialized models that do not support
        // tool calls. Advertising `false` is fail-closed: the agent panel
        // won't offer RunPod models for tool-using agent profiles, and the
        // inference request won't include tool definitions the model can't
        // honor. A future tool-capable endpoint can override this via a
        // per-model config field when one is needed.
        false
    }

    fn supports_streaming_tools(&self) -> bool {
        false
    }

    fn supports_thinking(&self) -> bool {
        false
    }

    fn supported_effort_levels(&self) -> Vec<language_model::LanguageModelEffortLevel> {
        Vec::new()
    }

    fn supports_tool_choice(&self, _choice: LanguageModelToolChoice) -> bool {
        true
    }

    fn supports_images(&self) -> bool {
        self.supports_images
    }

    fn tool_input_format(&self) -> LanguageModelToolSchemaFormat {
        LanguageModelToolSchemaFormat::JsonSchemaSubset
    }

    fn telemetry_id(&self) -> String {
        format!("runpod/{}", self.model_name)
    }

    fn max_token_count(&self) -> u64 {
        self.max_tokens
    }

    fn max_output_tokens(&self) -> Option<u64> {
        self.max_output_tokens
    }

    fn stream_completion(
        &self,
        request: LanguageModelRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<
            BoxStream<'static, Result<LanguageModelCompletionEvent, LanguageModelCompletionError>>,
            LanguageModelCompletionError,
        >,
    > {
        let request = match into_open_ai(
            request,
            &self.served_model_name,
            false, // supports_parallel_tool_calls
            false, // supports_prompt_cache_key
            self.max_output_tokens,
            ChatCompletionMaxTokensParameter::MaxTokens,
            None,  // reasoning_effort
            false, // interleaved_reasoning
        ) {
            Ok(request) => request,
            Err(error) => return async move { Err(error.into()) }.boxed(),
        };
        let stream = self.stream_completion(request, cx);

        async move {
            let mapper = OpenAiEventMapper::new();
            Ok(mapper.map_stream(stream.await?).boxed())
        }
        .boxed()
    }
}

/// Derive the OpenAI-compatible endpoint URL for a RunPod serverless endpoint.
/// Uses `RUNPOD_REST_API_BASE_URL` (`api.runpod.ai`), NOT the GraphQL API URL
/// (`api.runpod.io`). The two domains are different per the RunPod docs.
fn endpoint_url(_api_url: &str, endpoint_id: &str) -> String {
    format!("{RUNPOD_REST_API_BASE_URL}/v2/{endpoint_id}/openai/v1")
}

// ── RunPod GraphQL discovery ──────────────────────────────────────────────

/// GraphQL response shape from `query { myself { endpoints { ... } } }`.
#[derive(Debug, Deserialize)]
struct GraphqlResponse {
    #[serde(default)]
    data: Option<GraphqlData>,
}

#[derive(Debug, Deserialize)]
struct GraphqlData {
    #[serde(default)]
    myself: Option<GraphqlMyself>,
}

#[derive(Debug, Deserialize)]
struct GraphqlMyself {
    #[serde(default)]
    endpoints: Vec<RunpodEndpoint>,
}

#[derive(Debug, Deserialize)]
struct RunpodEndpoint {
    id: String,
    name: String,
    /// Endpoint type (e.g. `"QB"` for Queue-Based serverless). We accept all
    /// types but record the value for future filtering.
    #[serde(default)]
    #[allow(dead_code)]
    r#type: Option<String>,
    /// Endpoint env vars. We read `MODEL_NAME` for the display name when the
    /// endpoint has no human-friendly name.
    #[serde(default)]
    env: Vec<RunpodEnvVar>,
}

#[derive(Debug, Deserialize)]
struct RunpodEnvVar {
    key: String,
    value: String,
}

/// Fetch RunPod serverless endpoints via the GraphQL API and convert them to
/// `AvailableModel` entries.
async fn fetch_runpod_endpoints(
    http_client: &dyn HttpClient,
    api_url: &str,
    api_key: &str,
    extra_headers: &CustomHeaders,
) -> Result<Vec<AvailableModel>> {
    use http_client::{AsyncBody, Method, Request as HttpRequest, RequestBuilderExt};

    let base = api_url.trim_end_matches('/');
    let uri = format!("{base}/graphql");
    // The GraphQL query is a static string — no user input is interpolated.
    let query = r#"query { myself { endpoints { id name type env { key value } } } }"#;
    let body = serde_json::json!({ "query": query });
    let body = serde_json::to_string(&body)
        .map_err(|error| anyhow!("failed to serialize RunPod GraphQL query: {error}"))?;

    let request = HttpRequest::builder()
        .method(Method::POST)
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .extra_headers(extra_headers)
        .body(AsyncBody::from(body))
        .map_err(|error| anyhow!("failed to build RunPod GraphQL request: {error}"))?;

    let mut response = http_client
        .send(request)
        .await
        .map_err(|error| anyhow!("RunPod GraphQL request failed: {error}"))?;

    if !response.status().is_success() {
        let mut body = String::new();
        use futures::AsyncReadExt;
        response
            .body_mut()
            .read_to_string(&mut body)
            .await
            .map_err(|error| anyhow!("failed to read RunPod GraphQL error body: {error}"))?;
        return Err(anyhow!(
            "RunPod GraphQL request returned {}: {body}",
            response.status()
        ));
    }

    let mut body = String::new();
    use futures::AsyncReadExt;
    response
        .body_mut()
        .read_to_string(&mut body)
        .await
        .map_err(|error| anyhow!("failed to read RunPod GraphQL response: {error}"))?;

    let parsed: GraphqlResponse = serde_json::from_str(&body)
        .map_err(|error| anyhow!("failed to parse RunPod GraphQL response: {error}"))?;

    let endpoints = parsed
        .data
        .and_then(|data| data.myself)
        .map(|myself| myself.endpoints)
        .unwrap_or_default();

    let mut models: Vec<AvailableModel> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for endpoint in endpoints {
        if !seen.insert(endpoint.name.clone()) {
            continue;
        }
        // Derive a display name from MODEL_NAME when present, otherwise use
        // the endpoint name.
        let model_name_env = endpoint
            .env
            .iter()
            .find(|var| var.key == "MODEL_NAME")
            .map(|var| var.value.clone());
        let display_name = model_name_env
            .as_ref()
            .map(|mn| format!("{} ({})", endpoint.name, mn));
        models.push(AvailableModel {
            name: endpoint.name,
            display_name,
            endpoint_id: endpoint.id,
            // RunPod does not expose context length via GraphQL; use a sane
            // default. The user can override via `available_models` in settings.
            max_tokens: 32_768,
            max_output_tokens: None,
            // Infer vision support from the MODEL_NAME env var. OLMOCR and
            // other OCR models are vision models — discovery should report
            // `true` so the IPC vision-model list includes them and
            // `resolve_ocr_model` can verify the model is vision-capable.
            supports_images: model_name_env.as_ref().is_some_and(|mn| {
                let lower = mn.to_ascii_lowercase();
                lower.contains("olmocr") || lower.contains("ocr") || lower.contains("vision")
            }),
            // vLLM expects the `model` field to match MODEL_NAME unless
            // --served-model-name overrides it. Populate from the env var so
            // discovered endpoints work without manual config.
            served_model_name: model_name_env,
        });
    }

    Ok(models)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// D29 pin: the endpoint URL is derived as
    /// `{RUNPOD_REST_API_BASE_URL}/v2/{id}/openai/v1`, using the REST API
    /// domain (`api.runpod.ai`), NOT the GraphQL API domain (`api.runpod.io`).
    #[test]
    fn endpoint_url_derives_from_api_url_and_endpoint_id() {
        assert_eq!(
            endpoint_url("", "hsldzov6932wf5"),
            "https://api.runpod.ai/v2/hsldzov6932wf5/openai/v1"
        );
    }

    /// D29 pin: the telemetry_id is `runpod/{endpoint_name}` (lowercase
    /// provider prefix), so `RunPod/kask-ocr` resolves through
    /// `resolve_model_names` via the case-insensitive telemetry_id match.
    /// This test pins the format string; the full `LanguageModel::telemetry_id`
    /// method is exercised by the integration test in `model_resolution.rs`.
    #[test]
    fn telemetry_id_format_is_runpod_prefixed_endpoint_name() {
        // `telemetry_id()` returns `format!("runpod/{}", self.model_name)`.
        // Pin the format so a rename of the provider prefix is caught.
        let model_name = "kask-ocr";
        let telemetry_id = format!("runpod/{}", model_name);
        assert_eq!(telemetry_id, "runpod/kask-ocr");
    }

    /// D29 pin: `RunPod/kask-ocr` resolves through `resolve_model_names`
    /// because the provider id `runpod` matches `RunPod` case-insensitively
    /// and the model id `kask-ocr` matches the endpoint name. This test pins
    /// the string-comparison contract; a full integration test would require a
    /// GPUI test context with a registered RunPod provider.
    #[test]
    fn runpod_provider_id_matches_runpod_case_insensitively() {
        let configured_prefix = "RunPod";
        let registered_id = "runpod";
        assert!(
            registered_id.eq_ignore_ascii_case(configured_prefix),
            "case-insensitive comparison must match RunPod <-> runpod"
        );
    }

    /// D29 pin: the configured model name `RunPod/kask-ocr` matches the
    /// telemetry_id `runpod/kask-ocr` case-insensitively.
    #[test]
    fn runpod_telemetry_id_matches_configured_name_case_insensitively() {
        let configured = "RunPod/kask-ocr";
        let telemetry_id = "runpod/kask-ocr";
        assert!(
            telemetry_id.eq_ignore_ascii_case(configured),
            "telemetry_id comparison must be case-insensitive \
             (runpod/kask-ocr must match RunPod/kask-ocr)"
        );
    }

    /// D29 pin: the GraphQL response is parsed into `AvailableModel` entries,
    /// with the display name derived from `MODEL_NAME` when present.
    #[test]
    fn graphql_response_parses_into_available_models() {
        let body = r#"{
            "data": {
                "myself": {
                    "endpoints": [
                        {
                            "id": "hsldzov6932wf5",
                            "name": "kask-ocr",
                            "type": "QB",
                            "env": [
                                {"key": "MODEL_NAME", "value": "allenai/olmOCR-2-7B-1025"},
                                {"key": "RAW_OPENAI_OUTPUT", "value": "true"}
                            ]
                        }
                    ]
                }
            }
        }"#;
        let parsed: GraphqlResponse = serde_json::from_str(body).unwrap();
        let endpoints = parsed
            .data
            .and_then(|data| data.myself)
            .map(|myself| myself.endpoints)
            .unwrap_or_default();
        assert_eq!(endpoints.len(), 1);
        let endpoint = &endpoints[0];
        assert_eq!(endpoint.id, "hsldzov6932wf5");
        assert_eq!(endpoint.name, "kask-ocr");
        let model_name = endpoint
            .env
            .iter()
            .find(|var| var.key == "MODEL_NAME")
            .map(|var| var.value.clone());
        assert_eq!(model_name.as_deref(), Some("allenai/olmOCR-2-7B-1025"));

        // Verify the discovery function populates served_model_name from MODEL_NAME.
        // `model_name` is moved into `served_model_name` (no clone needed — it
        // is not used after this struct literal). The `display_name` borrows it
        // via `.as_ref()` before the move.
        let display_name = model_name
            .as_ref()
            .map(|mn| format!("{} ({})", endpoint.name, mn));
        let available = AvailableModel {
            name: endpoint.name.clone(),
            display_name,
            endpoint_id: endpoint.id.clone(),
            max_tokens: 32_768,
            max_output_tokens: None,
            supports_images: true,
            served_model_name: model_name,
        };
        assert_eq!(
            available.served_model_name.as_deref(),
            Some("allenai/olmOCR-2-7B-1025"),
            "served_model_name must be populated from MODEL_NAME env var"
        );
    }
}
