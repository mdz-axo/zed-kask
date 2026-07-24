//! Ollama backend — local inference via the OpenAI-compatible API.
//!
//! Ollama exposes `/v1/chat/completions`, `/v1/models`, and `/v1/embeddings` at
//! its base URL (default `http://localhost:11434`). The `Authorization` header
//! is accepted but ignored — no API key is required for a local daemon. A key
//! may still be set (`OM_API_KEY`) for remote Ollama instances that require auth.
//!
//! Reuses [`openai_compatible_generate`] and [`stream_chat_completion`] from
//! the shared OpenAI-compatible layer, exactly like the OpenRouter backend.
//!
//! ## Verified field tolerance
//!
//! Ollama's OpenAI layer silently ignores non-standard fields hKask sends
//! (`top_k`, `min_p`, `typical_p`, `n_probs`, `enable_thinking`)
//! rather than rejecting the request — confirmed empirically against a live
//! daemon. No per-provider field suppression is needed.
//!
//! ## Caveats
//!
//! - **`enable_thinking` is a no-op.** Thinking-capable Ollama models emit
//!   reasoning in a separate `delta.reasoning` field (ignored by hKask's
//!   `StreamDelta`) and will think *even when* hKask requests
//!   `disable_thinking = true` (e.g. condenser/summarization tasks). Use a
//!   non-thinking model — or a Modelfile that disables thinking — for
//!   token-sensitive paths; this cannot be controlled via the OpenAI wire.
//! - **Vision is supported** via the shared `vision_infer` path. `build_vision_request`
//!   already emits the OpenAI content-array multimodal format (`[{image_url},{text}]`)
//!   that Ollama's OpenAI-compatible layer accepts — verified empirically. OCR routes
//!   through `InferenceRouter::generate_vision` with an `OM/`-prefixed vision model
//!   (e.g. `OM/qwen3-vl:8b`), the same path `LlmOcrExecutor` uses for cloud providers.
//! - **Streaming is buffered.** `stream_chat_completion` reads the full
//!   response body then re-emits chunks — same as every other backend today,
//!   not an Ollama-specific regression.

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

/// Sentinel used as the `Authorization: Bearer` value when no Ollama API key is
/// configured, so the header still parses cleanly. Ollama ignores the header.
const OLLAMA_SENTINEL_KEY: &str = "ollama";

/// Ollama backend for chat completions and model listing.
pub struct OllamaBackend {
    base_url: String,
    api_key: String,
    client: Arc<reqwest::Client>,
}

/// A model entry returned by Ollama's `/v1/models` endpoint (OpenAI shape).
#[derive(Debug, Deserialize, Serialize)]
pub struct OllamaModel {
    pub id: String,
    pub object: Option<String>,
    pub created: Option<u64>,
    #[serde(default)]
    pub owned_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelList {
    data: Vec<OllamaModel>,
}

impl OllamaBackend {
    /// Create a new Ollama backend from inference config.
    ///
    /// Unlike cloud providers, Ollama does **not** require an API key — a local
    /// daemon ignores the `Authorization` header. The backend is therefore
    /// considered configured whenever a base URL is present (which always
    /// defaults to `http://localhost:11434`).
    ///
    /// expect: "The system creates provider membranes assembled from configured boundaries"
    /// \[P4\] Motivating: Clear Boundaries — Ollama provider membrane requires only a reachable endpoint
    /// pre:  config.ollama_base_url is set (default applies)
    /// post: returns OllamaBackend with configured HTTP client
    pub fn new(
        config: &InferenceConfig,
        client: Arc<reqwest::Client>,
    ) -> Result<Self, InferenceError> {
        if config.ollama_base_url.is_empty() {
            return Err(InferenceError::Connection(
                "Ollama base URL not configured (set OM_BASE_URL)".into(),
            ));
        }
        Ok(Self {
            base_url: config.ollama_base_url.clone(),
            api_key: config.ollama_api_key.clone(),
            client,
        })
    }

    /// Send a chat completion request to Ollama.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated text generation
    /// pre:  model is a valid Ollama model tag (e.g. `qwen3:8b`)
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
        // Empty key is fine — Ollama ignores the header. Use a sentinel so
        // the `Bearer ` header still parses cleanly if a key is absent.
        let effective_key = if self.api_key.is_empty() {
            OLLAMA_SENTINEL_KEY
        } else {
            self.api_key.as_str()
        };
        openai_compatible_generate(
            &self.client,
            &self.base_url,
            effective_key,
            model,
            prompt,
            params,
            tools,
            "/v1/chat/completions",
            "Bearer",
            "OM",
        )
        .await
    }

