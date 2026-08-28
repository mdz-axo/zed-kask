//! Media provider implementations for OpenRouter and DeepInfra.
//!
//! Both providers implement [`MediaProvider`] and are registered in
//! [`crate::media_router::MediaRouter::new`]. Providers are constructed
//! lazily — only when their API key is present.
//!
//! ## DeepInfra
//!
//! DeepInfra serves media generation via two API surfaces:
//! - **OpenAI-compatible** (`/v1/openai/...`): image generation, image edits
//! - **Native** (`/v1/inference/{model}`): TTS (Kokoro), STT (Whisper),
//!   text-to-video (Wan), background removal (Bria RMBG), upscale
//!
//! The native API returns raw bytes for TTS (audio), inline `data:` URIs for
//! video, and JSON for STT. Multipart form upload is used for STT audio input.
//!
//! ## OpenRouter
//!
//! OpenRouter provides dedicated endpoints for media generation:
//! - **Image generation**: `/v1/chat/completions` with image-generation models
//! - **TTS**: `/v1/audio/speech` (OpenAI-compatible, returns raw audio bytes)
//! - **STT**: `/v1/audio/transcriptions` (base64 JSON input, returns JSON)
//! - **Video generation**: `/v1/videos` (async submit+poll, returns video URL)

use crate::config::InferenceConfig;
use crate::openai_compat::sanitize_error_body;
use crate::provider::{MediaOp, MediaProvider};
use hkask_types::{InferenceError, MediaGenerateParams};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

// ── DeepInfra ─────────────────────────────────────────────────────────────

/// DeepInfra media generation backend.
///
/// Handles image generation (FLUX via OpenAI-compatible API), TTS (Kokoro via
/// native inference API), STT (Whisper via native inference API with multipart
/// upload), text-to-video (Wan via native inference API), background removal
/// (Bria RMBG), and image upscaling. Requires `DEEPINFRA_API_KEY`.
pub struct DeepInfraMediaProvider {
    base_url: String,
    api_key: String,
    client: Arc<reqwest::Client>,
}

impl DeepInfraMediaProvider {
    /// Construct from inference config. Returns `Err` if the API key is empty.
    pub fn new(
        config: &InferenceConfig,
        client: Arc<reqwest::Client>,
    ) -> Result<Self, InferenceError> {
        if config.deepinfra_api_key.is_empty() {
            return Err(InferenceError::NotConfigured(
                "DeepInfra API key not configured (set DEEPINFRA_API_KEY)".into(),
            ));
        }
        Ok(Self {
            base_url: config.deepinfra_base_url.clone(),
            api_key: config.deepinfra_api_key.clone(),
            client,
        })
    }

