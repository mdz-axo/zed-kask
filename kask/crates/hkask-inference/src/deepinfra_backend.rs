//! DeepInfra backend — media generation (background removal, TTS, STT).
//!
//! DeepInfra exposes `/v1/inference/{model}` for media ops and an OpenAI-compatible
//! `/v1/chat/completions` for chat. In zed-kask, chat/vision/embed/list_models route
//! through the zed IPC bridge (`InferenceIpcClient` → `LanguageModelRegistry`), so
//! this backend only implements the three `MediaProvider` ops it is cheapest for:
//! background removal (Bria RMBG 2.0), speech (Kokoro TTS), and transcription
//! (Whisper). Requires Bearer token authentication via `DEEPINFRA_API_KEY`.

use crate::config::InferenceConfig;
use crate::openai_compat::sanitize_error_body;
use crate::provider::{MediaOp, MediaProvider};
use hkask_types::{InferenceError, MediaGenerateParams};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// DeepInfra backend for media generation (background removal, TTS, STT).
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
                "DeepInfra API key not configured (set DEEPINFRA_API_KEY)".into(),
            ));
        }
        Ok(Self {
            base_url: config.deepinfra_base_url.clone(),
            api_key: config.deepinfra_api_key.clone(),
            client,
        })
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
                model,
                status,
                sanitize_error_body(&text)
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
                status,
                sanitize_error_body(&error_text)
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
                status,
                sanitize_error_body(&text)
            )));
        }

        serde_json::from_str(&text)
            .map_err(|e| InferenceError::Json(format!("DeepInfra STT parse: {}", e)))
    }
}

impl MediaProvider for DeepInfraBackend {
    fn id(&self) -> &'static str {
        "deepinfra"
    }

    /// DeepInfra is the preferred provider for the three ops it is cheapest
    /// for (background removal, TTS, STT). It also has `generate_image` /
    /// `image_to_image` methods, but those are intentionally NOT advertised
    /// here: leaving them out of `supports()` preserves the existing
    /// DeepInfra-first / fal-fallback dispatch exactly (fal.ai remains the
    /// sole provider for image/video generation). Register DeepInfra first
    /// in `ProviderRegistry` so it is preferred for these three ops.
    fn supports(&self, op: MediaOp) -> bool {
        matches!(
            op,
            MediaOp::RemoveBackground | MediaOp::GenerateSpeech | MediaOp::Transcribe
        )
    }

    fn execute<'a>(
        &'a self,
        op: MediaOp,
        params: &'a MediaGenerateParams,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, InferenceError>> + Send + 'a>> {
        Box::pin(async move {
            match op {
                MediaOp::RemoveBackground => {
                    let image_url = params.image_url.clone().unwrap_or_default();
                    self.remove_background(&image_url).await
                }
                MediaOp::GenerateSpeech => {
                    let text = params.text.clone().unwrap_or_default();
                    let voice = params.voice.clone().unwrap_or_else(|| "Rachel".to_string());
                    // model_id None → DeepInfra picks its default TTS model.
                    self.generate_speech(&text, &voice, None).await
                }
                MediaOp::Transcribe => {
                    let audio_url = params.audio_url.clone().unwrap_or_default();
                    self.transcribe(&audio_url, params.language.as_deref())
                        .await
                }
                // Unreachable: supports() returns false for these. A clear
                // error guards against a registry misconfiguration that calls
                // a provider for an op it doesn't support.
                other => Err(InferenceError::Connection(format!(
                    "deepinfra does not support media op: {}",
                    other.as_str()
                ))),
            }
        })
    }
}
