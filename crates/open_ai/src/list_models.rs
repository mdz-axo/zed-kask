//! Generic OpenAI-compatible model discovery via the standard `/v1/models` endpoint.
//!
//! Most OpenAI-compatible providers (DeepInfra, OpenRouter,
//! etc.) expose a `/v1/models` endpoint that returns a list of
//! available models in the standard OpenAI shape:
//!
//! ```json
//! { "data": [{ "id": "...", "object": "model", "created": ..., "owned_by": "..." }] }
//! ```
//!
//! Some providers extend this shape with optional fields like `context_length`
//! or capability hints. This module parses the standard shape plus the common
//! extensions, returning [`DiscoveredModel`] entries that convert to zed's
//! [`OpenAiCompatibleAvailableModel`](settings_content::OpenAiCompatibleAvailableModel).
//!
//! This is the reusable discovery primitive used by
//! `OpenAiCompatibleLanguageModelProvider` to auto-populate the model picker
//! for any OpenAI-compatible provider when the user has supplied an API key.
//! It is intentionally generic — provider-specific quirks (e.g. OpenRouter's
//! `/models/user` endpoint) live in their own crates, not here.

use futures::AsyncReadExt;
use http_client::{
    AsyncBody, CustomHeaders, HttpClient, Method, Request as HttpRequest, RequestBuilderExt,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors returned by [`list_models`].
#[derive(Debug, Error)]
pub enum ListModelsError {
    #[error("failed to build request: {0}")]
    BuildRequest(http_client::http::Error),
    #[error("failed to send request: {0}")]
    HttpSend(anyhow::Error),
    #[error("failed to read response body: {0}")]
    ReadResponse(anyhow::Error),
    #[error("failed to deserialize response: {0}")]
    DeserializeResponse(serde_json::Error),
    #[error("provider returned status {status}: {body}")]
    ApiError { status: u16, body: String },
}

/// A model entry discovered via `/v1/models`.
///
/// Mirrors the standard OpenAI model object plus the most common provider
/// extensions. Fields beyond the standard `id` are optional so this parses
/// cleanly across DeepInfra and OpenRouter
/// (which extends the shape with `context_length` and capability hints).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveredModel {
    /// The model ID (e.g. `"meta-llama/Llama-3.3-70B-Instruct"`).
    pub id: String,
    /// Optional display name. Falls back to `id` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional context window length in tokens. Falls back to a large default
    /// when the provider does not report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    /// Optional max output tokens. Some providers report this alongside
    /// `context_length`; others do not. OpenRouter nests it under
    /// `top_provider.max_completion_tokens` — `resolve_provider_fallbacks`
    /// folds that into this field when the top-level value is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    /// Optional list of supported parameters. Used to detect tool support
    /// (presence of `"tools"`) and reasoning support (presence of
    /// `"reasoning"`). OpenRouter populates this; many providers do not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_parameters: Vec<String>,
    /// Optional architecture metadata. The `input_modalities` field, when
    /// present, is used to detect image support. OpenRouter populates this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture: Option<ModelArchitecture>,
    /// Optional per-provider limits. OpenRouter nests `max_completion_tokens`
    /// and (a second copy of) `context_length` here rather than at the top
    /// level. `resolve_provider_fallbacks` promotes these into the top-level
    /// fields when the provider omits them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_provider: Option<TopProvider>,
}

/// Optional architecture metadata returned by some providers (notably OpenRouter).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelArchitecture {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_modalities: Vec<String>,
}

/// Per-provider limits nested under `top_provider` in OpenRouter's
/// `/v1/models` response. OpenRouter reports `max_completion_tokens` here
/// instead of as a top-level `max_output_tokens`, so without parsing this
/// every OpenRouter model would discover with `max_output_tokens: None` and
/// the agent would send no output cap — causing mid-tool-call truncation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TopProvider {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u64>,
}

/// The standard `/v1/models` response envelope.
#[derive(Debug, Deserialize)]
pub struct ListModelsResponse {
    pub data: Vec<DiscoveredModel>,
}