    /// POST JSON to a DeepInfra endpoint and return the JSON response.
    async fn post_json(&self, url: &str, body: Value) -> Result<Value, InferenceError> {
        let resp = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("DeepInfra request failed: {e}")))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InferenceError::Connection(format!(
                "DeepInfra {status}: {}",
                sanitize_error_body(&text)
            )));
        }
        serde_json::from_str(&text)
            .map_err(|e| InferenceError::Json(format!("DeepInfra JSON parse: {e}")))
    }

    /// POST JSON to a DeepInfra native inference endpoint (`/v1/inference/{model}`)
    /// and return the raw response bytes (for TTS which returns audio, not JSON).
    async fn post_inference_raw(
        &self,
        model: &str,
        body: Value,
    ) -> Result<bytes::Bytes, InferenceError> {
        let url = format!("{}/v1/inference/{}", self.base_url, model);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("DeepInfra inference failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(InferenceError::Connection(format!(
                "DeepInfra inference {status}: {}",
                sanitize_error_body(&text)
            )));
        }
        resp.bytes().await.map_err(|e| {
            InferenceError::Connection(format!("DeepInfra inference read failed: {e}"))
        })
    }

    /// Generate an image via DeepInfra's OpenAI-compatible images endpoint.
    /// Uses FLUX models by default. Returns `b64_json` image data.
    async fn generate_image(
        &self,
        prompt: &str,
        size: Option<&str>,
        count: Option<u32>,
        model: &str,
    ) -> Result<Value, InferenceError> {
        let url = format!("{}/v1/openai/images/generations", self.base_url);
        let mut body = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "n": count.unwrap_or(1),
            "response_format": "b64_json",
        });
        if let Some(sz) = size {
            body["size"] = Value::String(sz.to_string());
        }
        self.post_json(&url, body).await
    }

    /// Transform an image via DeepInfra's native inference API.
    /// Passes the image URL and prompt to the model's edit endpoint.
    /// When `mask` is provided, it's included in the request body for
    /// region-selective editing (inpainting).
    async fn image_to_image(
        &self,
        image_url: &str,
        prompt: &str,
        strength: Option<f32>,
        model: &str,
        mask: Option<&str>,
    ) -> Result<Value, InferenceError> {
        let mut body = serde_json::json!({
            "prompt": prompt,
            "image_url": image_url,
        });
        if let Some(s) = strength {
            body["strength"] = serde_json::json!(s);
        }
        if let Some(m) = mask {
            body["mask_url"] = serde_json::json!(m);
        }
        self.post_json(&format!("{}/v1/inference/{}", self.base_url, model), body)
            .await
    }

    /// Remove background via DeepInfra native inference endpoint (Bria RMBG 2.0).
    async fn remove_background(&self, image_url: &str) -> Result<Value, InferenceError> {
        let body = serde_json::json!({"image_url": image_url});
        self.post_json(
            &format!("{}/v1/inference/Bria/remove_background", self.base_url),
            body,
        )
        .await
    }

    /// Upscale an image via DeepInfra native inference endpoint.
    async fn upscale(&self, image_url: &str, scale: Option<u32>) -> Result<Value, InferenceError> {
        let mut body = serde_json::json!({"image_url": image_url});
        if let Some(s) = scale {
            body["outscale"] = serde_json::json!(s);
        }
        self.post_json(
            &format!("{}/v1/inference/latentconsistency/upscale", self.base_url),
            body,
        )
        .await
    }

    /// Generate speech via DeepInfra native inference API (Kokoro).
    ///
    /// The native `/v1/inference/{model}` endpoint returns raw audio bytes
    /// (WAV format). We base64-encode them into a data URI for portability.
    async fn generate_speech(
        &self,
        text: &str,
        voice: &str,
        model: &str,
    ) -> Result<Value, InferenceError> {
        let body = serde_json::json!({
            "text": text,
            "voice": voice,
        });
        let audio_bytes = self.post_inference_raw(model, body).await?;

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&audio_bytes);
        Ok(serde_json::json!({
            "audio": format!("data:audio/wav;base64,{b64}"),
            "format": "wav",
            "model": model,
            "voice_id": voice,
        }))
    }

    /// Transcribe audio via DeepInfra native inference API (Whisper).
    ///
    /// Uses multipart form upload (`-F audio=@file`) to the
    /// `/v1/inference/{model}` endpoint. The audio file is downloaded from
    /// the URL first, then uploaded as a multipart form field.
    async fn transcribe(
        &self,
        audio_url: &str,
        language: Option<&str>,
        model: &str,
    ) -> Result<Value, InferenceError> {
        // Download the audio file from the URL.
        let audio_bytes = self
            .client
            .get(audio_url)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("audio download failed: {e}")))?
            .bytes()
            .await
            .map_err(|e| InferenceError::Connection(format!("audio read failed: {e}")))?;

        let format = detect_audio_format(audio_url);
        let filename = format!("audio.{format}");
        let mime = match format.as_str() {
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            _ => "application/octet-stream",
        };

        let url = format!("{}/v1/inference/{}", self.base_url, model);

        // Build multipart form: audio file + optional language parameter.
        let mut form = reqwest::multipart::Form::new().part(
            "audio",
            reqwest::multipart::Part::bytes(audio_bytes.to_vec())
                .file_name(filename)
                .mime_str(mime)
                .map_err(|e| {
                    InferenceError::Connection(format!("multipart mime set failed: {e}"))
                })?,
        );
        if let Some(lang) = language {
            form = form.text("language", lang.to_string());
        }

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("DeepInfra STT failed: {e}")))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InferenceError::Connection(format!(
                "DeepInfra STT {status}: {}",
                sanitize_error_body(&text)
            )));
        }
        serde_json::from_str(&text)
            .map_err(|e| InferenceError::Json(format!("DeepInfra STT parse: {e}")))
    }

    /// Generate a video via DeepInfra native inference API (Wan).
    ///
    /// The native `/v1/inference/{model}` endpoint returns a JSON response
    /// with `video_url` as an inline `data:video/mp4;base64,...` URI.
    async fn generate_video(
        &self,
        prompt: &str,
        duration: Option<f32>,
        model: &str,
    ) -> Result<Value, InferenceError> {
        let mut body = serde_json::json!({
            "prompt": prompt,
        });
        if let Some(dur) = duration {
            body["duration"] = serde_json::json!(dur);
        }
        self.post_json(&format!("{}/v1/inference/{}", self.base_url, model), body)
            .await
    }

    /// Animate a still image into a video via DeepInfra native inference API.
    async fn image_to_video(
        &self,
        image_url: &str,
        prompt: Option<&str>,
        duration: Option<f32>,
        model: &str,
    ) -> Result<Value, InferenceError> {
        let mut body = serde_json::json!({
            "image_url": image_url,
        });
        if let Some(p) = prompt {
            body["prompt"] = Value::String(p.to_string());
        }
        if let Some(dur) = duration {
            body["duration"] = serde_json::json!(dur);
        }
        self.post_json(&format!("{}/v1/inference/{}", self.base_url, model), body)
            .await
    }
}

