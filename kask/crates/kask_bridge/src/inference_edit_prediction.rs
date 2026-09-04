//! Edit-prediction FIM completions over OpenAI-compatible provider credentials.
//!
//! Routes edit-prediction FIM completions through the `LanguageModelRegistry`,
//! reusing the same OpenRouter model + credentials the agent uses. Mirrors
//! `LanguageModelEmbeddingPort` (raw HTTP POST via `HttpClient`) but targets
//! `/completions` instead of `/embeddings`.
//!
//! Credentials (`api_url`, `api_key`) and the bare model id are resolved once at
//! construction from `LanguageModelRegistry::resolve_model_names` + the model's
//! `api_url()`/`api_key()` trait accessors (D24 overrides on `OpenRouterLanguageModel`).
//! No GPUI access is needed at request time — the port is `Send + Sync`.

use std::sync::Arc;

use edit_prediction::open_ai_compatible::KaskCompletionPort;
use futures::{AsyncReadExt, FutureExt};

use http_client::{AsyncBody, HttpClient, Method, Request};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

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
    ///
    /// `pub(crate)` because the only caller is `from_registry`, which is the
    /// public entry point. Tightening from `pub` per the essentialist G2
    /// finding (zero external callers confirmed via grep).
    pub(crate) fn new(
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

    /// Resolve the port from the `LanguageModelRegistry` — the visible
    /// chain only (the operator's no-hidden-models spec):
    /// 1. `kask.models.default_model` when the user set it,
    /// 2. else the zed default model (the user's active default, visible
    ///    in Settings → AI).
    /// Never a code constant. Returns `None` when nothing resolves or the
    /// resolved model has no `api_url`/`api_key` — edit prediction then
    /// falls back to the user's own configured provider (fail-visible, not
    /// a hidden model).
    pub fn from_registry(
        registry: &language_model::LanguageModelRegistry,
        http_client: Arc<dyn HttpClient>,
        tokio_handle: tokio::runtime::Handle,
        kask_default_model: Option<&str>,
        cx: &gpui::App,
    ) -> Option<Self> {
        let model = match kask_default_model {
            Some(name) => {
                crate::model_resolution::resolve_model_names(registry, &[name.to_string()], cx)
                    .0
                    .into_values()
                    .next()?
            }
            None => registry.default_model()?.model,
        };

        let api_url = model.api_url(cx)?;
        let api_key = model.api_key(cx)?;
        let model_id = model.id().0.to_string();

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