/// Fetch the list of models from an OpenAI-compatible `/v1/models` endpoint.
///
/// `api_url` should be the base URL the provider uses for chat completions
/// (e.g. `"https://api.deepinfra.com/v1/openai"`). The `/models` path is
/// appended. This matches the convention zed's `OpenAiCompatibleLanguageModel`
/// uses for chat completions (it appends `/chat/completions` to `api_url`).
///
/// The endpoint is called with a bearer token and any custom headers the user
/// configured. On any error, returns `Err(ListModelsError)`; the caller is
/// expected to log and degrade gracefully (empty model list) rather than
/// crash the provider.
pub async fn list_models(
    client: &dyn HttpClient,
    api_url: &str,
    api_key: &str,
    extra_headers: &CustomHeaders,
) -> Result<Vec<DiscoveredModel>, ListModelsError> {
    let base = api_url.trim_end_matches('/');
    let uri = format!("{base}/models");
    let request = HttpRequest::builder()
        .method(Method::GET)
        .uri(uri)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .extra_headers(extra_headers)
        .body(AsyncBody::default())
        .map_err(ListModelsError::BuildRequest)?;

    let mut response = client
        .send(request)
        .await
        .map_err(ListModelsError::HttpSend)?;

    let mut body = String::new();
    response
        .body_mut()
        .read_to_string(&mut body)
        .await
        .map_err(|e| ListModelsError::ReadResponse(e.into()))?;

    if response.status().is_success() {
        let mut parsed: ListModelsResponse =
            serde_json::from_str(&body).map_err(ListModelsError::DeserializeResponse)?;
        for model in parsed.data.iter_mut() {
            model.resolve_provider_fallbacks();
        }
        Ok(parsed.data)
    } else {
        Err(ListModelsError::ApiError {
            status: response.status().as_u16(),
            body,
        })
    }
}

impl DiscoveredModel {
    /// Whether the provider advertises tool support via `supported_parameters`.
    ///
    /// When `supported_parameters` is absent (empty), the provider didn't
    /// advertise capabilities — that's "unknown", not "unsupported".
    /// Default to `true` so providers like DeepInfra (which omit the field but
    /// whose models do support tools) don't get tool calling silently disabled.
    /// This matches `OpenAiCompatibleModelCapabilities::default()` (tools: true)
    /// used for the manual-config path, so discovery and manual config agree.
    pub fn supports_tools(&self) -> bool {
        if self.supported_parameters.is_empty() {
            true
        } else {
            self.supported_parameters.iter().any(|p| p == "tools")
        }
    }

    /// Whether the provider advertises image input support via architecture
    /// `input_modalities` containing `"image"`.
    pub fn supports_images(&self) -> bool {
        self.architecture
            .as_ref()
            .map(|arch| arch.input_modalities.iter().any(|m| m == "image"))
            .unwrap_or(false)
    }