impl MediaProvider for DeepInfraMediaProvider {
    fn id(&self) -> &'static str {
        "deepinfra"
    }

    fn supports(&self, op: MediaOp) -> bool {
        matches!(
            op,
            MediaOp::GenerateImage
                | MediaOp::ImageToImage
                | MediaOp::RemoveBackground
                | MediaOp::Upscale
                | MediaOp::GenerateSpeech
                | MediaOp::Transcribe
                | MediaOp::GenerateVideo
                | MediaOp::ImageToVideo
        )
    }

    fn execute<'a>(
        &'a self,
        op: MediaOp,
        params: &'a MediaGenerateParams,
    ) -> Pin<Box<dyn Future<Output = Result<Value, InferenceError>> + Send + 'a>> {
        Box::pin(async move {
            match op {
                MediaOp::GenerateImage => {
                    let prompt = params.prompt.clone().unwrap_or_default();
                    let model = crate::model_constants::resolve(
                        "HKASK_MEDIA_IMAGE_GEN_MODEL",
                        crate::model_constants::DEFAULT_IMAGE_GEN_MODEL,
                    );
                    let model = strip_prefix(&model, "DeepInfra/");
                    self.generate_image(&prompt, params.size.as_deref(), params.count, &model)
                        .await
                }
                MediaOp::ImageToImage => {
                    let image_url = params.image_url.clone().unwrap_or_default();
                    let prompt = params.prompt.clone().unwrap_or_default();
                    let model = crate::model_constants::resolve(
                        "HKASK_MEDIA_IMAGE_GEN_MODEL",
                        crate::model_constants::DEFAULT_IMAGE_GEN_MODEL,
                    );
                    let model = strip_prefix(&model, "DeepInfra/");
                    self.image_to_image(
                        &image_url,
                        &prompt,
                        params.strength,
                        &model,
                        params.mask.as_deref(),
                    )
                    .await
                }
                MediaOp::RemoveBackground => {
                    let image_url = params.image_url.clone().unwrap_or_default();
                    self.remove_background(&image_url).await
                }
                MediaOp::Upscale => {
                    let image_url = params.image_url.clone().unwrap_or_default();
                    self.upscale(&image_url, params.scale).await
                }
                MediaOp::GenerateSpeech => {
                    let text = params.text.clone().unwrap_or_default();
                    let voice = params.voice.clone().unwrap_or_else(|| "Rachel".to_string());
                    let model = crate::model_constants::resolve(
                        "HKASK_MEDIA_TTS_MODEL",
                        crate::model_constants::DEFAULT_TTS_MODEL,
                    );
                    let model = strip_prefix(&model, "DeepInfra/");
                    self.generate_speech(&text, &voice, &model).await
                }
                MediaOp::Transcribe => {
                    let audio_url = params.audio_url.clone().unwrap_or_default();
                    let model = crate::model_constants::resolve(
                        "HKASK_MEDIA_STT_MODEL",
                        crate::model_constants::DEFAULT_STT_MODEL,
                    );
                    let model = strip_prefix(&model, "DeepInfra/");
                    self.transcribe(&audio_url, params.language.as_deref(), &model)
                        .await
                }
                MediaOp::GenerateVideo => {
                    let prompt = params.prompt.clone().unwrap_or_default();
                    let model = crate::model_constants::resolve(
                        "HKASK_MEDIA_VIDEO_MODEL",
                        "Wan-AI/Wan2.2-T2V-A14B",
                    );
                    let model = strip_prefix(&model, "DeepInfra/");
                    self.generate_video(&prompt, params.duration, &model).await
                }
                MediaOp::ImageToVideo => {
                    let image_url = params.image_url.clone().unwrap_or_default();
                    let model = crate::model_constants::resolve(
                        "HKASK_MEDIA_VIDEO_MODEL",
                        "Wan-AI/Wan2.2-T2V-A14B",
                    );
                    let model = strip_prefix(&model, "DeepInfra/");
                    self.image_to_video(
                        &image_url,
                        params.prompt.as_deref(),
                        params.duration,
                        &model,
                    )
                    .await
                }
            }
        })
    }
}

