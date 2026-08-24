//! Embedding generation over OpenAI-compatible provider credentials.
//!
//! The port takes `(api_url, api_key)` resolved upfront from the bridge's
//! `INFERENCE_PROVIDERS` table (env var) and makes raw
//! `/embeddings` POSTs through the app's `HttpClient`. No GPUI access is
//! needed at request time — credentials are resolved once at construction.
//!
//! The model string (e.g. `OpenRouter/qwen/qwen3-embedding`) is stripped
//! of its provider prefix before being sent to the API — the provider expects
//! the bare model id, not the prefixed form.

use std::sync::Arc;

use futures::AsyncReadExt;
use hkask_types::EmbeddingGenerationError;
use http_client::{AsyncBody, HttpClient, Method, Request};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};

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

/// OpenAI-compatible embedding response (wire format).
#[derive(Debug, Deserialize)]
struct OpenAiEmbedResponse {
    data: Vec<OpenAiEmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingData {
    embedding: Vec<f32>,
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
                    let model_id = crate::inference_providers::strip_provider_prefix(&req.model);

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
