//! Embedding generation over OpenAI-compatible provider credentials.
//!
//! The port takes a provider-bound credential bundle resolved upfront from the bridge's
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
/// Construct with `ResolvedEmbeddingCredentials` resolved from the bridge's
/// `INFERENCE_PROVIDERS` table and the app's `HttpClient`. The port is
/// `Send + Sync` — no GPUI access is needed at request time.
#[derive(Clone)]
pub struct LanguageModelEmbeddingPort {
    tx: mpsc::UnboundedSender<EmbedRequest>,
}

impl LanguageModelEmbeddingPort {
    /// Construct the port and spawn the receiver task on the tokio runtime.
    ///
    /// The provider descriptor and bearer key are resolved together by
    /// `resolve_embedding_credentials`. Requests must name that provider;
    /// mismatches are rejected before serialization or HTTP. The `tokio_handle`
    /// is used to spawn the receiver task (obtained via
    /// `gpui_tokio::Tokio::handle(cx)` at the call site).
    pub fn new(
        credentials: crate::ResolvedEmbeddingCredentials,
        http_client: Arc<dyn HttpClient>,
        tokio_handle: tokio::runtime::Handle,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<EmbedRequest>();
        let provider = credentials.provider;
        let api_key = credentials.api_key;

        // The receiver runs on the tokio runtime — no GPUI access needed.
        tokio_handle.spawn(async move {
            while let Some(req) = rx.recv().await {
                let http_client = http_client.clone();
                let api_url = provider.api_url;
                let api_key = api_key.clone();
                let result = async move {
                    let (requested_provider, model_id) = req.model.split_once('/').ok_or_else(|| {
                        EmbeddingGenerationError::InvalidRequest("model must be provider-qualified".into())
                    })?;
                    if !requested_provider.eq_ignore_ascii_case(provider.id) || model_id.is_empty() {
                        return Err(EmbeddingGenerationError::InvalidRequest(format!(
                            "model '{}' cannot use the embedding port bound to '{}'; select a model from that provider or reconfigure the port",
                            req.model, provider.id,
                        )));
                    }

                    // Build and send the OpenAI-compatible /embeddings request.
                    // `encoding_format: "float"` requests raw float arrays instead
                    // of the default base64 encoding — avoids ~33% wire overhead
                    // and a decode pass. DeepInfra and OpenAI both support this.
                    let body = serde_json::json!({
                        "model": model_id,
                        "input": req.texts,
                        "encoding_format": "float",
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
        F: Fn(&str) -> Vec<f32> + Send + Sync + ?Sized + 'static,
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
    /// `DEFAULT_EMBEDDING_MODEL`). The prefix must match the bound provider
    /// before it is stripped for the API call.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// expect: "An embedding override never sends my text to a different provider" [P1]
    #[tokio::test]
    async fn embedding_provider_mismatch_sends_no_http() {
        for provider in crate::INFERENCE_PROVIDERS {
            let sends = Arc::new(AtomicUsize::new(0));
            let http_client = http_client::FakeHttpClient::create({
                let sends = sends.clone();
                move |mut request| {
                    let sends = sends.clone();
                    async move {
                        sends.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(
                            request.uri().to_string(),
                            format!("{}/embeddings", provider.api_url)
                        );
                        assert_eq!(request.headers()["Authorization"], "Bearer fixture-key");
                        let mut text = String::new();
                        request.body_mut().read_to_string(&mut text).await?;
                        let body: serde_json::Value = serde_json::from_str(&text)?;
                        assert_eq!(body["model"], "organization/embedding");
                        assert_eq!(body["input"], serde_json::json!(["private source"]));
                        Ok(http_client::Response::builder().status(200).body(
                            AsyncBody::from_bytes(
                                br#"{"data":[{"embedding":[1.0,0.0]}]}"#.to_vec().into(),
                            ),
                        )?)
                    }
                }
            });
            let port = LanguageModelEmbeddingPort::new(
                crate::ResolvedEmbeddingCredentials {
                    provider,
                    api_key: "fixture-key".into(),
                },
                http_client,
                tokio::runtime::Handle::current(),
            );
            let input = vec!["private source".to_string()];
            for other in crate::INFERENCE_PROVIDERS
                .iter()
                .filter(|other| other.id != provider.id)
            {
                let error = port
                    .embed(&format!("{}/embedding", other.id), &input)
                    .await
                    .expect_err("provider mismatch");
                assert!(matches!(error, EmbeddingGenerationError::InvalidRequest(_)));
            }
            for model in [
                "unqualified",
                "unknown/model",
                "🦀🦀🦀/model",
                &format!("{}/", provider.id),
            ] {
                assert!(matches!(
                    port.embed(model, &input).await,
                    Err(EmbeddingGenerationError::InvalidRequest(_))
                ));
            }
            assert_eq!(
                sends.load(Ordering::SeqCst),
                0,
                "rejected inputs must not reach transport"
            );
            assert_eq!(
                port.embed(
                    &format!("{}/organization/embedding", provider.id.to_lowercase()),
                    &input
                )
                .await
                .expect("same provider"),
                vec![vec![1.0, 0.0]]
            );
            assert_eq!(sends.load(Ordering::SeqCst), 1);
        }
    }
}
