//! Embedding router — multi-provider embedding generation.
//!
//! Routes embedding requests to DeepInfra or OpenRouter based on
//! the 2-letter provider prefix. Both use OpenAI-compatible `/v1/embeddings`.

use crate::config::{InferenceConfig, ProviderId};
use hkask_types::EmbeddingGenerationError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

/// Multi-provider embedding router.
///
/// DeepInfra and OpenRouter use `/v1/embeddings` (OpenAI-compatible).
pub struct EmbeddingRouter {
    config: InferenceConfig,
    deepinfra_client: Option<Arc<reqwest::Client>>,
    openrouter_client: Option<Arc<reqwest::Client>>,
    ollama_client: Option<Arc<reqwest::Client>>,
}

impl EmbeddingRouter {
    /// Create a new embedding router from an `InferenceConfig`.
    ///
    /// expect: "The system creates multi-provider membranes assembled from configured boundaries"
    /// \[P4\] Motivating: Clear Boundaries — embedding provider membrane gated by API key
    /// pre:  config is a valid InferenceConfig
    /// post: returns EmbeddingRouter with configured backends
    pub fn new(config: InferenceConfig) -> Self {
        let deepinfra_client = Self::build_gated_client(
            &config,
            !config.deepinfra_api_key.is_empty(),
            "DeepInfra embeddings unavailable (no API key)",
        );
        let openrouter_client = Self::build_gated_client(
            &config,
            !config.openrouter_api_key.is_empty(),
            "OpenRouter embeddings unavailable (no API key)",
        );
        let ollama_client = Self::build_gated_client(
            &config,
            !config.ollama_base_url.is_empty(),
            "Ollama embeddings unavailable (no base URL)",
        );
        Self {
            config,
            deepinfra_client,
            openrouter_client,
            ollama_client,
        }
    }

    /// Create an embedding router with a shared HTTP client.
    ///
    /// expect: "The system creates multi-provider membranes assembled from configured boundaries"
    /// \[P4\] Motivating: Clear Boundaries — embedding provider with shared connection pool
    /// pre:  config is a valid InferenceConfig; client is a configured reqwest::Client
    /// post: returns EmbeddingRouter with DeepInfra client from shared pool
    pub fn with_client(config: &InferenceConfig, client: Arc<reqwest::Client>) -> Self {
        let deepinfra_client = if config.deepinfra_api_key.is_empty() {
            warn!(target: "reg.inference", "DeepInfra embeddings unavailable (no API key)");
            None
        } else {
            Some(Arc::clone(&client))
        };

        let openrouter_client = if config.openrouter_api_key.is_empty() {
            warn!(target: "reg.inference", "OpenRouter embeddings unavailable (no API key)");
            None
        } else {
            Some(Arc::clone(&client))
        };

        Self {
            config: config.clone(),
            deepinfra_client,
            openrouter_client,
            ollama_client: if config.ollama_base_url.is_empty() {
                warn!(target: "reg.inference", "Ollama embeddings unavailable (no base URL)");
                None
            } else {
                Some(Arc::clone(&client))
            },
        }
    }

    /// Build an embedding HTTP client gated on a boolean availability check.
    ///
    /// Collapses the former per-provider `build_deepinfra_client` /
    /// `build_openrouter_client` / `build_ollama_client` triplet (each differed
    /// only in the gate condition and the warn message). The shared
    /// `build_client` + `map(Arc::new)` + warn-on-fail logic lives here once.
    ///
    /// `available` is the gate (e.g. `!config.X_api_key.is_empty()` for cloud
    /// providers, `!config.ollama_base_url.is_empty()` for key-less Ollama);
    /// `unavailable_msg` is logged when the gate is false.
    fn build_gated_client(
        config: &InferenceConfig,
        available: bool,
        unavailable_msg: &str,
    ) -> Option<Arc<reqwest::Client>> {
        if !available {
            warn!(target: "reg.inference", "{unavailable_msg}");
            return None;
        }
        config
            .build_client()
            .map(Arc::new)
            .map_err(|e| warn!(target: "reg.inference", "Embedding client build failed: {}", e))
            .ok()
    }

    /// Resolve provider and stripped model name from a model identifier.
    fn resolve(&self, model: &str) -> Result<(ProviderId, String), EmbeddingGenerationError> {
        let (provider, stripped) =
            ProviderId::parse_from_model(model).unwrap_or((self.config.default_provider, model));

        let available = match provider {
            ProviderId::DeepInfra => self.deepinfra_client.is_some(),
            ProviderId::OpenRouter => self.openrouter_client.is_some(),
            ProviderId::Fal => false,
            ProviderId::Together => false,
            ProviderId::KiloCode => false,
            ProviderId::Runpod => false,
            ProviderId::Ollama => self.ollama_client.is_some(),
            ProviderId::Cline => false,
        };

        if !available {
            return Err(EmbeddingGenerationError::Connection(format!(
                "Provider {} is not available for embeddings",
                provider.as_str()
            )));
        }

        Ok((provider, stripped.to_string()))
    }