// ── OpenRouter ────────────────────────────────────────────────────────────

/// Poll interval for OpenRouter async video generation tasks.
const OPENROUTER_VIDEO_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Max poll iterations for OpenRouter video generation (5s × 600 = 50 min).
const OPENROUTER_VIDEO_MAX_POLLS: u32 = 600;

/// OpenRouter media generation backend.
///
/// OpenRouter provides dedicated endpoints for media generation:
/// - **Image generation**: `/v1/chat/completions` with image-generation models
///   (models return image URLs in the chat response content)
/// - **TTS**: `/v1/audio/speech` (OpenAI-compatible, returns raw audio bytes)
/// - **STT**: `/v1/audio/transcriptions` (base64 JSON input, returns JSON)
/// - **Video generation**: `/v1/videos` (async submit+poll, returns video URL)
///
/// Auth: `Authorization: Bearer {OPENROUTER_API_KEY}`.
pub struct OpenRouterMediaProvider {
    base_url: String,
    api_key: String,
    client: Arc<reqwest::Client>,
}

impl OpenRouterMediaProvider {
    /// Construct from inference config. Returns `Err` if the API key is empty.
    pub fn new(
        config: &InferenceConfig,
        client: Arc<reqwest::Client>,
    ) -> Result<Self, InferenceError> {
        if config.openrouter_api_key.is_empty() {
            return Err(InferenceError::NotConfigured(
                "OpenRouter API key not configured (set OPENROUTER_API_KEY)".into(),
            ));
        }
        Ok(Self {
            base_url: config.openrouter_base_url.clone(),
            api_key: config.openrouter_api_key.clone(),
            client,
        })
    }

