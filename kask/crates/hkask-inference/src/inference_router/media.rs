//! Media generation dispatch — vision, image, video, speech, segmentation, transcription.

use super::InferenceRouter;
use hkask_types::InferenceError;
use hkask_types::template::LLMParameters;

impl InferenceRouter {
    /// Vision/multimodal inference — dispatch to the appropriate backend with base64 images.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated multimodal dispatch
    /// pre:  prompt is non-empty
    /// pre:  images is non-empty
    /// pre:  params is a valid LLMParameters
    /// post: dispatches to provider-resolved backend's generate_vision
    /// post: returns Ok(InferenceResult) on success
    /// post: if provider resolution fails → Err(InferenceError)
    /// post: if backend call fails → Err(InferenceError)
    #[must_use = "result must be used"]
    pub async fn generate_vision(
        &self,
        prompt: &str,
        images: &[String],
        params: &LLMParameters,
        model_override: Option<&str>,
    ) -> Result<hkask_types::InferenceResult, InferenceError> {
        let model_name = model_override
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.config.default_model.clone());
        // Vision dispatch uses parse_provider + the vision_backend match-fn (not
        // `resolve_chat`, which gates on chat availability — RunPod is vision-only).
        let (provider, model) = self.parse_provider(&model_name)?;
        let backend = self.vision_backend(provider).ok_or_else(|| {
            InferenceError::Connection(format!(
                "Provider {} is not available for vision (check configuration)",
                provider.as_str()
            ))
        })?;
        let model = model.to_string();
        let prompt = prompt.to_string();
        let params = params.clone();
        let images = images.to_vec();
        backend
            .generate_vision(&model, &prompt, &images, &params)
            .await
    }

    // ── Media generation dispatch ──────────────────────────────────────────

    /// Generate an image from a text prompt.
    /// Routes to fal.ai FLUX Schnell (default) or DeepInfra FLUX 2 Klein.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated image generation dispatch
    /// pre:  prompt is a non-empty text description
    /// post: returns Ok(serde_json::Value) with generated image data
    /// post: if fal backend unavailable → Err(InferenceError::Connection)
    #[must_use = "result must be used"]
    pub async fn generate_image(
        &self,
        prompt: &str,
        image_size: Option<&str>,
        num_images: Option<u32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection(
                "fal.ai backend unavailable for image generation".to_string(),
            )
        })?;
        backend.generate_image(prompt, image_size, num_images).await
    }

    /// Transform an existing image with a prompt (image-to-image).
    /// Routes to fal.ai Flux dev img2img (default) or DeepInfra Qwen Image Edit.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated image editing dispatch
    /// pre:  image_url is a valid, accessible image URL
    /// pre:  prompt is a non-empty transformation instruction
    /// post: returns Ok(serde_json::Value) with transformed image data
    /// post: if fal backend unavailable → Err(InferenceError::Connection)
    #[must_use = "result must be used"]
    pub async fn image_to_image(
        &self,
        image_url: &str,
        prompt: &str,
        strength: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("fal.ai backend unavailable for image-to-image".to_string())
        })?;
        backend.image_to_image(image_url, prompt, strength).await
    }

    /// Remove background from an image.
    /// Routes to DeepInfra Bria RMBG 2.0 (cheapest) with fal.ai Birefnet fallback.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated background removal dispatch
    /// pre:  image_url is a valid, accessible image URL
    /// post: tries DeepInfra first, falls back to fal.ai on failure
    /// post: returns Ok(serde_json::Value) with background-removed image data
    /// post: if no backend available → Err(InferenceError::Connection)
    #[must_use = "result must be used"]
    pub async fn remove_background(
        &self,
        image_url: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        // Try DeepInfra first (cheapest at $0.018/image)
        if let Some(ref di) = self.deepinfra {
            match di.remove_background(image_url).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(target: "reg.inference", error = %e, "DeepInfra background removal failed, falling back to fal.ai");
                }
            }
        }
        // Fallback to fal.ai Birefnet
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("No backend available for background removal".to_string())
        })?;
        backend.remove_background(image_url).await
    }

    /// Upscale an image.
    /// Routes to fal.ai SeedVR2 (queue).
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated upscaling dispatch
    /// pre:  image_url is a valid, accessible image URL
    /// post: returns Ok(serde_json::Value) with upscaled image data
    /// post: if fal backend unavailable → Err(InferenceError::Connection)
    #[must_use = "result must be used"]
    pub async fn upscale(
        &self,
        image_url: &str,
        scale: Option<u32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("fal.ai backend unavailable for upscaling".to_string())
        })?;
        backend.upscale(image_url, scale).await
    }

    /// Generate a video from a text prompt.
    /// Routes to fal.ai MiniMax video-01-live (queue).
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated video generation dispatch
    /// pre:  prompt is a non-empty text description
    /// post: returns Ok(serde_json::Value) with generated video data
    /// post: if fal backend unavailable → Err(InferenceError::Connection)
    #[must_use = "result must be used"]
    pub async fn generate_video(
        &self,
        prompt: &str,
        duration: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection(
                "fal.ai backend unavailable for video generation".to_string(),
            )
        })?;
        backend.generate_video(prompt, duration).await
    }

    /// Animate a still image into a video.
    /// Routes to fal.ai Seedance 2.0 image-to-video (queue).
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated video generation dispatch
    /// pre:  image_url is a valid, accessible image URL
    /// post: returns Ok(serde_json::Value) with generated video data
    /// post: if fal backend unavailable → Err(InferenceError::Connection)
    #[must_use = "result must be used"]
    pub async fn image_to_video(
        &self,
        image_url: &str,
        prompt: Option<&str>,
        duration: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("fal.ai backend unavailable for image-to-video".to_string())
        })?;
        backend.image_to_video(image_url, prompt, duration).await
    }

    /// Generate speech from text with a voice preset.
    /// Routes to DeepInfra ElevenLabs-compatible API (default) with fal.ai fallback.
    /// Default voice: "Rachel" (ElevenLabs default, available on both providers).
    /// Default model on DeepInfra: hexgrad/Kokoro-82M.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated speech synthesis dispatch
    /// pre:  text is non-empty
    /// pre:  voice is a valid voice preset name
    /// post: tries DeepInfra first, falls back to fal.ai on failure
    /// post: returns Ok(serde_json::Value) with generated speech audio data
    /// post: if no backend available → Err(InferenceError::Connection)
    #[must_use = "result must be used"]
    pub async fn generate_speech(
        &self,
        text: &str,
        voice: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        // Try DeepInfra first (ElevenLabs-compatible API)
        if let Some(ref di) = self.deepinfra {
            match di.generate_speech(text, voice, None).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(target: "reg.inference", error = %e, "DeepInfra TTS failed, falling back to fal.ai");
                }
            }
        }
        // Fallback to fal.ai ElevenLabs
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("No backend available for speech generation".to_string())
        })?;
        backend.generate_speech(text, voice).await
    }

    /// Segment/extract a specific object from an image.
    /// Routes to fal.ai Florence-2 segmentation.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated segmentation dispatch
    /// pre:  image_url is a valid, accessible image URL
    /// pre:  object_description is a non-empty description of the object to segment
    /// post: returns Ok(serde_json::Value) with segmented object data
    /// post: if fal backend unavailable → Err(InferenceError::Connection)
    #[must_use = "result must be used"]
    pub async fn segment_object(
        &self,
        image_url: &str,
        object_description: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection(
                "fal.ai backend required for object segmentation".to_string(),
            )
        })?;
        backend.segment_object(image_url, object_description).await
    }

    /// Transcribe speech audio to text.
    /// Routes to DeepInfra Whisper (default) with fal.ai Whisper fallback.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated transcription dispatch
    /// pre:  audio_url is a valid, accessible audio file URL
    /// post: tries DeepInfra first, falls back to fal.ai on failure
    /// post: returns Ok(serde_json::Value) with transcription data
    /// post: if no backend available → Err(InferenceError::Connection)
    #[must_use = "result must be used"]
    pub async fn transcribe(
        &self,
        audio_url: &str,
        language: Option<&str>,
    ) -> Result<serde_json::Value, InferenceError> {
        // Try DeepInfra first
        if let Some(ref di) = self.deepinfra {
            match di.transcribe(audio_url, language).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(target: "reg.inference", error = %e, "DeepInfra STT failed, falling back to fal.ai");
                }
            }
        }
        // Fallback to fal.ai Whisper
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection("No backend available for speech transcription".to_string())
        })?;
        backend.transcribe(audio_url).await
    }

    // ── Workflow dispatch ──────────────────────────────────────────────

    /// Execute a multi-step Fal media workflow through the inference router.
    ///
    /// Delegates to the Fal backend's DAG engine: parses the workflow JSON,
    /// topologically sorts nodes, resolves $references between nodes, and
    /// executes each model call sequentially.
    ///
    /// expect: "I can execute a multi-step Fal media workflow through the inference router"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated workflow dispatch
    /// \[P4\] Constraining: Clear Boundaries — workflow gated on Fal backend availability
    /// pre:  workflow is a valid JSON object with input, run, and display nodes
    /// post: returns Ok(WorkflowResult) with output URLs, fields, and node results
    /// post: if fal backend is unavailable → Err(InferenceError::Connection)
    /// post: if workflow is malformed → Err(InferenceError::Json) or Generation
    #[must_use = "result must be used"]
    pub async fn execute_workflow(
        &self,
        workflow: &serde_json::Value,
    ) -> Result<crate::fal_workflow::WorkflowResult, InferenceError> {
        let backend = self.fal.as_ref().ok_or_else(|| {
            InferenceError::Connection(
                "fal.ai backend unavailable for workflow execution".to_string(),
            )
        })?;
        backend.execute_workflow(workflow).await
    }
}
