//! Embedding generation over OpenAI-compatible provider credentials.
//!
//! The port takes a provider-bound credential bundle resolved upfront from the bridge's
//! `INFERENCE_PROVIDERS` table (env var) and makes raw
//! `/embeddings` POSTs through the app's `HttpClient`. No GPUI access is
//! needed at request time — credentials are resolved once at construction.
//!
//! The model string (e.g. `OpenRouter/qwen/qwen3-embedding-8b`) is stripped
//! of its provider prefix before being sent to the API — the provider expects
//! the bare model id, not the prefixed form.

use std::sync::Arc;

use futures::AsyncReadExt;
use hkask_types::{EmbeddingBatch, EmbeddingGenerationError};
use http_client::{AsyncBody, HttpClient, Method, Request};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};

/// Request sent to the tokio-side embedding executor.
struct EmbedRequest {
    /// The provider-prefixed model string (e.g.
    /// `OpenRouter/qwen/qwen3-embedding-8b`).
    /// The prefix is stripped before the API call.
    model: String,
    /// Texts to embed.
    texts: Vec<String>,
    /// Output width to request from MRL-capable models; `None` = native.
    dimensions: Option<u32>,
    /// Reply channel.
    reply: oneshot::Sender<Result<EmbeddingBatch, EmbeddingGenerationError>>,
}

/// OpenAI-compatible embedding response (wire format).
#[derive(Debug, Deserialize)]
struct OpenAiEmbedResponse {
    data: Vec<OpenAiEmbeddingData>,
    #[serde(default)]
    model: Option<String>,
    /// OpenAI-compatible embeddings responses carry `usage:
    /// {prompt_tokens, total_tokens}` — parse it so the batch reports
    /// measured cost (mcp-tool-review S-01 completion).
    #[serde(default)]
    usage: Option<OpenAiEmbedUsage>,
}

