//! Together AI backend — cloud inference via OpenAI-compatible API.
//!
//! Together AI exposes `/v1/chat/completions` and `/v1/models`.
//! Requires Bearer token authentication via `TG_API_KEY`.

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

/// Together AI backend for chat completions and model listing.
pub struct TogetherBackend {
    base_url: String,
    api_key: String,
    client: Arc<reqwest::Client>,
}

/// A model entry returned by Together AI's `/v1/models` endpoint.
#[derive(Debug, Deserialize, Serialize)]
pub struct TogetherModel {
    pub id: String,
    pub object: String,
    pub created: Option<u64>,
    #[serde(default)]
    pub owned_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TogetherModelList {
    data: Vec<TogetherModel>,
}

impl TogetherBackend {
    /// Create a new Together backend from inference config.
    ///
    /// Returns an error if `together_api_key` is empty.
    ///
    /// expect: "The system creates provider membranes requiring valid API keys"
    /// \[P4\] Motivating: Clear Boundaries — Together AI provider membrane requires valid API key
    /// pre:  config.together_api_key is set
    /// post: returns TogetherBackend with configured HTTP client
    pub fn new(
        config: &InferenceConfig,
        client: Arc<reqwest::Client>,
    ) -> Result<Self, InferenceError> {
        if config.together_api_key.is_empty() {
            return Err(InferenceError::Connection(
                "Together AI API key not configured (set TG_API_KEY)".into(),
            ));
        }
        Ok(Self {
            base_url: config.together_base_url.clone(),
            api_key: config.together_api_key.clone(),
            client,
        })
    }

    /// Send a chat completion request to Together AI.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated text generation
    /// pre:  model is a valid Together AI model name
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
            "TG",
        )
        .await
    }

    /// Send a multi-turn chat completion request to Together AI with an explicit
    /// message array.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated multi-turn text generation
    /// pre:  model is a valid Together AI model name
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
            "TG",
        )
        .await
    }

    /// Stream a chat completion from Together AI via SSE.
    /// Generate a streaming completion from Together.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated streaming text generation
    /// pre:  model is a valid Together model name
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
    /// pre:  model is a valid Together AI vision-capable model name
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
            "TG",
            model,
            prompt,
            images,
            params,
        )
        .await
    }

    /// List available models from Together AI.
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
                "Together AI models error {}: {}",
                status, body
            );
            return Vec::new();
        }

        let list: TogetherModelList = match response.json().await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    target: "reg.inference",
                    "Together AI models parse error: {}",
                    e
                );
                return Vec::new();
            }
        };

        info!(
            target: "hkask.inference.together",
            count = list.data.len(),
            "Fetched Together AI model list"
        );

        list.data
            .into_iter()
            .map(|m| crate::RouterModelEntry::from_model_entry(ProviderId::Together, &m.id))
            .collect()
    }
}
