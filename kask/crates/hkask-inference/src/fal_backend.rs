//! fal.ai backend — generative media (image/video/speech/transcription/workflow).
//!
//! fal.ai is NOT an OpenAI-compatible chat endpoint (`/v1/chat/completions`
//! returns 404; `/v1/models` uses `Authorization: Key` and returns a media
//! catalog). This backend is registered in `MediaRouter` as a `MediaProvider`
//! for media-generation ops only; chat/vision/embed/list_models are routed
//! through the zed IPC bridge by the `MediaRouter` `InferencePort` impl.
//! Auth: `Authorization: Key {FALAI_API_KEY}`.

use crate::config::InferenceConfig;
use crate::fal_workflow::{ExecutionMode, WorkflowResult};
use crate::openai_compat::sanitize_error_body;
use crate::provider::{MediaOp, MediaProvider};
use crate::workflow::NodeExecutor;
use hkask_types::{InferenceError, MediaGenerateParams};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// fal.ai backend for generative media (image/video/speech/transcription/workflow).
#[derive(Debug)]
pub struct FalBackend {
    media_base_url: String,
    queue_base_url: String,
    api_key: String,
    client: Arc<reqwest::Client>,
}

impl FalBackend {
    /// Create a new Fal backend from inference config.
    ///
    /// Returns an error if `fal_api_key` is empty.
    ///
    /// expect: "The system creates provider membranes requiring valid API keys"
    /// \[P4\] Motivating: Clear Boundaries — fal.ai provider membrane requires valid API key
    /// pre:  config.fal_api_key is set
    /// post: returns FalBackend with configured HTTP client
    pub fn new(
        config: &InferenceConfig,
        client: Arc<reqwest::Client>,
    ) -> Result<Self, InferenceError> {
        if config.fal_api_key.is_empty() {
            return Err(InferenceError::Connection(
                "fal.ai API key not configured (set FALAI_API_KEY)".into(),
            ));
        }
        Ok(Self {
            media_base_url: config.fal_media_base_url.clone(),
            queue_base_url: config.fal_queue_base_url.clone(),
            api_key: config.fal_api_key.clone(),
            client,
        })
    }

    // ── Media generation methods ───────────────────────────────────────────