/// The usage block of an OpenAI-compatible embeddings response.
#[derive(Debug, Deserialize)]
struct OpenAiEmbedUsage {
    prompt_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
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
    ///
    /// `max_concurrency` bounds the number of in-flight HTTP calls, mirroring
    /// the chat port's receiver: it is the embedding-side enforcement point
    /// for `kask.general.max_concurrency`. Awaiting each HTTP call inline in
    /// the receiver loop serialized every embedding consumer behind
    /// whichever request the loop picked up first (head-of-line blocking), so
    /// a corpus embed wave ran one request at a time regardless of the
    /// caller's own fan-out.
    pub fn new(
        credentials: crate::ResolvedEmbeddingCredentials,
        http_client: Arc<dyn HttpClient>,
        tokio_handle: tokio::runtime::Handle,
        max_concurrency: usize,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<EmbedRequest>();
        let provider = credentials.provider;
        let api_key = credentials.api_key;
        let concurrency = Arc::new(tokio::sync::Semaphore::new(max_concurrency.max(1)));

        // The receiver runs on the tokio runtime — no GPUI access needed. Each
        // request is dispatched as its own spawned task gated by `concurrency`;
        // the permit is held for the task's lifetime and released on drop. A
        // dropped caller reply is detected at send time, after the accepted
        // work completes — cancellation cannot reverse work already sent.
        tokio_handle.spawn({
            let tokio_handle = tokio_handle.clone();
            async move {
                while let Some(req) = rx.recv().await {
                    let http_client = http_client.clone();
                    let api_key = api_key.clone();
                    let concurrency = concurrency.clone();
                    tokio_handle.spawn(async move {
                        let api_url = provider.api_url;
                        let result = async move {
                            let _permit = concurrency.acquire().await.map_err(|e| {
                                EmbeddingGenerationError::Connection(format!(
                                    "embedding concurrency semaphore closed: {e}"
                                ))
                            })?;
                            let (requested_provider, model_id) =
                                req.model.split_once('/').ok_or_else(|| {
                                    EmbeddingGenerationError::InvalidRequest(
                                        "model must be provider-qualified".into(),
                                    )
                                })?;
                            if !requested_provider.eq_ignore_ascii_case(provider.id)
                                || model_id.is_empty()
                            {
                                return Err(EmbeddingGenerationError::InvalidRequest(format!(
                                    "model '{}' cannot use the embedding port bound to '{}'; select a model from that provider or reconfigure the port",
                                    req.model, provider.id,
                                )));
                            }

                            // Build and send the OpenAI-compatible /embeddings request.
                            // `encoding_format: "float"` requests raw float arrays instead
                            // of the default base64 encoding — avoids ~33% wire overhead
                            // and a decode pass. DeepInfra and OpenAI both support this.
                            let mut body = serde_json::json!({
                                "model": model_id,
                                "input": req.texts,
                                "encoding_format": "float",
                            });
                            // MRL truncation: request the store's width so width-bound
                            // vec0 tables always receive fitting vectors. Providers
                            // without `dimensions` support reject or ignore it; the
                            // insert-side dimension check fails visibly either way.
                            if let Some(dimensions) = req.dimensions {
                                if let Some(map) = body.as_object_mut() {
                                    map.insert("dimensions".to_string(), serde_json::json!(dimensions));
                                }
                            }
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

                            Ok(EmbeddingBatch {
                                vectors: embeddings,
                                requested_model: req.model,
                                actual_model: parsed.model,
                                usage: match parsed.usage {
                                    Some(OpenAiEmbedUsage {
                                        prompt_tokens,
                                        total_tokens,
                                    }) => hkask_types::InferenceUsage {
                                        prompt_tokens,
                                        completion_tokens: 0,
                                        total_tokens,
                                        reported: true,
                                    },
                                    None => hkask_types::InferenceUsage::default(),
                                },
                                cost_usd: None,
                            })
                        }
                        .await;

                        if let Err(result) = req.reply.send(result) {
                            tracing::trace!(target: "hkask.inference", "embedding reply dropped — caller cancelled");
                            let _ = result;
                        }
                    });
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
                    Ok(EmbeddingBatch {
                        vectors,
                        requested_model: req.model,
                        actual_model: None,
                        // The local embed-fn path computes vectors in-process
                        // — no provider, no tokens, no cost. `reported:
                        // false` keeps the absence honest.
                        usage: hkask_types::InferenceUsage::default(),
                        cost_usd: None,
                    })
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
    pub async fn embed_with_identity(
        &self,
        model: &str,
        texts: &[String],
    ) -> Result<EmbeddingBatch, EmbeddingGenerationError> {
        self.embed_with_dimensions(model, texts, None).await
    }

    /// Generate embeddings, requesting a specific output width from
    /// MRL-capable models (the OpenAI-compatible `dimensions` body field).
    /// `None` lets the model emit its native width; callers whose vector
    /// stores are width-bound (sqlite-vec `float[N]` tables) pass the
    /// store's width so returned vectors always fit.
    pub async fn embed_with_dimensions(
        &self,
        model: &str,
        texts: &[String],
        dimensions: Option<u32>,
    ) -> Result<EmbeddingBatch, EmbeddingGenerationError> {
        if texts.is_empty() {
            return Err(EmbeddingGenerationError::EmptyResponse);
        }
        let (tx_reply, rx_reply) = oneshot::channel();
        self.tx
            .send(EmbedRequest {
                model: model.to_string(),
                texts: texts.to_vec(),
                dimensions,
                reply: tx_reply,
            })
            .map_err(|e| {
                EmbeddingGenerationError::Connection(format!("embedding port channel closed: {e}"))
            })?;
        rx_reply.await.map_err(|e| {
            EmbeddingGenerationError::Connection(format!("embedding port reply dropped: {e}"))
        })?
    }

    /// Compatibility wrapper for vector-only consumers.
    pub async fn embed(
        &self,
        model: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, EmbeddingGenerationError> {
        Ok(self.embed_with_identity(model, texts).await?.vectors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// expect: Provider substitutions and omitted identities remain distinct from the requested embedding model.
    #[tokio::test]
    async fn embedding_response_preserves_provider_confirmed_identity() {
        let provider = crate::INFERENCE_PROVIDERS
            .first()
            .expect("at least one embedding provider");
        for (response_body, expected_actual) in [
            (
                r#"{"data":[{"embedding":[1.0,0.0]}],"model":"provider/substituted-model"}"#,
                Some("provider/substituted-model"),
            ),
            (r#"{"data":[{"embedding":[1.0,0.0]}]}"#, None),
        ] {
            let response_body = response_body.to_string();
            let http_client = http_client::FakeHttpClient::create(move |_| {
                let response_body = response_body.clone();
                async move {
                    Ok(http_client::Response::builder()
                        .status(200)
                        .body(AsyncBody::from_bytes(response_body.into_bytes().into()))?)
                }
            });
            let port = LanguageModelEmbeddingPort::new(
                crate::ResolvedEmbeddingCredentials {
                    provider,
                    api_key: "fixture-key".into(),
                },
                http_client,
                tokio::runtime::Handle::current(),
                4,
            );
            let requested = format!("{}/requested-alias", provider.id);
            let batch = port
                .embed_with_identity(&requested, &["source text".to_string()])
                .await
                .expect("embedding succeeds");
            assert_eq!(batch.requested_model, requested);
            assert_eq!(batch.actual_model.as_deref(), expected_actual);
            assert_eq!(batch.vectors, vec![vec![1.0, 0.0]]);
        }
    }

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
                4,
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

    /// expect: Concurrent embed requests dispatch concurrently — the receiver
    /// spawns one task per request, so a slow HTTP call does not head-of-line
    /// block the requests behind it.
    /// why: The receiver loop previously awaited each HTTP call inline, which
    /// serialized every embedding consumer into one request at a time
    /// regardless of the caller's fan-out; a corpus embed wave ran at ~1
    /// batch per HTTP round-trip. The spawned-task receiver must overlap
    /// in-flight HTTP calls up to the semaphore bound.
    /// pre: a fake HTTP client that sleeps 100ms per call and tracks the
    /// concurrent in-flight count; four embed requests issued together.
    /// post: all four succeed and the observed maximum in-flight count is at
    /// least 2 (all four are dispatched before the first sleep completes, so
    /// the true overlap is 4; the assertion floor tolerates scheduler jitter).
    #[tokio::test]
    async fn concurrent_embed_requests_overlap() {
        let provider = crate::INFERENCE_PROVIDERS
            .first()
            .expect("at least one embedding provider");
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let http_client = http_client::FakeHttpClient::create({
            let in_flight = in_flight.clone();
            let max_in_flight = max_in_flight.clone();
            move |_| {
                let in_flight = in_flight.clone();
                let max_in_flight = max_in_flight.clone();
                async move {
                    let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    max_in_flight.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    Ok(http_client::Response::builder()
                        .status(200)
                        .body(AsyncBody::from_bytes(
                            br#"{"data":[{"embedding":[1.0,0.0]}]}"#.to_vec().into(),
                        ))?)
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
            8,
        );
        let model = format!("{}/organization/embedding", provider.id);
        let t1 = vec!["source text".to_string()];
        let t2 = vec!["source text".to_string()];
        let t3 = vec!["source text".to_string()];
        let t4 = vec!["source text".to_string()];
        let (r1, r2, r3, r4) = tokio::join!(
            port.embed_with_identity(&model, &t1),
            port.embed_with_identity(&model, &t2),
            port.embed_with_identity(&model, &t3),
            port.embed_with_identity(&model, &t4),
        );
        for result in [r1, r2, r3, r4] {
            assert_eq!(
                result.expect("embedding succeeds").vectors,
                vec![vec![1.0, 0.0]]
            );
        }
        assert!(
            max_in_flight.load(Ordering::SeqCst) >= 2,
            "concurrent embeds must overlap; max in-flight was {}",
            max_in_flight.load(Ordering::SeqCst)
        );
    }
}