    /// Generate an image via OpenRouter chat completions.
    ///
    /// Image-generation models on OpenRouter return image URLs in their
    /// chat completion response content. We send a chat completion request
    /// and extract the image URL from the response.
    async fn generate_image(
        &self,
        prompt: &str,
        size: Option<&str>,
        count: Option<u32>,
        model: &str,
    ) -> Result<Value, InferenceError> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let mut body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": "Generate an image based on the user's description. Return ONLY the image URL, nothing else."},
                {"role": "user", "content": prompt},
            ],
            "n": count.unwrap_or(1),
        });
        if let Some(sz) = size {
            body["extra_body"] = serde_json::json!({"size": sz});
        }

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("OpenRouter image gen failed: {e}")))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InferenceError::Connection(format!(
                "OpenRouter image gen {status}: {}",
                sanitize_error_body(&text)
            )));
        }

        let json: Value = serde_json::from_str(&text)
            .map_err(|e| InferenceError::Json(format!("OpenRouter image gen parse: {e}")))?;

        let choices = json["choices"].as_array().ok_or_else(|| {
            InferenceError::Connection("OpenRouter: no choices in response".into())
        })?;

        let images: Vec<Value> = choices
            .iter()
            .filter_map(|choice| {
                let content = choice["message"]["content"].as_str()?;
                extract_image_url(content)
            })
            .collect();

        if images.is_empty() {
            return Err(InferenceError::Connection(
                "OpenRouter: no image URL found in response".into(),
            ));
        }

        Ok(serde_json::json!({
            "data": images,
            "model": model,
        }))
    }

    /// Generate speech via OpenRouter's dedicated TTS endpoint.
    ///
    /// Uses `/v1/audio/speech` (OpenAI-compatible). Returns raw audio bytes
    /// which we base64-encode into a data URI for portability.
    async fn generate_speech(
        &self,
        text: &str,
        voice: &str,
        model: &str,
    ) -> Result<Value, InferenceError> {
        let url = format!("{}/v1/audio/speech", self.base_url);
        let body = serde_json::json!({
            "model": model,
            "input": text,
            "voice": voice,
            "response_format": "mp3",
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("OpenRouter TTS failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(InferenceError::Connection(format!(
                "OpenRouter TTS {status}: {}",
                sanitize_error_body(&error_text)
            )));
        }

        let audio_bytes = resp
            .bytes()
            .await
            .map_err(|e| InferenceError::Connection(format!("OpenRouter TTS read failed: {e}")))?;

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&audio_bytes);
        Ok(serde_json::json!({
            "audio": format!("data:audio/mp3;base64,{b64}"),
            "format": "mp3",
            "model": model,
            "voice_id": voice,
        }))
    }

    /// Transcribe audio via OpenRouter's dedicated STT endpoint.
    ///
    /// Uses `/v1/audio/transcriptions`. OpenRouter requires base64-encoded
    /// audio (not URLs), so we download the audio from the URL first, then
    /// base64-encode it. Supports `verbose_json` with word-level timestamps.
    async fn transcribe(
        &self,
        audio_url: &str,
        language: Option<&str>,
        model: &str,
    ) -> Result<Value, InferenceError> {
        // OpenRouter STT requires base64-encoded audio data (no URL support).
        // Download the audio file and encode it.
        let audio_bytes = self
            .client
            .get(audio_url)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("audio download failed: {e}")))?
            .bytes()
            .await
            .map_err(|e| InferenceError::Connection(format!("audio read failed: {e}")))?;

        let format = detect_audio_format(audio_url);
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&audio_bytes);

        let url = format!("{}/v1/audio/transcriptions", self.base_url);
        let mut body = serde_json::json!({
            "model": model,
            "input_audio": {
                "data": b64,
                "format": format,
            },
            "response_format": "verbose_json",
            "timestamp_granularities": ["word", "segment"],
        });
        if let Some(lang) = language {
            body["language"] = Value::String(lang.to_string());
        }

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("OpenRouter STT failed: {e}")))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InferenceError::Connection(format!(
                "OpenRouter STT {status}: {}",
                sanitize_error_body(&text)
            )));
        }

        serde_json::from_str(&text)
            .map_err(|e| InferenceError::Json(format!("OpenRouter STT parse: {e}")))
    }

    /// Generate a video via OpenRouter's async video generation API.
    ///
    /// Uses `/v1/videos` (submit + poll). Submits the prompt, receives a job
    /// ID, then polls until the video is ready. Returns the video URL.
    async fn generate_video(
        &self,
        prompt: &str,
        duration: Option<f32>,
        model: &str,
    ) -> Result<Value, InferenceError> {
        let submit_url = format!("{}/v1/videos", self.base_url);
        let mut body = serde_json::json!({
            "model": model,
            "prompt": prompt,
        });
        if let Some(dur) = duration {
            body["duration"] = serde_json::json!(dur);
        }

        let resp = self
            .client
            .post(&submit_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                InferenceError::Connection(format!("OpenRouter video submit failed: {e}"))
            })?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InferenceError::Connection(format!(
                "OpenRouter video submit {status}: {}",
                sanitize_error_body(&text)
            )));
        }

        let json: Value = serde_json::from_str(&text)
            .map_err(|e| InferenceError::Json(format!("OpenRouter video submit parse: {e}")))?;

        let job_id = json["id"].as_str().ok_or_else(|| {
            InferenceError::Connection("OpenRouter: no video job id in response".into())
        })?;

        self.poll_video(job_id, model).await
    }

    /// Animate a still image into a video via OpenRouter's video API.
    async fn image_to_video(
        &self,
        image_url: &str,
        prompt: Option<&str>,
        duration: Option<f32>,
        model: &str,
    ) -> Result<Value, InferenceError> {
        let submit_url = format!("{}/v1/videos", self.base_url);
        let mut body = serde_json::json!({
            "model": model,
            "image_url": image_url,
        });
        if let Some(p) = prompt {
            body["prompt"] = Value::String(p.to_string());
        }
        if let Some(dur) = duration {
            body["duration"] = serde_json::json!(dur);
        }

        let resp = self
            .client
            .post(&submit_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                InferenceError::Connection(format!("OpenRouter video submit failed: {e}"))
            })?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InferenceError::Connection(format!(
                "OpenRouter video submit {status}: {}",
                sanitize_error_body(&text)
            )));
        }

        let json: Value = serde_json::from_str(&text)
            .map_err(|e| InferenceError::Json(format!("OpenRouter video submit parse: {e}")))?;

        let job_id = json["id"].as_str().ok_or_else(|| {
            InferenceError::Connection("OpenRouter: no video job id in response".into())
        })?;

        self.poll_video(job_id, model).await
    }

    /// Poll a video generation job until completion.
    async fn poll_video(&self, job_id: &str, model: &str) -> Result<Value, InferenceError> {
        let poll_url = format!("{}/v1/videos/{}", self.base_url, job_id);
        for _ in 0..OPENROUTER_VIDEO_MAX_POLLS {
            let resp = self
                .client
                .get(&poll_url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await
                .map_err(|e| {
                    InferenceError::Connection(format!("OpenRouter video poll failed: {e}"))
                })?;

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(InferenceError::Connection(format!(
                    "OpenRouter video poll {status}: {}",
                    sanitize_error_body(&text)
                )));
            }

            let json: Value = resp
                .json()
                .await
                .map_err(|e| InferenceError::Json(format!("OpenRouter video poll parse: {e}")))?;

            if let Some(state) = json["status"].as_str() {
                match state {
                    "completed" | "succeeded" => {
                        let video_url = json["url"].as_str().unwrap_or("");
                        return Ok(serde_json::json!({
                            "url": video_url,
                            "model": model,
                            "status": "completed",
                        }));
                    }
                    "failed" | "error" => {
                        let error = json["error"].as_str().unwrap_or("unknown error");
                        return Err(InferenceError::Connection(format!(
                            "OpenRouter video generation failed: {error}"
                        )));
                    }
                    _ => {} // pending, processing — keep polling
                }
            }

            tokio::time::sleep(OPENROUTER_VIDEO_POLL_INTERVAL).await;
        }

        Err(InferenceError::Connection(format!(
            "OpenRouter video generation timed out ({}s max)",
            OPENROUTER_VIDEO_MAX_POLLS as u64 * OPENROUTER_VIDEO_POLL_INTERVAL.as_secs()
        )))
    }
}

