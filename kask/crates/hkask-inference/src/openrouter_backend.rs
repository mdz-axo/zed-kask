//! OpenRouter backend — cloud inference via OpenAI-compatible API.
//!
//! OpenRouter exposes `/v1/chat/completions` and `/v1/models` at
//! `https://openrouter.ai/api`. Requires Bearer token
//! authentication via `OR_API_KEY`.
//!
//! OpenRouter provides a unified API to hundreds of models from
//! multiple providers through a single endpoint.

use crate::chat_protocol::{stream_chat_completion, vision_infer};
use crate::config::InferenceConfig;
use crate::openai_compat::{openai_compatible_generate, openai_compatible_generate_messages};
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferenceResult, InferenceStreamChunk,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

/// OpenRouter backend for chat completions and model listing.
pub struct OpenRouterBackend {
    base_url: String,
    api_key: String,
    client: Arc<reqwest::Client>,
}

/// A model entry returned by OpenRouter's `/v1/models` endpoint.
#[derive(Debug, Deserialize, Serialize)]
pub struct OpenRouterModel {
    pub id: String,
    pub object: Option<String>,
    pub created: Option<u64>,
    #[serde(default)]
    pub owned_by: Option<String>,
    /// Display name (e.g. "Z.ai: GLM 5.2").
    #[serde(default)]
    pub name: Option<String>,
    /// Pricing per token (multiply by 1_000_000 for per-million).
    #[serde(default)]
    pub pricing: Option<OpenRouterPricing>,
    /// Architecture metadata (modalities, tokenizer).
    #[serde(default)]
    pub architecture: Option<OpenRouterArchitecture>,
    /// Benchmark scores (intelligence_index, agentic_index, etc.).
    #[serde(default)]
    pub benchmarks: Option<serde_json::Value>,
    /// Supported parameters (tools, temperature, reasoning, etc.).
    #[serde(default)]
    pub supported_parameters: Vec<String>,
    /// Context length in tokens.
    #[serde(default)]
    pub context_length: Option<u64>,
}

/// Pricing fields from OpenRouter's `/v1/models` response.
///
/// All values are USD per token — multiply by 1_000_000 for per-million.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpenRouterPricing {
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub completion: Option<String>,
}

/// Architecture metadata from OpenRouter's `/v1/models` response.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpenRouterArchitecture {
    #[serde(default)]
    pub modality: Option<String>,
    #[serde(default)]
    pub input_modalities: Vec<String>,
    #[serde(default)]
    pub output_modalities: Vec<String>,
    #[serde(default)]
    pub tokenizer: Option<String>,
}

/// A model that passed the favorites thresholds.
///
/// Returned by `discover_favorites` — models that meet the price and
/// intelligence gates, sorted by intelligence index descending.
#[derive(Debug, Clone, Serialize)]
pub struct FavoriteModel {
    /// Provider-prefixed model ID (e.g. "OpenRouter/z-ai/glm-5.2").
    pub prefixed_id: String,
    /// Raw model ID from OpenRouter (e.g. "z-ai/glm-5.2").
    pub id: String,
    /// Display name.
    pub name: String,
    /// Intelligence index (0–100 scale).
    pub intelligence_index: f64,
    /// Prompt price per million tokens (USD).
    pub prompt_price_per_m: f64,
    /// Completion price per million tokens (USD).
    pub completion_price_per_m: f64,
    /// Context length in tokens.
    pub context_length: u64,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelList {
    data: Vec<OpenRouterModel>,
}

impl OpenRouterBackend {
    /// Create a new OpenRouter backend from inference config.
    ///
    /// Returns an error if `openrouter_api_key` is empty.
    ///
    /// expect: "The system creates provider membranes requiring valid API keys"
    /// \[P4\] Motivating: Clear Boundaries — OpenRouter provider membrane requires valid API key
    /// pre:  config.openrouter_api_key is set
    /// post: returns OpenRouterBackend with configured HTTP client
    pub fn new(
        config: &InferenceConfig,
        client: Arc<reqwest::Client>,
    ) -> Result<Self, InferenceError> {
        if config.openrouter_api_key.is_empty() {
            return Err(InferenceError::Connection(
                "OpenRouter API key not configured (set OR_API_KEY)".into(),
            ));
        }
        Ok(Self {
            base_url: config.openrouter_base_url.clone(),
            api_key: config.openrouter_api_key.clone(),
            client,
        })
    }

    /// Construct a backend for public-endpoint discovery (no API key required).
    ///
    /// The `/v1/models` endpoint is public — this constructor allows
    /// favorites discovery without an API key. When a key is provided, it's
    /// sent as a Bearer token to personalize results, but it's not required.
    #[must_use]
    pub fn new_public(base_url: &str, api_key: &str, client: Arc<reqwest::Client>) -> Self {
        Self {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            client,
        }
    }

