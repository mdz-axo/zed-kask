//! Media router — pluggable multi-provider media generation.
//!
//! In zed-kask, chat inference routes through the zed IPC bridge
//! (`InferenceIpcClient` → `LanguageModelRegistry`). This router handles only
//! media generation (image/video/speech/transcription) via the
//! [`ProviderRegistry`] — capabilities not covered by zed's `LanguageModel`
//! (chat-completions-only) abstraction.
//!
//! The `InferencePort` impl returns clear errors for chat/vision/embed/list_models
//! — those are the IPC bridge's responsibility. The `InferenceIpcServer` holds a
//! `MediaRouter` as its `media_router` and dispatches `media_generate` requests
//! to it.
//!
//! Media is routed to zed via the IPC bridge, but terminates here (the hKask
//! `MediaRouter`) rather than zed's `LanguageModelRegistry`, because media
//! generation uses non-chat APIs (fal.ai queue/run + app-id routing; DeepInfra
//! inference/tts/transcription with binary returns) that `LanguageModel`
//! cannot represent. If zed later adds a media trait to its registry, this
//! terminal can delegate to it instead — until then the providers live here.
//! Adding a provider = implement [`crate::provider::MediaProvider`] + register
//! in [`MediaRouter::new`]; no dispatch edits.

use crate::atlascloud_backend::AtlasCloudBackend;
use crate::config::InferenceConfig;
use crate::deepinfra_backend::DeepInfraBackend;
use crate::provider::{MediaOp, MediaProvider, ProviderRegistry};
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, EmbedFuture, InferenceError, InferencePort, InferenceResult,
    InferenceStreamChunk, MediaFuture, MediaGenerateParams, ModelEntry,
};
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;

/// Media generation router — a pluggable provider registry.
///
/// Constructed from `InferenceConfig::from_env()`. Providers are created
/// lazily: a provider is only registered if its API key is present. Media
/// methods that find no supporting provider return a clear `Connection`
/// error. The registry order encodes the preference policy: DeepInfra first
/// (cheapest for background removal / TTS / STT, with fal.ai fallback),
/// fal.ai for everything else.
pub struct MediaRouter {
    pub(crate) registry: ProviderRegistry,
}

impl MediaRouter {
    /// Build the media router from an `InferenceConfig`.
    ///
    /// Constructs providers lazily — a provider is only created if its
    /// configuration is valid (non-empty API key). Providers that fail to
    /// construct are not registered and emit a `reg.inference` warn. The
    /// registry order is DeepInfra-first so the runtime fallback preserves
    /// the prior DeepInfra-first / fal-fallback policy for the three shared
    /// ops (remove_background, generate_speech, transcribe).
    ///
    /// expect: "The system creates provider membranes requiring valid API keys"
    /// \[P4\] Motivating: Clear Boundaries — providers registered only with valid keys
    /// pre:  none (reads config)
    /// post: returns MediaRouter whose registry holds all constructible providers
    #[must_use]
    pub fn new(config: InferenceConfig) -> Self {
        let shared_client = config
            .build_client()
            .map(Arc::new)
            .map_err(|e| tracing::warn!(target: "reg.inference", "HTTP client build failed: {}", e))
            .ok();

        let mut providers: Vec<Arc<dyn MediaProvider>> = Vec::new();

        if let Some(client) = &shared_client {
            // DeepInfra first: preferred for remove_background / speech /
            // transcribe (cheapest). Registered before fal.ai so the registry
            // tries it first and falls back to fal.ai on runtime error.
            match DeepInfraBackend::new(&config, Arc::clone(client)) {
                Ok(di) => providers.push(Arc::new(di)),
                Err(_) => tracing::warn!(
                    target: "reg.inference",
                    "DeepInfra backend unavailable (no API key) — \
                     speech/transcription fallback disabled"
                ),
            }
            match AtlasCloudBackend::new(&config, Arc::clone(client)) {
                Ok(ac) => providers.push(Arc::new(ac)),
                Err(_) => tracing::warn!(
                    target: "reg.inference",
                    "AtlasCloud backend unavailable (no API key) — image/video generation fallback disabled"
                ),
            }
        }

        if providers.is_empty() {
            tracing::warn!(
                target: "reg.inference",
                "no media providers configured — all media generation will fail \
                 (set DEEPINFRA_API_KEY and/or ATLASCLOUD_API_KEY)"
            );
        }

        Self {
            registry: ProviderRegistry::new(providers),
        }
    }

    /// Generate an image from a text prompt.
    #[must_use = "result must be used"]
    pub async fn generate_image(
        &self,
        prompt: &str,
        image_size: Option<&str>,
        num_images: Option<u32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let params = MediaGenerateParams {
            prompt: Some(prompt.to_string()),
            size: image_size.map(|s| s.to_string()),
            count: num_images,
            ..Default::default()
        };
        self.registry.execute(MediaOp::GenerateImage, &params).await
    }

    /// Transform an existing image with a prompt (image-to-image).
    #[must_use = "result must be used"]
    pub async fn image_to_image(
        &self,
        image_url: &str,
        prompt: &str,
        strength: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let params = MediaGenerateParams {
            image_url: Some(image_url.to_string()),
            prompt: Some(prompt.to_string()),
            strength,
            ..Default::default()
        };
        self.registry.execute(MediaOp::ImageToImage, &params).await
    }