    /// Send a multi-turn chat completion request to Ollama with an explicit
    /// message array.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated multi-turn text generation
    /// pre:  model is a valid Ollama model tag
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
        let effective_key = if self.api_key.is_empty() {
            OLLAMA_SENTINEL_KEY
        } else {
            self.api_key.as_str()
        };
        // Sanitize tool schemas for Ollama — its Go API cannot parse
        // boolean-valued JSON Schema fields like additionalProperties.
        let tools_owned: Option<Vec<hkask_types::ChatToolDefinition>> =
            tools.map(sanitize_tools_for_ollama);
        let tools_ref = tools_owned.as_deref();
        openai_compatible_generate_messages(
            &self.client,
            &self.base_url,
            effective_key,
            model,
            messages,
            params,
            tools_ref,
            "/v1/chat/completions",
            "Bearer",
            "OM",
        )
        .await
    }

    /// Stream a chat completion from Ollama via SSE.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated streaming text generation
    /// pre:  model is a valid Ollama model tag
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
        let key = if self.api_key.is_empty() {
            "ollama".to_string()
        } else {
            self.api_key.clone()
        };
        let auth = format!("Bearer {}", key);
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
    /// Reuses the shared `vision_infer` path -- `build_vision_request` emits the
    /// OpenAI content-array format Ollama's OpenAI-compatible layer accepts.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation -- regulated multimodal generation
    /// pre:  model is a vision-capable Ollama tag (e.g. `qwen3-vl:8b`)
    /// pre:  prompt is non-empty
    /// pre:  images is non-empty (at least one base64-encoded image)
    /// pre:  params is a valid LLMParameters
    /// post: returns Ok(InferenceResult) with vision-generated text
    /// post: if connection fails -> Err(InferenceError::Connection)
    pub async fn generate_vision(
        &self,
        model: &str,
        prompt: &str,
        images: &[String],
        params: &LLMParameters,
    ) -> Result<InferenceResult, InferenceError> {
        let key = if self.api_key.is_empty() {
            "ollama".to_string()
        } else {
            self.api_key.clone()
        };
        vision_infer(
            &self.client,
            &self.base_url,
            &key,
            "OM",
            model,
            prompt,
            images,
            params,
        )
        .await
    }

    /// List available models from Ollama.
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
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    target: "reg.inference",
                    "Ollama models request failed: {e}"
                );
                return Vec::new();
            }
        };

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            tracing::warn!(
                target: "reg.inference",
                "Ollama models error {status}: {body}"
            );
            return Vec::new();
        }

        let list: OllamaModelList = match response.json().await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    target: "reg.inference",
                    "Ollama models parse error: {e}"
                );
                return Vec::new();
            }
        };

        info!(
            target: "hkask.inference.ollama",
            count = list.data.len(),
            "Fetched Ollama model list"
        );

        list.data
            .into_iter()
            .map(|m| crate::RouterModelEntry::from_model_entry(ProviderId::Ollama, &m.id))
            .collect()
    }
}

/// Remove JSON Schema fields that Ollama's Go API cannot parse.
///
/// Ollama's Go structs expect tool parameter properties to be objects, not
/// booleans. Standard JSON Schema fields like
/// cause a 400 error. This function recursively strips those fields.
fn sanitize_tools_for_ollama(
    tools: &[hkask_types::ChatToolDefinition],
) -> Vec<hkask_types::ChatToolDefinition> {
    tools
        .iter()
        .map(|t| {
            let mut function = t.function.clone();
            function.parameters = sanitize_schema(&function.parameters);
            hkask_types::ChatToolDefinition {
                tool_type: t.tool_type.clone(),
                function,
            }
        })
        .collect()
}

fn sanitize_schema(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut cleaned = serde_json::Map::new();
            for (k, v) in map {
                // Ollama's Go API expects tool parameter properties to be
                // objects, not booleans. Strip any boolean-valued field
                // (additionalProperties, readOnly, writeOnly, deprecated, etc.).
                if v.is_boolean() {
                    continue;
                }
                cleaned.insert(k.clone(), sanitize_schema(v));
            }
            serde_json::Value::Object(cleaned)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(sanitize_schema).collect())
        }
        other => other.clone(),
    }
}