    /// Call a fal.ai sync endpoint (https://fal.run/{endpoint}).
    async fn fal_sync_post(
        &self,
        endpoint: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, InferenceError> {
        let url = format!("{}/{}", self.media_base_url, endpoint);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Key {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("fal.ai request failed: {}", e)))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InferenceError::Connection(format!(
                "fal.ai {} status {}: {}",
                endpoint,
                status,
                sanitize_error_body(&text)
            )));
        }
        serde_json::from_str(&text)
            .map_err(|e| InferenceError::Json(format!("fal.ai JSON parse: {}", e)))
    }

    /// Call a fal.ai queue endpoint (https://queue.fal.run/{endpoint}) with polling.
    async fn fal_queue_post(
        &self,
        endpoint: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, InferenceError> {
        let submit_url = format!("{}/{}", self.queue_base_url, endpoint);
        let resp = self
            .client
            .post(&submit_url)
            .header("Authorization", format!("Key {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                InferenceError::Connection(format!("fal.ai queue submit failed: {}", e))
            })?;

        let status = resp.status();
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| InferenceError::Json(format!("fal.ai queue parse: {}", e)))?;

        if !status.is_success() {
            return Err(InferenceError::Connection(format!(
                "fal.ai queue {} status {}: {}",
                endpoint, status, v
            )));
        }

        let request_id = v
            .get("request_id")
            .and_then(|r| r.as_str())
            .unwrap_or("unknown")
            .to_string();

        let status_url = format!(
            "{}/{}/requests/{}/status",
            self.queue_base_url, endpoint, request_id
        );
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);

        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(InferenceError::Connection(format!(
                    "fal.ai queue poll timed out after 120s (request_id: {})",
                    request_id
                )));
            }
            match self
                .client
                .get(&status_url)
                .header("Authorization", format!("Key {}", self.api_key))
                .send()
                .await
            {
                Ok(resp) => {
                    let v: serde_json::Value = resp
                        .json()
                        .await
                        .map_err(|e| InferenceError::Json(format!("fal.ai status parse: {}", e)))?;
                    match v.get("status").and_then(|s| s.as_str()) {
                        Some("COMPLETED") => break,
                        Some("FAILED") => {
                            return Err(InferenceError::Generation(format!(
                                "fal.ai job failed: {}",
                                v
                            )));
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    return Err(InferenceError::Connection(format!(
                        "fal.ai status check failed: {}",
                        e
                    )));
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        let result_url = format!(
            "{}/{}/requests/{}",
            self.queue_base_url, endpoint, request_id
        );
        let resp = self
            .client
            .get(&result_url)
            .header("Authorization", format!("Key {}", self.api_key))
            .send()
            .await
            .map_err(|e| {
                InferenceError::Connection(format!("fal.ai result fetch failed: {}", e))
            })?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InferenceError::Connection(format!(
                "fal.ai result {} status {}: {}",
                endpoint, status, text
            )));
        }
        serde_json::from_str(&text)
            .map_err(|e| InferenceError::Json(format!("fal.ai result parse: {}", e)))
    }

    /// Generate an image from a text prompt.
    /// Endpoint: fal-ai/flux/schnell (fast) or fal-ai/flux-pro (quality).
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated image generation
    /// pre:  prompt is a non-empty text description
    /// post: returns Ok(serde_json::Value) with generated image data
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn generate_image(
        &self,
        prompt: &str,
        image_size: Option<&str>,
        num_images: Option<u32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let body = serde_json::json!({
            "prompt": prompt,
            "image_size": image_size.unwrap_or("1024x1024"),
            "num_images": num_images.unwrap_or(1),
        });
        self.fal_sync_post("fal-ai/flux/schnell", body).await
    }

    /// Transform an existing image with a prompt (image-to-image).
    /// Endpoint: fal-ai/flux/dev/image-to-image
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated image editing
    /// pre:  image_url is a valid, accessible image URL
    /// pre:  prompt is a non-empty transformation instruction
    /// post: returns Ok(serde_json::Value) with transformed image data
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn image_to_image(
        &self,
        image_url: &str,
        prompt: &str,
        strength: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let mut body = serde_json::json!({
            "prompt": prompt,
            "image_url": image_url,
        });
        if let Some(s) = strength {
            body["strength"] = serde_json::json!(s);
        }
        self.fal_sync_post("fal-ai/flux/dev/image-to-image", body)
            .await
    }

    /// Remove background from an image.
    /// Endpoint: fal-ai/birefnet
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
        self.fal_sync_post("fal-ai/birefnet", body).await
    }

    /// Upscale an image.
    /// Endpoint: fal-ai/seedvr2 (queue)
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated image upscaling
    /// pre:  image_url is a valid, accessible image URL
    /// post: returns Ok(serde_json::Value) with upscaled image data
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn upscale(
        &self,
        image_url: &str,
        scale: Option<u32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let body = serde_json::json!({
            "image_url": image_url,
            "scale": scale.unwrap_or(4),
        });
        self.fal_queue_post("fal-ai/seedvr2", body).await
    }

    /// Generate a video from a text prompt.
    /// Endpoint: fal-ai/minimax/video-01-live (queue)
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated video generation
    /// pre:  prompt is a non-empty text description
    /// post: returns Ok(serde_json::Value) with generated video data
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn generate_video(
        &self,
        prompt: &str,
        duration: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let mut body = serde_json::json!({"prompt": prompt});
        if let Some(d) = duration {
            body["duration"] = serde_json::json!(d);
        }
        self.fal_queue_post("fal-ai/minimax/video-01-live", body)
            .await
    }

    /// Animate a still image into a video.
    /// Endpoint: fal-ai/seedance-2.0/image-to-video (queue)
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated video generation
    /// pre:  image_url is a valid, accessible image URL
    /// post: returns Ok(serde_json::Value) with generated video data
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn image_to_video(
        &self,
        image_url: &str,
        prompt: Option<&str>,
        duration: Option<f32>,
    ) -> Result<serde_json::Value, InferenceError> {
        let mut body = serde_json::json!({"image_url": image_url});
        if let Some(p) = prompt {
            body["prompt"] = serde_json::json!(p);
        }
        if let Some(d) = duration {
            body["duration"] = serde_json::json!(d);
        }
        self.fal_queue_post("fal-ai/seedance-2.0/image-to-video", body)
            .await
    }

    /// Segment/extract a specific object from an image.
    /// Endpoint: fal-ai/florence-2-large/referring-expression-segmentation
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated image segmentation
    /// pre:  image_url is a valid, accessible image URL
    /// pre:  object_description is a non-empty description of the object to segment
    /// post: returns Ok(serde_json::Value) with segmented object data
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn segment_object(
        &self,
        image_url: &str,
        object_description: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        let body = serde_json::json!({
            "image_url": image_url,
            "prompt": object_description,
        });
        self.fal_sync_post(
            "fal-ai/florence-2-large/referring-expression-segmentation",
            body,
        )
        .await
    }

    /// Generate speech from text with a voice preset.
    /// Uses fal.ai ElevenLabs TTS (eleven-v3).
    /// Available voices: Rachel, Aria, Roger, Sarah, Laura, Charlie, George,
    /// Callum, River, Liam, Charlotte, Alice, Matilda, Will, Jessica, Eric,
    /// Chris, Brian, Daniel, Lily, Bill. Default: "Rachel".
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated speech synthesis
    /// pre:  text is non-empty
    /// pre:  voice is a valid voice preset name
    /// post: returns Ok(serde_json::Value) with generated speech audio data
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn generate_speech(
        &self,
        text: &str,
        voice: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        let body = serde_json::json!({
            "text": text,
            "voice": voice,
        });
        self.fal_sync_post("fal-ai/elevenlabs/tts/eleven-v3", body)
            .await
    }

    /// Transcribe speech audio to text using Whisper.
    /// Endpoint: fal-ai/whisper
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated speech transcription
    /// pre:  audio_url is a valid, accessible audio file URL
    /// post: returns Ok(serde_json::Value) with transcription data
    /// post: if API call fails → Err(InferenceError::Connection)
    pub async fn transcribe(&self, audio_url: &str) -> Result<serde_json::Value, InferenceError> {
        let body = serde_json::json!({"audio_url": audio_url});
        self.fal_sync_post("fal-ai/whisper", body).await
    }

    // ── Workflow execution ─────────────────────────────────────────────

    /// Execute a workflow plan JSON against Fal GPU infrastructure.
    ///
    /// Delegates to the general workflow engine (`crate::workflow`): fal.ai's
    /// `Input`/`Run`/`Display` DAG is parsed into `Source`/`Compute`/`Sink` and
    /// executed in dependency order with `$reference` resolution. Results are
    /// byte-identical to the pre-refactor implementation (sequential,
    /// abort-on-failure — fal.ai JSON carries no failure policy).
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated workflow execution
    /// pre:  workflow is a valid JSON object with input, run, and display nodes
    /// post: returns Ok(WorkflowResult) with output URLs, fields, and node results
    /// post: if workflow is malformed → Err(InferenceError::Json)
    /// post: if workflow has circular deps → Err(InferenceError::Generation)
    /// post: if a Fal API call fails → Err(InferenceError::Connection)
    pub async fn execute_workflow(
        &self,
        workflow: &Value,
    ) -> Result<WorkflowResult, InferenceError> {
        let graph = crate::workflow::fal_adapter::parse_fal_workflow(workflow)?;
        graph.execute(self).await
    }
}

