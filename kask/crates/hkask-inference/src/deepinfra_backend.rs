//! DeepInfra backend — cloud inference via OpenAI-compatible API.
//!
//! DeepInfra exposes `/v1/chat/completions` and `/v1/models` at
//! `https://api.deepinfra.com`. Requires Bearer token
//! authentication via `DI_API_KEY`.
//!
//! DeepInfra has the broadest open-source model catalog and the
//! lowest per-token pricing among GPU cloud providers.

use crate::chat_protocol::{stream_chat_completion, vision_infer};
use crate::config::InferenceConfig;
use crate::openai_compat::{openai_compatible_generate, openai_compatible_generate_messages};
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferenceResult, InferenceStreamChunk,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// DeepInfra backend for chat completions and model listing.
pub struct DeepInfraBackend {
    base_url: String,
    api_key: String,
    client: Arc<reqwest::Client>,
}

impl DeepInfraBackend {
    /// Create a new DeepInfra backend from inference config.
    ///
    /// Returns an error if `deepinfra_api_key` is empty.
    ///
    /// expect: "The system creates provider membranes requiring valid API keys"
    /// \[P4\] Motivating: Clear Boundaries — DeepInfra provider membrane requires valid API key
    /// pre:  config.deepinfra_api_key is set
    /// post: returns DeepInfraBackend with configured HTTP client
    pub fn new(
        config: &InferenceConfig,
        client: Arc<reqwest::Client>,
    ) -> Result<Self, InferenceError> {
        if config.deepinfra_api_key.is_empty() {
            return Err(InferenceError::Connection(
                "DeepInfra API key not configured (set DI_API_KEY)".into(),
            ));
        }
        Ok(Self {
            base_url: config.deepinfra_base_url.clone(),
            api_key: config.deepinfra_api_key.clone(),
            client,
        })
    }

