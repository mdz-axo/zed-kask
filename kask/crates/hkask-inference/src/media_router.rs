//! Media router — fal.ai/DeepInfra media generation only.
//!
//! In zed-kask, chat inference routes through the zed IPC bridge
//! (`InferenceIpcClient` → `LanguageModelRegistry`). This router handles only
//! media generation (image/video/speech/transcription) via fal.ai and DeepInfra
//! backends — capabilities not covered by zed's `LanguageModel` abstraction.
//!
//! The `InferencePort` impl returns clear errors for chat/vision/embed/list_models
//! — those are the IPC bridge's responsibility. The `InferenceIpcServer` holds a
//! `MediaRouter` as its `media_router` and dispatches `media_generate` requests
//! to it.

use crate::config::InferenceConfig;
use crate::deepinfra_backend::DeepInfraBackend;
use crate::fal_backend::FalBackend;
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, EmbedFuture, InferenceError, InferencePort, InferenceResult,
    InferenceStreamChunk, MediaFuture, MediaGenerateParams, ModelEntry,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Media generation router — fal.ai + DeepInfra backends only.
///
/// Constructed from `InferenceConfig::from_env()`. Backends are created lazily:
/// a backend is only `Some` if its API key is present. Media methods that need
/// a missing backend return a clear `Connection` error.
pub struct MediaRouter {
    fal: Option<FalBackend>,
    deepinfra: Option<DeepInfraBackend>,
}

impl MediaRouter {
    /// Build the media router from an `InferenceConfig`.
    ///
    /// Constructs backends lazily — a backend is only created if its
    /// configuration is valid (non-empty API key). A `None` backend is
    /// rejected by the media method that needs it with a clear error.
    #[must_use]
    pub fn new(config: InferenceConfig) -> Self {
        let shared_client = config
            .build_client()
            .map(Arc::new)
            .map_err(|e| tracing::warn!(target: "reg.inference", "HTTP client build failed: {}", e))
            .ok();

        let fal = shared_client
            .as_ref()
            .and_then(|c| FalBackend::new(&config, Arc::clone(c)).ok());
        let deepinfra = shared_client
            .as_ref()
            .and_then(|c| DeepInfraBackend::new(&config, Arc::clone(c)).ok());

        if fal.is_none() {
            tracing::warn!(target: "reg.inference", "fal.ai backend unavailable (no API key) — media generation disabled");
        }
        if deepinfra.is_none() {
            tracing::warn!(target: "reg.inference", "DeepInfra backend unavailable (no API key) — speech/transcription fallback disabled");
        }

        Self { fal, deepinfra }
    }

    /// Generate an image from a text prompt via fal.ai.
    #[must_use = "result must be used"]
    pub async fn generate_image(
        &self,
        prompt: &str,
        image_size: Option<&str>,
        num_images: Option<u32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("fal.ai backend unavailable for image generation".into())
        })?;
        backend.generate_image(prompt, image_size, num_images).await
    }

    /// Transform an existing image with a prompt (image-to-image) via fal.ai.
    #[must_use = "result must be used"]
    pub async fn image_to_image(
        &self,
        image_url: &str,
        prompt: &str,
        strength: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("fal.ai backend unavailable for image-to-image".into())
        })?;
        backend.image_to_image(image_url, prompt, strength).await
    }

    /// Remove background from an image. DeepInfra first (cheapest), fal.ai fallback.
    #[must_use = "result must be used"]
    pub async fn remove_background(
        &self,
        image_url: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        if let Some(ref di) = self.deepinfra {
            match di.remove_background(image_url).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(
                        target: "reg.inference",
                        error = %e,
                        "DeepInfra background removal failed, falling back to fal.ai"
                    );
                }
            }
        }
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("No backend available for background removal".into())
        })?;
        backend.remove_background(image_url).await
    }

    /// Upscale an image via fal.ai SeedVR2.
    #[must_use = "result must be used"]
    pub async fn upscale(
        &self,
        image_url: &str,
        scale: Option<u32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("fal.ai backend unavailable for upscaling".into())
        })?;
        backend.upscale(image_url, scale).await
    }

    /// Generate a video from a text prompt via fal.ai.
    #[must_use = "result must be used"]
    pub async fn generate_video(
        &self,
        prompt: &str,
        duration: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("fal.ai backend unavailable for video generation".into())
        })?;
        backend.generate_video(prompt, duration).await
    }

    /// Animate a still image into a video via fal.ai Seedance.
    #[must_use = "result must be used"]
    pub async fn image_to_video(
        &self,
        image_url: &str,
        prompt: Option<&str>,
        duration: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("fal.ai backend unavailable for image-to-video".into())
        })?;
        backend.image_to_video(image_url, prompt, duration).await
    }

    /// Generate speech from text. DeepInfra first, fal.ai fallback.
    #[must_use = "result must be used"]
    pub async fn generate_speech(
        &self,
        text: &str,
        voice: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        if let Some(ref di) = self.deepinfra {
            match di.generate_speech(text, voice, None).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(
                        target: "reg.inference",
                        error = %e,
                        "DeepInfra TTS failed, falling back to fal.ai"
                    );
                }
            }
        }
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("No backend available for speech generation".into())
        })?;
        backend.generate_speech(text, voice).await
    }

    /// Segment/extract a specific object from an image via fal.ai Florence-2.
    #[must_use = "result must be used"]
    pub async fn segment_object(
        &self,
        image_url: &str,
        object_description: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("fal.ai backend required for object segmentation".into())
        })?;
        backend.segment_object(image_url, object_description).await
    }

    /// Transcribe speech audio to text. DeepInfra first, fal.ai fallback.
    #[must_use = "result must be used"]
    pub async fn transcribe(
        &self,
        audio_url: &str,
        language: Option<&str>,
    ) -> Result<serde_json::Value, InferenceError> {
        if let Some(ref di) = self.deepinfra {
            match di.transcribe(audio_url, language).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(
                        target: "reg.inference",
                        error = %e,
                        "DeepInfra STT failed, falling back to fal.ai"
                    );
                }
            }
        }
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("No backend available for speech transcription".into())
        })?;
        backend.transcribe(audio_url).await
    }

    /// Execute a multi-step Fal media workflow.
    #[must_use = "result must be used"]
    pub async fn execute_workflow(
        &self,
        workflow: &serde_json::Value,
    ) -> Result<crate::fal_workflow::WorkflowResult, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("fal.ai backend unavailable for workflow execution".into())
        })?;
        backend.execute_workflow(workflow).await
    }
}

