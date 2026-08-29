use anyhow::{Context as _, Result};
use cloud_llm_client::predict_edits_v3::{RawCompletionRequest, RawCompletionResponse};
use futures::AsyncReadExt as _;
use gpui::{App, AppContext as _, Entity, Global, SharedString, Task, http_client};
use language::language_settings::{OpenAiCompatibleEditPredictionSettings, all_language_settings};
use language_model::{ApiKeyState, EnvVar, env_var};
use std::sync::Arc;

// zed-kask: D24 — Kask edit-prediction port hook.
//
// When wired (by `main.rs` after the `LanguageModelRegistry` resolves the
// edit-prediction model), this port replaces the raw HTTP call in
// `send_custom_server_request` with a call routed through the
// `LanguageModelRegistry` (resolve model → `api_url()` + `api_key()` →
// raw `/completions` POST). This collapses edit predictions onto the same
// model + credentials the agent uses, instead of a separate cloud endpoint
// or a separately-configured OpenAI-compatible server.
//
// `Mutex`-based (re-settable) — same pattern as `set_memory_port`,
// `set_thread_condenser`, etc. When `None`, `send_custom_server_request`
// falls through to the existing HTTP path (upstream behavior).
static KASK_COMPLETION_PORT: std::sync::Mutex<Option<Arc<dyn KaskCompletionPort>>> =
    std::sync::Mutex::new(None);

/// Trait implemented by the kask bridge to route raw completion requests
/// through the `LanguageModelRegistry`.
///
/// `send_completion` returns `(text, request_id)`, matching the shape of
/// `send_custom_server_request`.
pub trait KaskCompletionPort: Send + Sync {
    fn send_completion(
        &self,
        prompt: String,
        max_tokens: u32,
        stop_tokens: Vec<String>,
    ) -> futures::future::BoxFuture<'static, Result<(String, String)>>;
}

/// Wire (or unwire) the kask edit-prediction port. Called from `main.rs`
/// once the `LanguageModelRegistry` has resolved the edit-prediction model.
pub fn set_kask_completion_port(port: Option<Arc<dyn KaskCompletionPort>>) {
    *KASK_COMPLETION_PORT
        .lock()
        .expect("KASK_COMPLETION_PORT poisoned") = port;
}

fn kask_completion_port() -> Option<Arc<dyn KaskCompletionPort>> {
    KASK_COMPLETION_PORT
        .lock()
        .expect("KASK_COMPLETION_PORT poisoned")
        .clone()
}

pub fn open_ai_compatible_api_url(cx: &App) -> SharedString {
    all_language_settings(None, cx)
        .edit_predictions
        .open_ai_compatible_api
        .as_ref()
        .map(|settings| settings.api_url.clone())
        .unwrap_or_default()
        .into()
}

pub const OPEN_AI_COMPATIBLE_CREDENTIALS_USERNAME: &str = "openai-compatible-api-token";
pub static OPEN_AI_COMPATIBLE_TOKEN_ENV_VAR: std::sync::LazyLock<EnvVar> =
    env_var!("ZED_OPEN_AI_COMPATIBLE_EDIT_PREDICTION_API_KEY");

struct GlobalOpenAiCompatibleApiKey(Entity<ApiKeyState>);

impl Global for GlobalOpenAiCompatibleApiKey {}

pub fn open_ai_compatible_api_token(cx: &mut App) -> Entity<ApiKeyState> {
    if let Some(global) = cx.try_global::<GlobalOpenAiCompatibleApiKey>() {
        return global.0.clone();
    }

    let entity = cx.new(|cx| {
        ApiKeyState::new(
            open_ai_compatible_api_url(cx),
            OPEN_AI_COMPATIBLE_TOKEN_ENV_VAR.clone(),
        )
    });
    cx.set_global(GlobalOpenAiCompatibleApiKey(entity.clone()));
    entity
}

pub fn load_open_ai_compatible_api_token(
    cx: &mut App,
) -> Task<Result<(), language_model::AuthenticateError>> {
    let credentials_provider = zed_credentials_provider::global(cx);
    let api_url = open_ai_compatible_api_url(cx);
    open_ai_compatible_api_token(cx).update(cx, |key_state, cx| {
        key_state.load_if_needed(api_url, |s| s, credentials_provider, cx)
    })
}

pub fn load_open_ai_compatible_api_key_if_needed(
    provider: settings::EditPredictionProvider,
    cx: &mut App,
) -> Option<Arc<str>> {
    if provider != settings::EditPredictionProvider::OpenAiCompatibleApi {
        return None;
    }
    _ = load_open_ai_compatible_api_token(cx);
    let url = open_ai_compatible_api_url(cx);
    return open_ai_compatible_api_token(cx).read(cx).key(&url);
}