    /// Send a chat completion request to DeepInfra.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated text generation
    /// pre:  model is a valid DeepInfra model name
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
            "DI",
        )
        .await
    }

    /// Send a multi-turn chat completion request to DeepInfra with an explicit
    /// message array. Each message carries its own role (`"system"`, `"user"`,
    /// `"assistant"`), so the provider sees the full conversation history.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated multi-turn text generation
    /// pre:  model is a valid DeepInfra model name
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
            "DI",
        )
        .await
    }

    /// Vision/multimodal inference with base64-encoded images.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated multimodal generation
    /// pre:  model is a valid DeepInfra vision-capable model name
    /// pre:  prompt is non-empty
    /// pre:  images is non-empty (at least one base64-encoded image)
    /// pre:  params is a valid LLMParameters
    /// post: returns Ok(InferenceResult) with vision-generated text
    /// post: if images is empty → Err(InferenceError::Generation("No images provided"))
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
            "DI",
            model,
            prompt,
            images,
            params,
        )
        .await
    }

    /// Stream a chat completion from DeepInfra via SSE.
    /// Generate a streaming completion from DeepInfra.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated streaming text generation
    /// pre:  model is a valid DeepInfra model name
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

    /// List models from DeepInfra via `/v1/models`, filtered to last 6 months.
    ///
    /// Returns `RouterModelEntry` with provider prefix applied on each entry.
    /// Graceful degradation: returns empty vec on any error.
    ///
    /// expect: "I can discover available models across providers"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — model variety discovery with freshness filter
    /// pre:  self.client and self.base_url are initialized
    /// post: returns `Vec<RouterModelEntry>` with models updated in last 180 days
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

        if !response.status().is_success() {
            return Vec::new();
        }

        let list: DeepInfraModelList = match response.json().await {
            Ok(l) => l,
            Err(_) => return Vec::new(),
        };

        // Filter to models updated in the last 6 months
        let cutoff = chrono::Utc::now() - chrono::Duration::days(180);
        list.data
            .into_iter()
            .filter(|m| {
                m.created_at
                    .as_ref()
                    .and_then(|ts| {
                        chrono::DateTime::parse_from_rfc3339(ts)
                            .ok()
                            .or_else(|| {
                                tracing::warn!(target: "reg.inference", model = %m.id, created_at = %ts, "DeepInfra model has unparseable created_at, filtering out");
                                None
                            })
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                    })
                    .map(|dt| dt >= cutoff)
                    .unwrap_or(false)
            })
            .map(|m| crate::RouterModelEntry::from_model_entry(ProviderId::DeepInfra, &m.id))
            .collect()
    }

    // ── Media generation methods ───────────────────────────────────────────

    /// Call a DeepInfra inference endpoint for image generation.
    /// DeepInfra image models use POST /v1/inference/{model} with custom bodies.
    async fn di_inference_post(
        &self,
        model: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, InferenceError> {
        let url = format!("{}/v1/inference/{}", self.base_url, model);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("DeepInfra request failed: {}", e)))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InferenceError::Connection(format!(
                "DeepInfra {} status {}: {}",
                model, status, text
            )));
        }
        serde_json::from_str(&text)
            .map_err(|e| InferenceError::Json(format!("DeepInfra JSON parse: {}", e)))
    }

    /// Remove background from an image using Bria RMBG 2.0.
    /// Model: Bria/remove_background — $0.018/image, commercial-ready.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated image transformation
    /// pre:  image_url is a valid, accessible image URL
    /// post: returns Ok(serde_json::Value) with background-removed image data
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn remove_background(
        &self,
        image_url: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        let body = serde_json::json!({"image_url": image_url});
        self.di_inference_post("Bria/remove_background", body).await
    }

    /// Generate an image from a text prompt using FLUX 2 Klein.
    /// Model: black-forest-labs/FLUX-2-klein-4b — fast 4B param FLUX.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated image generation
    /// pre:  prompt is a non-empty text description
    /// post: returns Ok(serde_json::Value) with generated image data (1024x1024)
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn generate_image(
        &self,
        prompt: &str,
        _image_size: Option<&str>,
    ) -> Result<serde_json::Value, InferenceError> {
        let body = serde_json::json!({
            "prompt": prompt,
            "width": 1024,
            "height": 1024,
        });
        self.di_inference_post("black-forest-labs/FLUX-2-klein-4b", body)
            .await
    }

    /// Edit/transform an image using Qwen Image Edit.
    /// Model: Qwen/Qwen-Image-Edit — style transfer, precise edits.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated image editing
    /// pre:  image_url is a valid, accessible image URL
    /// pre:  prompt is a non-empty edit instruction
    /// post: returns Ok(serde_json::Value) with edited image data
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn image_to_image(
        &self,
        image_url: &str,
        prompt: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        let body = serde_json::json!({
            "image_url": image_url,
            "prompt": prompt,
        });
        self.di_inference_post("Qwen/Qwen-Image-Edit", body).await
    }

    /// Generate speech from text with a voice description.
    /// Uses DeepInfra's ElevenLabs-compatible TTS API.
    /// Default model: hexgrad/Kokoro-82M.
    /// API: POST /v1/text-to-speech/{voice_id}
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated speech synthesis
    /// pre:  text is non-empty
    /// pre:  voice_id is a valid voice identifier
    /// post: returns Ok(serde_json::Value) with base64-encoded MP3 audio
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn generate_speech(
        &self,
        text: &str,
        voice_id: &str,
        model_id: Option<&str>,
    ) -> Result<serde_json::Value, InferenceError> {
        let model = model_id.unwrap_or("hexgrad/Kokoro-82M");
        let url = format!("{}/v1/text-to-speech/{}", self.base_url, voice_id);
        let body = serde_json::json!({
            "text": text,
            "model_id": model,
            "output_format": "mp3",
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("DeepInfra TTS failed: {}", e)))?;

        let status = resp.status();
        if !status.is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(InferenceError::Connection(format!(
                "DeepInfra TTS status {}: {}",
                status, error_text
            )));
        }

        // TTS returns raw audio bytes — wrap in a JSON response with metadata
        let audio_bytes = resp
            .bytes()
            .await
            .map_err(|e| InferenceError::Connection(format!("DeepInfra TTS read failed: {}", e)))?;

        // Return as base64 data URI for portability
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&audio_bytes);
        Ok(serde_json::json!({
            "audio": format!("data:audio/mp3;base64,{}", b64),
            "format": "mp3",
            "model": model,
            "voice_id": voice_id,
        }))
    }

    /// Transcribe speech audio to text using Whisper.
    /// Uses DeepInfra's OpenAI-compatible audio transcription endpoint.
    /// API: POST /v1/audio/transcriptions
    /// Requests word-level timestamps for interactive transcript bundles.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated speech transcription
    /// pre:  audio_url is a valid, accessible audio file URL
    /// post: returns Ok(serde_json::Value) with verbose_json transcription (word+segment timestamps)
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn transcribe(
        &self,
        audio_url: &str,
        language: Option<&str>,
    ) -> Result<serde_json::Value, InferenceError> {
        let url = format!("{}/v1/audio/transcriptions", self.base_url);
        let mut body = serde_json::json!({
            "file": audio_url,
            "model": "openai/whisper-large-v3",
            "response_format": "verbose_json",
            "timestamp_granularities": ["word", "segment"],
        });
        if let Some(lang) = language {
            body["language"] = serde_json::json!(lang);
        }

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("DeepInfra STT failed: {}", e)))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InferenceError::Connection(format!(
                "DeepInfra STT status {}: {}",
                status, text
            )));
        }

        serde_json::from_str(&text)
            .map_err(|e| InferenceError::Json(format!("DeepInfra STT parse: {}", e)))
    }
}

// ── DeepInfra model types ────────────────────────────────────────────────────

/// A model entry from DeepInfra's `/v1/models` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepInfraModelEntry {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub owned_by: Option<String>,
}

/// OpenAI-compatible model list response.
#[derive(Debug, Deserialize)]
struct DeepInfraModelList {
    data: Vec<DeepInfraModelEntry>,
}