impl MediaProvider for OpenRouterMediaProvider {
    fn id(&self) -> &'static str {
        "openrouter"
    }

    fn supports(&self, op: MediaOp) -> bool {
        matches!(
            op,
            MediaOp::GenerateImage
                | MediaOp::GenerateSpeech
                | MediaOp::Transcribe
                | MediaOp::GenerateVideo
                | MediaOp::ImageToVideo
        )
    }

    fn execute<'a>(
        &'a self,
        op: MediaOp,
        params: &'a MediaGenerateParams,
    ) -> Pin<Box<dyn Future<Output = Result<Value, InferenceError>> + Send + 'a>> {
        Box::pin(async move {
            match op {
                MediaOp::GenerateImage => {
                    let prompt = params.prompt.clone().unwrap_or_default();
                    let model = crate::model_constants::resolve(
                        "HKASK_MEDIA_IMAGE_GEN_MODEL",
                        "openai/dall-e-3",
                    );
                    let model = strip_prefix(&model, "OpenRouter/");
                    self.generate_image(&prompt, params.size.as_deref(), params.count, &model)
                        .await
                }
                MediaOp::GenerateSpeech => {
                    let text = params.text.clone().unwrap_or_default();
                    let voice = params.voice.clone().unwrap_or_else(|| "alloy".to_string());
                    let model = crate::model_constants::resolve(
                        "HKASK_MEDIA_TTS_MODEL",
                        "openai/gpt-4o-mini-tts",
                    );
                    let model = strip_prefix(&model, "OpenRouter/");
                    self.generate_speech(&text, &voice, &model).await
                }
                MediaOp::Transcribe => {
                    let audio_url = params.audio_url.clone().unwrap_or_default();
                    let model = crate::model_constants::resolve(
                        "HKASK_MEDIA_STT_MODEL",
                        "openai/whisper-large-v3",
                    );
                    let model = strip_prefix(&model, "OpenRouter/");
                    self.transcribe(&audio_url, params.language.as_deref(), &model)
                        .await
                }
                MediaOp::GenerateVideo => {
                    let prompt = params.prompt.clone().unwrap_or_default();
                    let model = crate::model_constants::resolve(
                        "HKASK_MEDIA_VIDEO_MODEL",
                        "google/gemini-2.5-flash-video",
                    );
                    let model = strip_prefix(&model, "OpenRouter/");
                    self.generate_video(&prompt, params.duration, &model).await
                }
                MediaOp::ImageToVideo => {
                    let image_url = params.image_url.clone().unwrap_or_default();
                    let model = crate::model_constants::resolve(
                        "HKASK_MEDIA_VIDEO_MODEL",
                        "google/gemini-2.5-flash-video",
                    );
                    let model = strip_prefix(&model, "OpenRouter/");
                    self.image_to_video(
                        &image_url,
                        params.prompt.as_deref(),
                        params.duration,
                        &model,
                    )
                    .await
                }
                other => Err(InferenceError::Connection(format!(
                    "openrouter does not support media op: {}",
                    other.as_str()
                ))),
            }
        })
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Strip a provider prefix (e.g. `DeepInfra/`, `OpenRouter/`) from a model
/// name so the bare model id is passed to the provider's API.
fn strip_prefix(model: &str, prefix: &str) -> String {
    if let Some(stripped) = model.strip_prefix(prefix) {
        stripped.to_string()
    } else {
        model.to_string()
    }
}