    /// The display name, falling back to the id when absent.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// Promote `top_provider.max_completion_tokens` → `max_output_tokens` and
    /// `top_provider.context_length` → `context_length` when the top-level
    /// fields are `None`. OpenRouter nests these under `top_provider` rather
    /// than at the top level, so without this step every OpenRouter model
    /// would discover with `max_output_tokens: None` and the agent would send
    /// no output cap — causing mid-tool-call truncation (the `ToolInput::recv`
    /// warn path) when the provider applies its own default.
    pub fn resolve_provider_fallbacks(&mut self) {
        let Some(top) = self.top_provider.as_ref() else {
            return;
        };
        if self.max_output_tokens.is_none() && top.max_completion_tokens.is_some() {
            self.max_output_tokens = top.max_completion_tokens;
        }
        if self.context_length.is_none() && top.context_length.is_some() {
            self.context_length = top.context_length;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_client::FakeHttpClient;
    use serde_json::json;

    #[test]
    fn test_supports_tools_and_images_detection() {
        let model = DiscoveredModel {
            id: "test-model".into(),
            name: None,
            context_length: None,
            max_output_tokens: None,
            supported_parameters: vec!["tools".to_string(), "temperature".to_string()],
            architecture: Some(ModelArchitecture {
                input_modalities: vec!["text".into(), "image".into()],
            }),
            top_provider: None,
        };
        assert!(model.supports_tools());
        assert!(model.supports_images());
        assert_eq!(model.display_name(), "test-model");
    }

    #[test]
    fn test_display_name_falls_back_to_id() {
        let model = DiscoveredModel {
            id: "meta-llama/Llama-3.3-70B-Instruct".into(),
            name: Some("Llama 3.3 70B".into()),
            context_length: None,
            max_output_tokens: None,
            supported_parameters: Vec::new(),
            architecture: None,
            top_provider: None,
        };
        assert_eq!(model.display_name(), "Llama 3.3 70B");
    }

    #[test]
    fn test_parses_standard_openai_models_response() {
        let body = json!({
            "data": [
                { "id": "gpt-4o", "object": "model", "created": 1234567890, "owned_by": "openai" },
                { "id": "o3", "object": "model", "created": 1234567890, "owned_by": "openai" }
            ]
        });
        let parsed: ListModelsResponse = serde_json::from_str(&body.to_string()).unwrap();
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.data[0].id, "gpt-4o");
        // No `supported_parameters` field → unknown, not unsupported. Default
        // to `true` so providers that omit the field (DeepInfra, standard
        // OpenAI) don't get tool calling silently disabled.
        assert!(parsed.data[0].supports_tools());
        assert!(!parsed.data[0].supports_images());
    }

    #[test]
    fn test_parses_extended_response_with_context_and_capabilities() {
        let body = json!({
            "data": [{
                "id": "meta-llama/Llama-3.3-70B-Instruct",
                "name": "Llama 3.3 70B Instruct",
                "context_length": 128000,
                "supported_parameters": ["tools", "temperature", "reasoning"],
                "architecture": { "input_modalities": ["text", "image"] }
            }]
        });
        let parsed: ListModelsResponse = serde_json::from_str(&body.to_string()).unwrap();
        assert_eq!(parsed.data.len(), 1);
        let model = &parsed.data[0];
        assert_eq!(model.id, "meta-llama/Llama-3.3-70B-Instruct");
        assert_eq!(model.context_length, Some(128000));
        assert!(model.supports_tools());
        assert!(model.supports_images());
        assert_eq!(model.display_name(), "Llama 3.3 70B Instruct");
    }

    #[test]
    fn test_top_provider_max_completion_tokens_fallback() {
        // OpenRouter nests `max_completion_tokens` under `top_provider` rather
        // than as a top-level `max_output_tokens`. Without the fallback, every
        // OpenRouter model would discover with `max_output_tokens: None`, the
        // agent would send no output cap, and the provider would truncate
        // mid-tool-call — surfacing as the `ToolInput::recv` warn.
        let body = json!({
            "data": [{
                "id": "z-ai/glm-5.2",
                "name": "Z.ai: GLM 5.2",
                "context_length": 1048576,
                "top_provider": {
                    "context_length": 1048576,
                    "max_completion_tokens": 131072
                }
            }]
        });
        let mut parsed: ListModelsResponse = serde_json::from_str(&body.to_string()).unwrap();
        let model = parsed.data.get_mut(0).unwrap();
        model.resolve_provider_fallbacks();
        assert_eq!(
            model.max_output_tokens,
            Some(131072),
            "top_provider.max_completion_tokens must promote to max_output_tokens"
        );
        assert_eq!(
            model.context_length,
            Some(1048576),
            "top-level context_length must be preserved, not overwritten by top_provider"
        );
    }

    #[test]
    fn test_api_error_response() {
        let body = json!({
            "data": []
        });
        let parsed: ListModelsResponse = serde_json::from_str(&body.to_string()).unwrap();
        assert!(parsed.data.is_empty());
    }

    #[test]
    fn test_list_models_sends_bearer_token_and_parses_response() {
        use futures::executor::block_on;
        use std::sync::{Arc, Mutex};

        let captured_uri = Arc::new(Mutex::new(None));
        let captured_auth = Arc::new(Mutex::new(None));
        let captured_uri_for_handler = captured_uri.clone();
        let captured_auth_for_handler = captured_auth.clone();
        let http_client = FakeHttpClient::create(move |request| {
            let captured_uri = captured_uri_for_handler.clone();
            let captured_auth = captured_auth_for_handler.clone();
            async move {
                *captured_uri.lock().unwrap() = Some(request.uri().to_string());
                *captured_auth.lock().unwrap() = request
                    .headers()
                    .get("Authorization")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                let body = serde_json::json!({
                    "data": [
                        { "id": "model-a", "object": "model" },
                        { "id": "model-b", "context_length": 32000 }
                    ]
                });
                Ok(http_client::Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(AsyncBody::from(body.to_string()))
                    .unwrap())
            }
        });

        let extra_headers = CustomHeaders::default();
        let result = block_on(list_models(
            http_client.as_ref(),
            "https://api.example.com/v1",
            "test-key",
            &extra_headers,
        ));

        assert!(result.is_ok(), "expected Ok, got Err: {:?}", result.err());
        let models = result.unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "model-a");
        assert_eq!(models[1].context_length, Some(32000));
        assert_eq!(
            *captured_uri.lock().unwrap(),
            Some("https://api.example.com/v1/models".to_string())
        );
        assert_eq!(
            *captured_auth.lock().unwrap(),
            Some("Bearer test-key".to_string())
        );
    }

    #[test]
    fn test_list_models_returns_api_error_on_non_200() {
        use futures::executor::block_on;

        let http_client = FakeHttpClient::create(|_| async move {
            Ok(http_client::Response::builder()
                .status(401)
                .body(AsyncBody::from("unauthorized"))
                .unwrap())
        });

        let extra_headers = CustomHeaders::default();
        let result = block_on(list_models(
            http_client.as_ref(),
            "https://api.example.com/v1",
            "bad-key",
            &extra_headers,
        ));

        match result {
            Err(ListModelsError::ApiError { status, body }) => {
                assert_eq!(status, 401);
                assert_eq!(body, "unauthorized");
            }
            other => panic!("expected ApiError, got {:?}", other),
        }
    }
}