impl MediaProvider for FalBackend {
    fn id(&self) -> &'static str {
        "fal.ai"
    }

    fn supports(&self, op: MediaOp) -> bool {
        matches!(
            op,
            MediaOp::GenerateImage
                | MediaOp::ImageToImage
                | MediaOp::RemoveBackground
                | MediaOp::Upscale
                | MediaOp::GenerateVideo
                | MediaOp::ImageToVideo
                | MediaOp::SegmentObject
                | MediaOp::GenerateSpeech
                | MediaOp::Transcribe
                | MediaOp::ExecuteWorkflow
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
                    let image_size = params.size.clone();
                    self.generate_image(&prompt, image_size.as_deref(), params.count)
                        .await
                }
                MediaOp::ImageToImage => {
                    let image_url = params.image_url.clone().unwrap_or_default();
                    let prompt = params.prompt.clone().unwrap_or_default();
                    self.image_to_image(&image_url, &prompt, params.strength)
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
                MediaOp::GenerateVideo => {
                    let prompt = params.prompt.clone().unwrap_or_default();
                    self.generate_video(&prompt, params.duration).await
                }
                MediaOp::ImageToVideo => {
                    let image_url = params.image_url.clone().unwrap_or_default();
                    let prompt = params.prompt.clone();
                    self.image_to_video(&image_url, prompt.as_deref(), params.duration)
                        .await
                }
                MediaOp::SegmentObject => {
                    let image_url = params.image_url.clone().unwrap_or_default();
                    let object_description = params.object_description.clone().unwrap_or_default();
                    self.segment_object(&image_url, &object_description).await
                }
                MediaOp::GenerateSpeech => {
                    let text = params.text.clone().unwrap_or_default();
                    let voice = params.voice.clone().unwrap_or_else(|| "Rachel".to_string());
                    self.generate_speech(&text, &voice).await
                }
                MediaOp::Transcribe => {
                    let audio_url = params.audio_url.clone().unwrap_or_default();
                    self.transcribe(&audio_url).await
                }
                MediaOp::ExecuteWorkflow => {
                    let workflow = params.workflow.clone().unwrap_or(Value::Null);
                    let result = self.execute_workflow(&workflow).await?;
                    serde_json::to_value(result).map_err(|e| {
                        InferenceError::Json(format!("WorkflowResult serialize failed: {e}"))
                    })
                }
            }
        })
    }
}