/// Detect audio format from a URL's file extension.
///
/// Returns the format string expected by the STT APIs (`mp3`, `wav`, etc.).
/// Defaults to `mp3` when the extension is unrecognized.
fn detect_audio_format(url: &str) -> String {
    let lower = url.to_lowercase();
    if lower.ends_with(".wav") {
        "wav".to_string()
    } else if lower.ends_with(".mp3") {
        "mp3".to_string()
    } else if lower.ends_with(".flac") {
        "flac".to_string()
    } else if lower.ends_with(".m4a") {
        "m4a".to_string()
    } else if lower.ends_with(".ogg") {
        "ogg".to_string()
    } else if lower.ends_with(".webm") {
        "webm".to_string()
    } else if lower.ends_with(".aac") {
        "aac".to_string()
    } else {
        "mp3".to_string()
    }
}

/// Extract an image URL from a chat completion content string.
///
/// Models return image URLs in various formats:
/// - Raw URL: `https://example.com/image.png`
/// - Markdown: `![image](https://example.com/image.png)`
/// - JSON: `{"url": "https://example.com/image.png"}`
fn extract_image_url(content: &str) -> Option<Value> {
    let trimmed = content.trim();

    // Try markdown image: ![alt](url)
    if let Some(start) = trimmed.find("](") {
        if let Some(end) = trimmed[start + 2..].find(')') {
            let url = &trimmed[start + 2..start + 2 + end];
            if url.starts_with("http") {
                return Some(serde_json::json!({"url": url}));
            }
        }
    }

    // Try raw URL
    if trimmed.starts_with("http")
        && (trimmed.ends_with(".png")
            || trimmed.ends_with(".jpg")
            || trimmed.ends_with(".jpeg")
            || trimmed.ends_with(".webp"))
    {
        return Some(serde_json::json!({"url": trimmed}));
    }

    // Try JSON with url field
    if let Ok(json) = serde_json::from_str::<Value>(trimmed) {
        if let Some(url) = json.get("url").and_then(|u| u.as_str()) {
            return Some(serde_json::json!({"url": url}));
        }
    }

    // Try finding any http URL in the content
    let mut in_url = false;
    let mut url_start = 0;
    let chars: Vec<char> = trimmed.chars().collect();
    for (i, ch) in chars.iter().enumerate() {
        if !in_url && *ch == 'h' && trimmed[i..].starts_with("http") {
            in_url = true;
            url_start = i;
        }
        if in_url && (*ch == ' ' || *ch == '\n' || *ch == '"') {
            let url = &trimmed[url_start..i];
            if url.contains("://")
                && (url.contains(".png")
                    || url.contains(".jpg")
                    || url.contains(".jpeg")
                    || url.contains(".webp")
                    || url.contains("/image"))
            {
                return Some(serde_json::json!({"url": url}));
            }
            in_url = false;
        }
    }
    if in_url {
        let url = &trimmed[url_start..];
        if url.contains("://") {
            return Some(serde_json::json!({"url": url}));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_prefix_removes_provider() {
        assert_eq!(
            strip_prefix("DeepInfra/hexgrad/Kokoro-82M", "DeepInfra/"),
            "hexgrad/Kokoro-82M"
        );
        assert_eq!(
            strip_prefix("hexgrad/Kokoro-82M", "DeepInfra/"),
            "hexgrad/Kokoro-82M"
        );
    }

    #[test]
    fn extract_image_url_from_markdown() {
        let result = extract_image_url("![generated](https://example.com/image.png)");
        assert_eq!(
            result,
            Some(serde_json::json!({"url": "https://example.com/image.png"}))
        );
    }

    #[test]
    fn extract_image_url_from_raw_url() {
        let result = extract_image_url("https://example.com/image.png");
        assert_eq!(
            result,
            Some(serde_json::json!({"url": "https://example.com/image.png"}))
        );
    }

    #[test]
    fn extract_image_url_from_json() {
        let result = extract_image_url(r#"{"url": "https://example.com/image.png"}"#);
        assert_eq!(
            result,
            Some(serde_json::json!({"url": "https://example.com/image.png"}))
        );
    }

    #[test]
    fn extract_image_url_returns_none_for_plain_text() {
        let result = extract_image_url("I cannot generate images.");
        assert!(result.is_none());
    }

    #[test]
    fn detect_audio_format_from_extension() {
        assert_eq!(detect_audio_format("https://example.com/audio.wav"), "wav");
        assert_eq!(detect_audio_format("https://example.com/audio.mp3"), "mp3");
        assert_eq!(
            detect_audio_format("https://example.com/audio.flac"),
            "flac"
        );
        assert_eq!(detect_audio_format("https://example.com/audio.m4a"), "m4a");
        assert_eq!(detect_audio_format("https://example.com/audio.ogg"), "ogg");
        assert_eq!(
            detect_audio_format("https://example.com/audio.webm"),
            "webm"
        );
        assert_eq!(detect_audio_format("https://example.com/audio.aac"), "aac");
    }

    #[test]
    fn detect_audio_format_defaults_to_mp3() {
        assert_eq!(detect_audio_format("https://example.com/audio"), "mp3");
        assert_eq!(detect_audio_format("https://example.com/audio.xyz"), "mp3");
    }
}