pub(crate) async fn send_custom_server_request(
    provider: settings::EditPredictionProvider,
    settings: &OpenAiCompatibleEditPredictionSettings,
    prompt: String,
    max_tokens: u32,
    stop_tokens: Vec<String>,
    api_key: Option<Arc<str>>,
    http_client: &Arc<dyn http_client::HttpClient>,
) -> Result<(String, String)> {
    // zed-kask: D24 — when the kask completion port is wired, route through
    // the `LanguageModelRegistry` instead of the configured HTTP endpoint.
    if let Some(port) = kask_completion_port() {
        return port.send_completion(prompt, max_tokens, stop_tokens).await;
    }
    match provider {
        settings::EditPredictionProvider::Ollama => {
            let response = crate::ollama::make_request(
                settings.clone(),
                prompt,
                stop_tokens,
                http_client.clone(),
            )
            .await?;
            Ok((response.response, response.created_at))
        }
        _ => {
            let request = RawCompletionRequest {
                model: settings.model.clone(),
                prompt,
                max_tokens: Some(max_tokens),
                temperature: None,
                stop: stop_tokens
                    .into_iter()
                    .map(std::borrow::Cow::Owned)
                    .collect(),
                environment: None,
            };

            let request_body = serde_json::to_string(&request)?;
            let mut http_request_builder = http_client::Request::builder()
                .method(http_client::Method::POST)
                .uri(settings.api_url.as_ref())
                .header("Content-Type", "application/json");

            if let Some(api_key) = api_key {
                http_request_builder =
                    http_request_builder.header("Authorization", format!("Bearer {}", api_key));
            }

            let http_request =
                http_request_builder.body(http_client::AsyncBody::from(request_body))?;

            let mut response = http_client.send(http_request).await?;
            let status = response.status();

            if !status.is_success() {
                let mut body = String::new();
                response.body_mut().read_to_string(&mut body).await?;
                anyhow::bail!("custom server error: {} - {}", status, body);
            }

            let mut body = String::new();
            response.body_mut().read_to_string(&mut body).await?;

            let parsed: RawCompletionResponse =
                serde_json::from_str(&body).context("Failed to parse completion response")?;
            let text = parsed
                .choices
                .into_iter()
                .next()
                .map(|choice| choice.text)
                .unwrap_or_default();
            Ok((text, parsed.id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt as _;
    use gpui::http_client::FakeHttpClient;
    use language::language_settings::{
        EditPredictionPromptFormat, OpenAiCompatibleEditPredictionSettings,
    };
    use settings::DelayMs;
    use std::sync::Arc;

    /// A mock `KaskCompletionPort` that returns a canned `(text, request_id)`.
    struct MockCompletionPort {
        text: String,
        request_id: String,
    }

    impl KaskCompletionPort for MockCompletionPort {
        fn send_completion(
            &self,
            _prompt: String,
            _max_tokens: u32,
            _stop_tokens: Vec<String>,
        ) -> futures::future::BoxFuture<'static, Result<(String, String)>> {
            let text = self.text.clone();
            let request_id = self.request_id.clone();
            async move { Ok((text, request_id)) }.boxed()
        }
    }

    /// zed-kask: D24 — when the kask completion port is wired,
    /// `send_custom_server_request` delegates to the port instead of the HTTP path.
    /// This pins the deliberate deviation: edit predictions route through the
    /// `LanguageModelRegistry`, not the configured HTTP endpoint.
    #[gpui::test]
    async fn test_kask_completion_port_intercepts_send_custom_server_request(
        _cx: &mut gpui::TestAppContext,
    ) {
        // Wire the mock port.
        set_kask_completion_port(Some(Arc::new(MockCompletionPort {
            text: "mock_completion".to_string(),
            request_id: "mock_req_1".to_string(),
        })));

        // Construct dummy params — the port intercepts before any are used.
        let settings = OpenAiCompatibleEditPredictionSettings {
            model: "".to_string(),
            max_output_tokens: 64,
            api_url: "".into(),
            prompt_format: EditPredictionPromptFormat::default(),
            prediction_debounce: DelayMs(0),
        };
        let http_client: Arc<dyn http_client::HttpClient> = FakeHttpClient::with_404_response();

        // Clean up the global via a guard so a panic doesn't leak it.
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                set_kask_completion_port(None);
            }
        }
        let _guard = Guard;

        let result = send_custom_server_request(
            settings::EditPredictionProvider::OpenAiCompatibleApi,
            &settings,
            "test prompt".to_string(),
            64,
            vec!["stop".to_string()],
            None,
            &http_client,
        )
        .await;

        let (text, request_id) = result.expect("mock port should succeed");
        assert_eq!(text, "mock_completion");
        assert_eq!(request_id, "mock_req_1");

        // Verify the global is cleaned up by the guard.
        drop(_guard);
        assert!(kask_completion_port().is_none());
    }
}