    /// Remove background from an image. DeepInfra first (cheapest), fal.ai fallback.
    #[must_use = "result must be used"]
    pub async fn remove_background(
        &self,
        image_url: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        let params = MediaGenerateParams {
            image_url: Some(image_url.to_string()),
            ..Default::default()
        };
        self.registry
            .execute(MediaOp::RemoveBackground, &params)
            .await
    }

    /// Upscale an image.
    #[must_use = "result must be used"]
    pub async fn upscale(
        &self,
        image_url: &str,
        scale: Option<u32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let params = MediaGenerateParams {
            image_url: Some(image_url.to_string()),
            scale,
            ..Default::default()
        };
        self.registry.execute(MediaOp::Upscale, &params).await
    }

    /// Generate a video from a text prompt.
    #[must_use = "result must be used"]
    pub async fn generate_video(
        &self,
        prompt: &str,
        duration: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let params = MediaGenerateParams {
            prompt: Some(prompt.to_string()),
            duration,
            ..Default::default()
        };
        self.registry.execute(MediaOp::GenerateVideo, &params).await
    }

    /// Animate a still image into a video.
    #[must_use = "result must be used"]
    pub async fn image_to_video(
        &self,
        image_url: &str,
        prompt: Option<&str>,
        duration: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let params = MediaGenerateParams {
            image_url: Some(image_url.to_string()),
            prompt: prompt.map(|p| p.to_string()),
            duration,
            ..Default::default()
        };
        self.registry.execute(MediaOp::ImageToVideo, &params).await
    }

    /// Generate speech from text. DeepInfra first, fal.ai fallback.
    #[must_use = "result must be used"]
    pub async fn generate_speech(
        &self,
        text: &str,
        voice: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        let params = MediaGenerateParams {
            text: Some(text.to_string()),
            voice: Some(voice.to_string()),
            ..Default::default()
        };
        self.registry
            .execute(MediaOp::GenerateSpeech, &params)
            .await
    }

    /// Transcribe speech audio to text. DeepInfra first, AtlasCloud fallback.
    #[must_use = "result must be used"]
    pub async fn transcribe(
        &self,
        audio_url: &str,
        language: Option<&str>,
    ) -> Result<serde_json::Value, InferenceError> {
        let params = MediaGenerateParams {
            audio_url: Some(audio_url.to_string()),
            language: language.map(|l| l.to_string()),
            ..Default::default()
        };
        self.registry.execute(MediaOp::Transcribe, &params).await
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
            let media_op = MediaOp::from_str(&op)?;
            self.registry.execute(media_op, &params).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InferenceConfig;

    /// No API keys → empty registry → every op reports "no provider configured".
    #[test]
    fn media_router_without_keys_has_no_providers() {
        let router = MediaRouter::new(InferenceConfig::default());
        assert!(router.registry.is_empty(), "no API keys → empty registry");
        assert!(!router.registry.supports(MediaOp::GenerateImage));
        assert!(!router.registry.supports(MediaOp::RemoveBackground));
    }

    /// An AtlasCloud key alone registers AtlasCloud, which supports all
    #[test]
    fn media_router_with_atlascloud_key_supports_all_ops() {
        let config = InferenceConfig {
            atlascloud_api_key: "test-key".into(),
            ..Default::default()
        };
        let router = MediaRouter::new(config);
        assert!(!router.registry.is_empty());
        assert!(router.registry.supports(MediaOp::GenerateImage));
        assert!(router.registry.supports(MediaOp::GenerateVideo));
        assert!(router.registry.supports(MediaOp::Upscale));
    }

    /// A DeepInfra key alone registers DeepInfra, which supports only the
    /// three ops it's preferred for — image generation is NOT available
    /// (DeepInfra's generate_image method is intentionally not advertised,
    /// preserving the AtlasCloud-only image dispatch).
    #[test]
    fn media_router_with_deepinfra_key_supports_only_three_ops() {
        let config = InferenceConfig {
            deepinfra_api_key: "di-key".into(),
            ..Default::default()
        };
        let router = MediaRouter::new(config);
        assert!(router.registry.supports(MediaOp::RemoveBackground));
        assert!(router.registry.supports(MediaOp::GenerateSpeech));
        assert!(router.registry.supports(MediaOp::Transcribe));
        assert!(
            !router.registry.supports(MediaOp::GenerateImage),
            "image generation must require AtlasCloud — DeepInfra's generate_image is not advertised"
        );
    }

    /// `media_generate` with an unknown op string returns a clear error
    /// (never panics, never silently succeeds).
    #[tokio::test]
    async fn media_generate_unknown_op_errors() {
        let router = MediaRouter::new(InferenceConfig::default());
        let err = router
            .media_generate("nonsense_op", &MediaGenerateParams::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown media op"), "got: {err}");
    }

    /// `media_generate` with no provider configured returns a clear
    /// "no provider configured" error rather than a generic backend error.
    #[tokio::test]
    async fn media_generate_no_provider_errors_clearly() {
        let router = MediaRouter::new(InferenceConfig::default());
        let err = router
            .media_generate("generate_image", &MediaGenerateParams::default())
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("no provider configured"),
            "got: {err}"
        );
    }
}
