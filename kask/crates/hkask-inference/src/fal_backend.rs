//! fal.ai backend — cloud inference via OpenAI-compatible API.
//!
//! fal.ai exposes `/v1/chat/completions` for text and vision models.
//! Requires Bearer token authentication via `FA_API_KEY`.
//!
//! Model listing: fal.ai does not expose a standard `/v1/models` endpoint.
//! Instead, a static catalog of known vision-capable models is used.

use crate::chat_protocol::{
    ChatResponse, build_vision_request, chat_response_to_result, stream_chat_completion,
    validate_prompt,
};
use crate::config::InferenceConfig;
use crate::fal_workflow::{self, WorkflowNode, WorkflowResult};
use crate::openai_compat::{openai_compatible_generate, openai_compatible_generate_messages};
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferenceResult, InferenceStreamChunk,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// fal.ai backend for chat completions and vision inference.
#[derive(Debug)]
pub struct FalBackend {
    base_url: String,
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
                "fal.ai API key not configured (set FA_API_KEY)".into(),
            ));
        }
        Ok(Self {
            base_url: config.fal_base_url.clone(),
            media_base_url: config.fal_media_base_url.clone(),
            queue_base_url: config.fal_queue_base_url.clone(),
            api_key: config.fal_api_key.clone(),
            client,
        })
    }

    /// Send a chat completion request to fal.ai.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated text generation
    /// pre:  model is a valid fal.ai model name
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
            "Key",
            "FA",
        )
        .await
    }

    /// Send a multi-turn chat completion request to fal.ai with an explicit
    /// message array.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated multi-turn text generation
    /// pre:  model is a valid fal.ai model name
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
            "Key",
            "FA",
        )
        .await
    }

    /// Vision/multimodal inference with base64-encoded images.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated multimodal generation
    /// pre:  model is a valid fal.ai vision-capable model name
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
        validate_prompt(prompt)?;
        if images.is_empty() {
            return Err(InferenceError::Generation("No images provided".into()));
        }
        let request = build_vision_request(model, prompt, images, params);
        let response = self
            .client
            .post(format!("{}/chat/completions", self.media_base_url))
            .header("Authorization", format!("Key {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| InferenceError::Connection(format!("fal.ai vision: {}", e)))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(InferenceError::Connection(format!(
                "fal.ai {}: {}",
                status, body
            )));
        }
        let body = response
            .text()
            .await
            .map_err(|e| InferenceError::Connection(format!("fal.ai body read: {}", e)))?;
        let chat_response: ChatResponse = serde_json::from_str(&body).map_err(|e| {
            let preview = if body.len() > 500 {
                format!("{}...", &body[..500])
            } else {
                body.clone()
            };
            InferenceError::Json(format!("fal.ai JSON: {} | body: {}", e, preview))
        })?;
        chat_response_to_result(chat_response)
    }

    /// List available models from fal.ai.
    /// Stream a chat completion from fal.ai via SSE.
    /// Generate a streaming completion from Fal.
    ///
    /// expect: "The system regulates text/image/speech generation through provider membranes"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated streaming text generation
    /// pre:  model is a valid Fal model name
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
        let auth = format!("Key {}", self.api_key);
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

    /// List known fal.ai models from the static catalog.
    ///
    /// fal.ai does not expose a standard `/v1/models` endpoint.
    /// Returns a curated list of vision-capable models known to work
    /// with the OpenAI-compatible chat completions endpoint.
    /// List available models from fal.ai static catalog.
    ///
    /// Returns `RouterModelEntry` with provider prefix applied on each entry.
    ///
    /// expect: "I can discover available models across providers"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — static model catalog for variety
    /// pre:  none (static catalog, no API call)
    /// post: returns `Vec<RouterModelEntry>` with curated model list
    #[must_use]
    pub async fn list_models(&self) -> Vec<crate::RouterModelEntry> {
        use crate::config::ProviderId;

        // Static catalog of known fal.ai vision models.
        // These are models confirmed to work via the chat completions endpoint.
        vec!["paddleocr", "nemotron-parse", "docres"]
            .into_iter()
            .map(|id| crate::RouterModelEntry::from_model_entry(ProviderId::Fal, id))
            .collect()
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
                endpoint, status, text
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

    async fn execute_node(&self, app: &str, input: &Value) -> Result<Value, InferenceError> {
        self.fal_sync_post(app, input.clone()).await
    }

    /// Execute a workflow plan JSON against Fal GPU infrastructure.
    ///
    /// Parses the DAG, topologically sorts nodes, executes each model
    /// call sequentially, resolves `$references` between nodes, and
    /// returns output URLs with metadata.
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
        let start = std::time::Instant::now();

        let nodes = fal_workflow::parse_workflow_nodes(workflow)?;
        fal_workflow::validate_workflow_structure(&nodes)?;

        let node_map: HashMap<&str, &WorkflowNode> = nodes.iter().map(|n| (n.id(), n)).collect();
        let order = fal_workflow::topological_sort(&nodes)?;

        tracing::debug!(
            target: "hkask.fal",
            node_count = nodes.len(),
            execution_order = ?order.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
            "Workflow execution started"
        );

        let mut results: HashMap<String, Value> = HashMap::new();
        let mut output_fields = Value::Null;
        let mut output_urls: Vec<String> = Vec::new();

        for node_id in &order {
            let node = node_map.get(node_id.as_str()).ok_or_else(|| {
                InferenceError::Generation(format!("Node '{node_id}' not found in workflow"))
            })?;

            match node {
                WorkflowNode::Input { input, .. } => {
                    results.insert(node_id.clone(), input.clone());
                }
                WorkflowNode::Run {
                    app,
                    input,
                    depends,
                    mode,
                    ..
                } => {
                    let resolved_input =
                        fal_workflow::resolve_references(input, &results, depends)?;
                    let node_result = match mode {
                        fal_workflow::ExecutionMode::Sync => {
                            self.execute_node(app, &resolved_input).await?
                        }
                        fal_workflow::ExecutionMode::Queue => {
                            self.fal_queue_post(app, resolved_input).await?
                        }
                    };
                    results.insert(node_id.clone(), node_result);
                }
                WorkflowNode::Display {
                    fields, depends, ..
                } => {
                    let resolved = fal_workflow::resolve_references(fields, &results, depends)?;
                    output_fields = resolved.clone();
                    output_urls = fal_workflow::extract_urls(&resolved);
                }
            }
        }

        let elapsed = start.elapsed().as_secs_f64();

        tracing::debug!(
            target: "hkask.fal",
            output_count = output_urls.len(),
            elapsed_seconds = elapsed,
            "Workflow execution complete"
        );

        Ok(WorkflowResult {
            output_urls,
            output_fields,
            node_results: results,
            elapsed_seconds: elapsed,
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
            err.to_string().contains("FA_API_KEY"),
            "error should mention FA_API_KEY, got: {}",
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

    /// expect: "Inference static catalog lookup works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates model variety catalog
    #[tokio::test]
    async fn static_catalog_returns_vision_models() {
        let config = InferenceConfig {
            fal_api_key: "test-key".into(),
            ..Default::default()
        };
        let backend = FalBackend::new(&config, test_client()).unwrap();
        let models = backend.list_models().await;
        assert!(!models.is_empty(), "catalog should not be empty");
        let ids: Vec<&str> = models.iter().map(|m| m.model.as_str()).collect();
        assert!(
            ids.contains(&"paddleocr"),
            "catalog should include paddleocr"
        );
        assert!(
            ids.contains(&"nemotron-parse"),
            "catalog should include nemotron-parse"
        );
        assert!(ids.contains(&"docres"), "catalog should include docres");
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
            RouterModelEntry::infer_vision_support("FA/paddleocr", None),
            Some(true)
        );
    }
}