/// Drives the general workflow engine's `Compute` nodes against the fal.ai
/// provider: `Sync` → `fal_sync_post`, `Queue` → `fal_queue_post`.
impl NodeExecutor for FalBackend {
    fn execute_node<'a>(
        &'a self,
        app: &'a str,
        input: Value,
        mode: ExecutionMode,
    ) -> Pin<Box<dyn Future<Output = Result<Value, InferenceError>> + Send + 'a>> {
        Box::pin(async move {
            match mode {
                ExecutionMode::Sync => self.fal_sync_post(app, input).await,
                ExecutionMode::Queue => self.fal_queue_post(app, input).await,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> Arc<reqwest::Client> {
        Arc::new(reqwest::Client::new())
    }

    /// expect: "Inference backend construction fails correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates boundary enforcement without key
    #[test]
    fn construction_fails_without_api_key() {
        let config = InferenceConfig::default();
        assert!(config.fal_api_key.is_empty());
        let result = FalBackend::new(&config, test_client());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("FALAI_API_KEY"),
            "error should mention FALAI_API_KEY, got: {}",
            err
        );
    }

    /// expect: "Inference backend construction succeeds correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates boundary construction with key
    #[test]
    fn construction_succeeds_with_api_key() {
        let config = InferenceConfig {
            fal_api_key: "test-key-123".into(),
            ..Default::default()
        };
        let result = FalBackend::new(&config, test_client());
        assert!(
            result.is_ok(),
            "should succeed with API key: {:?}",
            result.err()
        );
    }

    /// expect: "Inference vision support heuristic works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates vision model heuristic
    #[test]
    fn vision_support_heuristic_recognizes_fal_models() {
        use crate::RouterModelEntry;
        assert_eq!(
            RouterModelEntry::infer_vision_support("paddleocr", None),
            Some(true)
        );
        assert_eq!(
            RouterModelEntry::infer_vision_support("nemotron-parse", None),
            Some(true)
        );
        assert_eq!(
            RouterModelEntry::infer_vision_support("fal.ai/paddleocr", None),
            Some(true)
        );
    }
}