    /// Generate embedding vectors for multiple sentences.
    ///
    /// One vector per input sentence, same order. Dimension set by model.
    ///
    /// expect: "The system generates regulated embeddings"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated batch embedding generation
    /// pre:  model is a valid provider-prefixed model name
    /// pre:  sentences is non-empty
    /// post: returns ``Vec<Vec<f32>>`` with one vector per sentence, same order
    /// post: if sentences is empty → Err(EmptyResponse)
    /// post: if provider is Fal → Err(Connection) (fal.ai does not support embeddings)
    #[must_use = "result must be used"]
    pub async fn embed_sentences(
        &self,
        model: &str,
        sentences: &[&str],
    ) -> Result<Vec<Vec<f32>>, EmbeddingGenerationError> {
        if sentences.is_empty() {
            return Err(EmbeddingGenerationError::EmptyResponse);
        }

        let (provider, model) = self.resolve(model)?;
        let texts: Vec<String> = sentences.iter().map(|s| s.to_string()).collect();

        let result = match provider {
            ProviderId::DeepInfra => {
                let client = self.deepinfra_client.as_ref().ok_or_else(|| {
                    EmbeddingGenerationError::Connection("DeepInfra client not initialized".into())
                })?;
                self.embed_openai(
                    client,
                    &self.config.deepinfra_base_url,
                    &self.config.deepinfra_api_key,
                    &model,
                    &texts,
                )
                .await?
            }
            ProviderId::Fal => {
                return Err(EmbeddingGenerationError::Connection(
                    "fal.ai does not support embeddings".into(),
                ));
            }
            ProviderId::Together => {
                return Err(EmbeddingGenerationError::Connection(
                    "Together AI embedding client not yet implemented".into(),
                ));
            }
            ProviderId::OpenRouter => {
                let client = self.openrouter_client.as_ref().ok_or_else(|| {
                    EmbeddingGenerationError::Connection("OpenRouter client not initialized".into())
                })?;
                self.embed_openai(
                    client,
                    &self.config.openrouter_base_url,
                    &self.config.openrouter_api_key,
                    &model,
                    &texts,
                )
                .await?
            }
            ProviderId::KiloCode => {
                return Err(EmbeddingGenerationError::Connection(
                    "KiloCode does not support embeddings yet".into(),
                ));
            }
            ProviderId::Runpod => {
                return Err(EmbeddingGenerationError::Connection(
                    "Runpod is an adapter-composition provider".into(),
                ));
            }
            ProviderId::Ollama => {
                let client = self.ollama_client.as_ref().ok_or_else(|| {
                    EmbeddingGenerationError::Connection("Ollama client not initialized".into())
                })?;
                let key = if self.config.ollama_api_key.is_empty() {
                    "ollama".to_string()
                } else {
                    self.config.ollama_api_key.clone()
                };
                self.embed_openai(client, &self.config.ollama_base_url, &key, &model, &texts)
                    .await?
            }
            ProviderId::Cline => {
                return Err(EmbeddingGenerationError::Connection(
                    "Cline is a chat gateway, not an embedding provider".into(),
                ));
            }
        };

        if result.len() != sentences.len() {
            return Err(EmbeddingGenerationError::DimensionMismatch {
                expected: sentences.len(),
                actual: result.len(),
            });
        }

        info!(
            target: "reg.inference",
            provider = %provider.as_str(),
            model = %model,
            count = sentences.len(),
            dim = result.first().map(|v| v.len()).unwrap_or(0),
            "Embedding generation completed"
        );

        Ok(result)
    }

    /// Convenience wrapper around `embed_sentences`.
    ///
    /// expect: "The system generates regulated embeddings"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated single embedding generation
    /// pre:  model is a valid provider-prefixed model name
    /// pre:  sentence is a non-empty string
    /// post: returns `Vec<f32>` — the first (only) embedding vector
    /// post: delegates to embed_sentences, inherits its error conditions
    #[must_use = "result must be used"]
    pub async fn embed_sentence(
        &self,
        model: &str,
        sentence: &str,
    ) -> Result<Vec<f32>, EmbeddingGenerationError> {
        let results = self.embed_sentences(model, &[sentence]).await?;
        results
            .into_iter()
            .next()
            .ok_or(EmbeddingGenerationError::EmptyResponse)
    }

    /// OpenAI-compatible embedding via `/v1/embeddings` (DeepInfra).
    async fn embed_openai(
        &self,
        client: &reqwest::Client,
        base_url: &str,
        api_key: &str,
        model: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, EmbeddingGenerationError> {
        let request = OpenAiEmbedRequest {
            model: model.to_string(),
            input: texts.to_vec(),
        };

        let response = client
            .post(format!("{}/v1/embeddings", base_url))
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| EmbeddingGenerationError::Connection(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(EmbeddingGenerationError::Api(status.as_u16(), error_text));
        }

        let openai_response: OpenAiEmbedResponse = response.json().await.map_err(|e| {
            EmbeddingGenerationError::Json(format!("OpenAI embed JSON parse: {}", e))
        })?;

        let embeddings: Vec<Vec<f32>> = openai_response
            .data
            .into_iter()
            .map(|d| d.embedding)
            .collect();

        if embeddings.is_empty() {
            return Err(EmbeddingGenerationError::EmptyResponse);
        }

        Ok(embeddings)
    }
}

// ── Wire format types ────────────────────────────────────────────────────────

/// OpenAI-compatible embed request.
#[derive(Debug, Clone, Serialize)]
struct OpenAiEmbedRequest {
    model: String,
    input: Vec<String>,
}

/// OpenAI embed response wraps embeddings in a `data` array.
#[derive(Debug, Deserialize)]
struct OpenAiEmbedResponse {
    data: Vec<OpenAiEmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingData {
    embedding: Vec<f32>,
}