    /// Send a chat completion request to OpenRouter.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated text generation
    /// pre:  model is a valid OpenRouter model name
    /// pre:  prompt is non-empty (validated by validate_prompt)
    /// pre:  params is a valid LLMParameters
    /// post: returns Ok(InferenceResult) with generated text, model, usage stats
    /// post: if connection fails → Err(InferenceError::Connection)
    /// post: if prompt is empty → Err(InferenceError::Generation)
    pub async fn generate(
        &self,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Result<InferenceResult, InferenceError> {
        openai_compatible_generate(
            &self.client,
            &self.base_url,
            &self.api_key,
            model,
            prompt,
            params,
            tools,
            "/v1/chat/completions",
            "Bearer",
            "OR",
        )
        .await
    }

    /// Send a multi-turn chat completion request to OpenRouter with an explicit
    /// message array.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated multi-turn text generation
    /// pre:  model is a valid OpenRouter model name
    /// pre:  messages is non-empty
    /// pre:  params is a valid LLMParameters
    /// post: returns Ok(InferenceResult) with generated text, model, usage stats
    /// post: if connection fails → Err(InferenceError::Connection)
    pub async fn generate_with_messages(
        &self,
        model: &str,
        messages: &[ChatMessage],
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Result<InferenceResult, InferenceError> {
        openai_compatible_generate_messages(
            &self.client,
            &self.base_url,
            &self.api_key,
            model,
            messages,
            params,
            tools,
            "/v1/chat/completions",
            "Bearer",
            "OR",
        )
        .await
    }

    /// Stream a chat completion from OpenRouter via SSE.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated streaming text generation
    /// pre:  model is a valid OpenRouter model name
    /// post: returns stream of inference chunks
    pub fn generate_stream(
        &self,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn futures_util::Stream<Item = Result<InferenceStreamChunk, InferenceError>>
                + Send
                + '_,
        >,
    > {
        let auth = format!("Bearer {}", self.api_key);
        stream_chat_completion(
            Arc::clone(&self.client),
            self.base_url.clone(),
            auth,
            model.to_string(),
            prompt.to_string(),
            params.clone(),
            tools.map(|t| t.to_vec()),
        )
    }

    /// Vision/multimodal inference with base64-encoded images.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated multimodal generation
    /// pre:  model is a valid OpenRouter vision-capable model name
    /// pre:  prompt is non-empty
    /// pre:  images is non-empty (at least one base64-encoded image)
    /// pre:  params is a valid LLMParameters
    /// post: returns Ok(InferenceResult) with vision-generated text
    /// post: if connection fails → Err(InferenceError::Connection)
    pub async fn generate_vision(
        &self,
        model: &str,
        prompt: &str,
        images: &[String],
        params: &LLMParameters,
    ) -> Result<InferenceResult, InferenceError> {
        vision_infer(
            &self.client,
            &self.base_url,
            &self.api_key,
            "OR",
            model,
            prompt,
            images,
            params,
        )
        .await
    }

    /// List available models from OpenRouter.
    ///
    /// Returns `RouterModelEntry` with provider prefix applied on each entry.
    /// Graceful degradation: returns empty vec on any error.
    ///
    /// expect: "I can discover available models across providers"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — model variety discovery
    /// pre:  self.client and self.base_url are initialized
    /// post: returns `Vec<RouterModelEntry>` with all available models
    /// post: if API or parse fails → returns empty vec (graceful degradation)
    #[must_use]
    pub async fn list_models(&self) -> Vec<crate::RouterModelEntry> {
        use crate::config::ProviderId;

        let response = match self
            .client
            .get(format!("{}/v1/models", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            tracing::warn!(
                target: "reg.inference",
                "OpenRouter models error {}: {}",
                status, body
            );
            return Vec::new();
        }

        let list: OpenRouterModelList = match response.json().await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    target: "reg.inference",
                    "OpenRouter models parse error: {}",
                    e
                );
                return Vec::new();
            }
        };

        info!(
            target: "hkask.inference.openrouter",
            count = list.data.len(),
            "Fetched OpenRouter model list"
        );

        list.data
            .into_iter()
            .map(|m| crate::RouterModelEntry::from_model_entry(ProviderId::OpenRouter, &m.id))
            .collect()
    }

    /// Discover "favorite" models from OpenRouter that pass the price and
    /// intelligence thresholds.
    ///
    /// Uses the `/v1/models` endpoint with server-side filtering:
    /// `?output_modalities=text&sort=intelligence-high-to-low&supported_parameters=temperature,top_p,structured_outputs,tools,reasoning&max_price={max_price}&min_intelligence_index={min_intelligence}`
    ///
    /// The endpoint is public (no API key required for discovery), but we
    /// send the Authorization header when a key is available so OpenRouter
    /// can personalize results.
    ///
    /// Returns models sorted by intelligence index descending. On any error
    /// (network, parse, non-200), returns an empty vec — graceful degradation.
    ///
    /// expect: "I can discover affordable, high-quality models for fusion"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — favorites discovery
    /// pre:  self.client and self.base_url are initialized
    /// post: returns Vec<FavoriteModel> sorted by intelligence_index desc
    /// post: on any error → returns empty vec
    #[must_use]
    pub async fn discover_favorites(
        &self,
        max_price_per_m: f64,
        min_intelligence_index: f64,
    ) -> Vec<FavoriteModel> {
        use crate::config::ProviderId;

        const SUPPORTED_PARAMS: &str = "temperature,top_p,structured_outputs,tools,reasoning";

        let url = format!(
            "{}/v1/models?output_modalities=text&sort=intelligence-high-to-low&supported_parameters={}&max_price={}&min_intelligence_index={}",
            self.base_url.trim_end_matches('/'),
            SUPPORTED_PARAMS,
            max_price_per_m,
            min_intelligence_index
        );

        let mut req = self
            .client
            .get(&url)
            .header("User-Agent", "zed-kask-fusion");
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    target: "reg.fusion",
                    error = %e,
                    "OpenRouter favorites discovery request failed"
                );
                return Vec::new();
            }
        };

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            tracing::warn!(
                target: "reg.fusion",
                status = %status,
                body = %body,
                "OpenRouter favorites discovery returned non-200"
            );
            return Vec::new();
        }

        let list: OpenRouterModelList = match response.json().await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    target: "reg.fusion",
                    error = %e,
                    "OpenRouter favorites discovery parse error"
                );
                return Vec::new();
            }
        };

        let mut favorites: Vec<FavoriteModel> = list
            .data
            .into_iter()
            .filter_map(|model| {
                // Parse prompt price (per-token → per-million).
                let prompt_price_per_m = model
                    .pricing
                    .as_ref()
                    .and_then(|p| p.prompt.as_deref())
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|v| v * 1_000_000.0)?;

                // Parse completion price (per-token → per-million).
                let completion_price_per_m = model
                    .pricing
                    .as_ref()
                    .and_then(|p| p.completion.as_deref())
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|v| v * 1_000_000.0)
                    .unwrap_or(0.0);

                // Parse intelligence index from benchmarks.
                let intelligence_index =
                    find_index(&model.benchmarks, "intelligence_index").unwrap_or(-1.0);

                // Server-side filtering should have already applied these gates,
                // but we re-check client-side as a safety net.
                if prompt_price_per_m > max_price_per_m {
                    return None;
                }
                if intelligence_index < min_intelligence_index {
                    return None;
                }

                let name = model.name.clone().unwrap_or_else(|| model.id.clone());

                Some(FavoriteModel {
                    prefixed_id: ProviderId::OpenRouter.prefix_model(&model.id),
                    id: model.id,
                    name,
                    intelligence_index,
                    prompt_price_per_m,
                    completion_price_per_m,
                    context_length: model.context_length.unwrap_or(0),
                })
            })
            .collect();

        // Sort by intelligence index descending (server should already do this).
        favorites.sort_by(|a, b| {
            b.intelligence_index
                .partial_cmp(&a.intelligence_index)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        info!(
            target: "reg.fusion",
            count = favorites.len(),
            "Discovered OpenRouter favorites (max_price=${}/M, min_ia={})",
            max_price_per_m,
            min_intelligence_index
        );

        favorites
    }
}

/// Recursively search a JSON value for a numeric field by name.
/// Used to extract `intelligence_index` from the nested `benchmarks` object.
fn find_index(value: &Option<serde_json::Value>, key: &str) -> Option<f64> {
    let value = value.as_ref()?;
    find_index_recursive(value, key)
}

fn find_index_recursive(value: &serde_json::Value, key: &str) -> Option<f64> {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            if let Some(v) = map.get(key) {
                if let Some(f) = v.as_f64() {
                    return Some(f);
                }
                if let Some(s) = v.as_str() {
                    return s.parse::<f64>().ok();
                }
            }
            for v in map.values() {
                if let Some(found) = find_index_recursive(v, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .filter_map(|v| find_index_recursive(v, key))
            .next(),
        _ => None,
    }
}