/// Error message for chat/vision/embed operations routed to the MediaRouter.
///
/// These operations are the IPC bridge's responsibility — the MediaRouter only
/// handles media generation. When the fallback path constructs a MediaRouter
/// (because the IPC socket is unreachable), chat/vision/embed requests get
/// this error instead of silently routing to a dead keychain namespace.
const BRIDGE_ERROR: &str = "Chat/vision/embed operations are routed through the zed IPC bridge, not the MediaRouter. \
     The IPC bridge is unreachable — ensure HKASK_INFERENCE_SOCKET is set and zed is running.";

impl InferencePort for MediaRouter {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &LLMParameters,
        _tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        Box::pin(async { Err(InferenceError::Generation(BRIDGE_ERROR.to_string())) })
    }

    fn generate_with_model(
        &self,
        _prompt: &str,
        _parameters: &LLMParameters,
        _model_override: Option<&str>,
        _tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        Box::pin(async { Err(InferenceError::Generation(BRIDGE_ERROR.to_string())) })
    }

    fn generate_with_messages(
        &self,
        _messages: &[ChatMessage],
        _parameters: &LLMParameters,
        _model_override: Option<&str>,
        _tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        Box::pin(async { Err(InferenceError::Generation(BRIDGE_ERROR.to_string())) })
    }

    fn generate_stream(
        &self,
        _prompt: &str,
        _parameters: &LLMParameters,
        _tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<
        Box<
            dyn futures_util::Stream<Item = Result<InferenceStreamChunk, InferenceError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(futures_util::stream::once(async {
            Err(InferenceError::Generation(BRIDGE_ERROR.to_string()))
        }))
    }

    fn generate_vision(
        &self,
        _prompt: &str,
        _images: &[String],
        _parameters: &LLMParameters,
        _model_override: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        Box::pin(async { Err(InferenceError::Generation(BRIDGE_ERROR.to_string())) })
    }

    fn embed<'a>(&'a self, _model: &str, _texts: &[String]) -> EmbedFuture<'a> {
        Box::pin(async {
            Err(hkask_types::EmbeddingGenerationError::Connection(
                BRIDGE_ERROR.to_string(),
            ))
        })
    }

    fn list_models<'a>(&'a self) -> Pin<Box<dyn Future<Output = Vec<ModelEntry>> + Send + 'a>> {
        Box::pin(async { Vec::new() })
    }

    fn media_generate<'a>(&'a self, op: &str, params: &MediaGenerateParams) -> MediaFuture<'a> {
        let op = op.to_string();
        let params = params.clone();
        Box::pin(async move {
            match op.as_str() {
                "generate_image" => {
                    let prompt = params.prompt.as_deref().unwrap_or("");
                    self.generate_image(prompt, params.size.as_deref(), params.count)
                        .await
                }
                "image_to_image" => {
                    let image_url = params.image_url.as_deref().unwrap_or("");
                    let prompt = params.prompt.as_deref().unwrap_or("");
                    self.image_to_image(image_url, prompt, params.strength)
                        .await
                }
                "remove_background" => {
                    let image_url = params.image_url.as_deref().unwrap_or("");
                    self.remove_background(image_url).await
                }
                "upscale" => {
                    let image_url = params.image_url.as_deref().unwrap_or("");
                    self.upscale(image_url, params.scale).await
                }
                "generate_video" => {
                    let prompt = params.prompt.as_deref().unwrap_or("");
                    self.generate_video(prompt, params.duration).await
                }
                "image_to_video" => {
                    let image_url = params.image_url.as_deref().unwrap_or("");
                    self.image_to_video(image_url, params.prompt.as_deref(), params.duration)
                        .await
                }
                "generate_speech" => {
                    let text = params.text.as_deref().unwrap_or("");
                    let voice = params.voice.as_deref().unwrap_or("Rachel");
                    self.generate_speech(text, voice).await
                }
                "segment_object" => {
                    let image_url = params.image_url.as_deref().unwrap_or("");
                    let object_description = params.object_description.as_deref().unwrap_or("");
                    self.segment_object(image_url, object_description).await
                }
                "transcribe" => {
                    let audio_url = params.audio_url.as_deref().unwrap_or("");
                    self.transcribe(audio_url, params.language.as_deref()).await
                }
                "execute_workflow" => {
                    let workflow = params.workflow.clone().unwrap_or(serde_json::Value::Null);
                    let result = self.execute_workflow(&workflow).await?;
                    serde_json::to_value(result).map_err(|e| {
                        InferenceError::Json(format!("WorkflowResult serialize failed: {e}"))
                    })
                }
                other => Err(InferenceError::Connection(format!(
                    "unknown media op: {other}"
                ))),
            }
        })
    }
}
