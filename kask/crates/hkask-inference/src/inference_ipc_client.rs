//! `InferenceIpcClient` — an `InferencePort` implementation that delegates
//! to a Unix socket connection back to the zed process.
//!
//! This is the MCP-server side of the inference IPC bridge. When zed launches
//! an MCP server child process, it passes a Unix socket path via the
//! `HKASK_INFERENCE_SOCKET` env var. The MCP server constructs an
//! `InferenceIpcClient` instead of an `InferenceRouter`, and all inference
//! calls are routed back to zed's `LanguageModelRegistry` (with fusion, guard,
//! and zed's configured API keys).
//!
//! ## Protocol
//!
//! Newline-delimited JSON over a Unix socket. Each request is a single line;
//! each response is a single line. The `id` field correlates responses to
//! requests.
//!
//! ## Connection management
//!
//! The client holds a single socket connection. If the connection drops, the
//! next call returns an `InferenceError::Connection`. The caller can retry by
//! constructing a new client.
//!
//! ## Why not streaming?
//!
//! Streaming is not supported over IPC — the server side collects the stream
//! and returns a single `InferenceResult`. This matches the existing
//! `LanguageModelInferencePort` pattern and is sufficient for MCP server use
//! cases (OCR, classification, summarization, etc.).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::FutureExt;
use hkask_types::inference_ipc::{
    INFERENCE_SOCKET_ENV, InferenceMethod, InferenceOutcome, InferenceParams, InferenceRequest,
    InferenceResponse,
};
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, EmbeddingGenerationError, InferenceError, InferencePort,
    InferenceResult, MediaGenerateParams,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

/// An `InferencePort` that delegates to a Unix socket connection back to zed.
///
/// Construct with `InferenceIpcClient::connect()` or
/// `InferenceIpcClient::from_env()`.
pub struct InferenceIpcClient {
    /// The socket connection, protected by a mutex so only one request is
    /// in flight at a time (the protocol is request-response, not multiplexed).
    stream: Arc<Mutex<Option<UnixStream>>>,
    /// Next request ID.
    next_id: AtomicU64,
}

impl InferenceIpcClient {
    /// Connect to a Unix socket at the given path.
    pub async fn connect(socket_path: &Path) -> Result<Self, InferenceError> {
        let stream = UnixStream::connect(socket_path)
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC connect failed: {e}")))?;
        Ok(Self {
            stream: Arc::new(Mutex::new(Some(stream))),
            next_id: AtomicU64::new(1),
        })
    }

    /// Construct from the `HKASK_INFERENCE_SOCKET` env var.
    ///
    /// Returns `None` if the env var is not set (MCP server falls back to
    /// `InferenceRouter::from_env()` in that case).
    pub async fn from_env() -> Option<Result<Self, InferenceError>> {
        let path = std::env::var(INFERENCE_SOCKET_ENV).ok()?;
        if path.is_empty() {
            return None;
        }
        Some(Self::connect(Path::new(&path)).await)
    }

    /// Send a request and receive the response.
    async fn call(
        &self,
        method: InferenceMethod,
        params: InferenceParams,
    ) -> Result<InferenceResult, InferenceError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = InferenceRequest { id, method, params };
        let request_json = serde_json::to_string(&request)
            .map_err(|e| InferenceError::Json(format!("IPC serialize failed: {e}")))?;

        let mut guard = self.stream.lock().await;
        let stream = guard
            .as_mut()
            .ok_or_else(|| InferenceError::Connection("IPC socket closed".into()))?;

        // Send the request as a single line.
        stream
            .write_all(request_json.as_bytes())
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC write failed: {e}")))?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC write failed: {e}")))?;
        stream
            .flush()
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC flush failed: {e}")))?;

        // Read the response line.
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC read failed: {e}")))?;

        if line.is_empty() {
            // Connection closed by the server.
            *guard = None;
            return Err(InferenceError::Connection(
                "IPC socket closed by server".into(),
            ));
        }

        let response: InferenceResponse = serde_json::from_str(&line)
            .map_err(|e| InferenceError::Json(format!("IPC deserialize failed: {e}")))?;

        if response.id != id {
            return Err(InferenceError::Connection(format!(
                "IPC ID mismatch: expected {id}, got {}",
                response.id
            )));
        }

        match response.outcome {
            InferenceOutcome::Result { result } => Ok(result),
            InferenceOutcome::Error { error } => Err(error.into()),
            InferenceOutcome::Embeddings { .. } => Err(InferenceError::Connection(
                "received Embeddings outcome for a non-embed request".into(),
            )),
            InferenceOutcome::ModelList { .. } => Err(InferenceError::Connection(
                "received ModelList outcome for a non-list-models request".into(),
            )),
            InferenceOutcome::Media { .. } => Err(InferenceError::Connection(
                "received Media outcome for a non-media request".into(),
            )),
        }
    }

    /// Send an embedding request and receive the response.
    async fn call_embed(
        &self,
        model: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, EmbeddingGenerationError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = InferenceRequest {
            id,
            method: InferenceMethod::Embed,
            params: InferenceParams {
                prompt: None,
                messages: None,
                images: None,
                parameters: LLMParameters::default(),
                model_override: None,
                tools: None,
                embed_model: Some(model.to_string()),
                embed_texts: Some(texts.to_vec()),
                media_op: None,
                media_prompt: None,
                media_image_url: None,
                media_audio_url: None,
                media_text: None,
                media_voice: None,
                media_size: None,
                media_count: None,
                media_strength: None,
            media_scale: None,
                media_duration: None,
                media_object_description: None,
                media_language: None,
                media_workflow: None,
            },
        };
        let request_json = serde_json::to_string(&request)
            .map_err(|e| EmbeddingGenerationError::Json(format!("IPC serialize failed: {e}")))?;

        let mut guard = self.stream.lock().await;
        let stream = guard
            .as_mut()
            .ok_or_else(|| EmbeddingGenerationError::Connection("IPC socket closed".into()))?;

        stream
            .write_all(request_json.as_bytes())
            .await
            .map_err(|e| EmbeddingGenerationError::Connection(format!("IPC write failed: {e}")))?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|e| EmbeddingGenerationError::Connection(format!("IPC write failed: {e}")))?;
        stream
            .flush()
            .await
            .map_err(|e| EmbeddingGenerationError::Connection(format!("IPC flush failed: {e}")))?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| EmbeddingGenerationError::Connection(format!("IPC read failed: {e}")))?;

        if line.is_empty() {
            *guard = None;
            return Err(EmbeddingGenerationError::Connection(
                "IPC socket closed by server".into(),
            ));
        }

        let response: InferenceResponse = serde_json::from_str(&line)
            .map_err(|e| EmbeddingGenerationError::Json(format!("IPC deserialize failed: {e}")))?;

        if response.id != id {
            return Err(EmbeddingGenerationError::Connection(format!(
                "IPC ID mismatch: expected {id}, got {}",
                response.id
            )));
        }

        match response.outcome {
            InferenceOutcome::Embeddings { embeddings } => Ok(embeddings),
            InferenceOutcome::Error { error } => Err(EmbeddingGenerationError::Connection(
                format!("{}: {}", error.code, error.message),
            )),
            InferenceOutcome::Result { .. } => Err(EmbeddingGenerationError::Connection(
                "received Result outcome for an embed request".into(),
            )),
            InferenceOutcome::ModelList { .. } => Err(EmbeddingGenerationError::Connection(
                "received ModelList outcome for an embed request".into(),
            )),
            InferenceOutcome::Media { .. } => Err(EmbeddingGenerationError::Connection(
                "received Media outcome for an embed request".into(),
            )),
        }
    }

    /// Generate embeddings for a batch of texts via the IPC bridge.
    ///
    /// `model` is the provider-prefixed model string (e.g.
    /// `DeepInfra/Qwen/Qwen3-Embedding-0.6B`). The zed process strips the
    /// prefix and resolves credentials from its `LanguageModelRegistry`.
    pub async fn embed(
        &self,
        model: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, EmbeddingGenerationError> {
        if texts.is_empty() {
            return Err(EmbeddingGenerationError::EmptyResponse);
        }
        self.call_embed(model, texts).await
    }

    /// List available models from zed's `LanguageModelRegistry` via the IPC bridge.
    async fn call_list_models(
        &self,
    ) -> Result<Vec<hkask_types::inference_ipc::ModelListEntry>, InferenceError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = InferenceRequest {
            id,
            method: InferenceMethod::ListModels,
            params: InferenceParams {
                prompt: None,
                messages: None,
                images: None,
                parameters: LLMParameters::default(),
                model_override: None,
                tools: None,
                embed_model: None,
                embed_texts: None,
                media_op: None,
                media_prompt: None,
                media_image_url: None,
                media_audio_url: None,
                media_text: None,
                media_voice: None,
                media_size: None,
                media_count: None,
                media_strength: None,
            media_scale: None,
                media_duration: None,
                media_object_description: None,
                media_language: None,
                media_workflow: None,
            },
        };
        let request_json = serde_json::to_string(&request)
            .map_err(|e| InferenceError::Json(format!("IPC serialize failed: {e}")))?;

        let mut guard = self.stream.lock().await;
        let stream = guard
            .as_mut()
            .ok_or_else(|| InferenceError::Connection("IPC socket closed".into()))?;

        stream
            .write_all(request_json.as_bytes())
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC write failed: {e}")))?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC write failed: {e}")))?;
        stream
            .flush()
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC flush failed: {e}")))?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC read failed: {e}")))?;

        if line.is_empty() {
            *guard = None;
            return Err(InferenceError::Connection(
                "IPC socket closed by server".into(),
            ));
        }

        let response: InferenceResponse = serde_json::from_str(&line)
            .map_err(|e| InferenceError::Json(format!("IPC deserialize failed: {e}")))?;

        if response.id != id {
            return Err(InferenceError::Connection(format!(
                "IPC ID mismatch: expected {id}, got {}",
                response.id
            )));
        }

        match response.outcome {
            InferenceOutcome::ModelList { models } => Ok(models),
            InferenceOutcome::Error { error } => Err(error.into()),
            InferenceOutcome::Result { .. } => Err(InferenceError::Connection(
                "received Result outcome for a list_models request".into(),
            )),
            InferenceOutcome::Embeddings { .. } => Err(InferenceError::Connection(
                "received Embeddings outcome for a list_models request".into(),
            )),
            InferenceOutcome::Media { .. } => Err(InferenceError::Connection(
                "received Media outcome for a list_models request".into(),
            )),
        }
    }

    /// Send a media-generation request and receive the response.
    ///
    /// `op` selects the backend method (e.g. "generate_image", "transcribe").
    /// `params` carries the op-specific fields. The server-side dispatch
    /// reads only the fields relevant to each op.
    async fn call_media_generate(
        &self,
        op: &str,
        params: &MediaGenerateParams,
    ) -> Result<serde_json::Value, InferenceError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = InferenceRequest {
            id,
            method: InferenceMethod::MediaGenerate,
            params: InferenceParams {
                prompt: None,
                messages: None,
                images: None,
                parameters: LLMParameters::default(),
                model_override: None,
                tools: None,
                embed_model: None,
                embed_texts: None,
                media_op: Some(op.to_string()),
                media_prompt: params.prompt.clone(),
                media_image_url: params.image_url.clone(),
                media_audio_url: params.audio_url.clone(),
                media_text: params.text.clone(),
                media_voice: params.voice.clone(),
                media_size: params.size.clone(),
                media_count: params.count,
                media_strength: params.strength,
                media_scale: params.scale,
                media_duration: params.duration,
                media_object_description: params.object_description.clone(),
                media_language: params.language.clone(),
                media_workflow: params.workflow.clone(),
            },
        };
        let request_json = serde_json::to_string(&request)
            .map_err(|e| InferenceError::Json(format!("IPC serialize failed: {e}")))?;

        let mut guard = self.stream.lock().await;
        let stream = guard
            .as_mut()
            .ok_or_else(|| InferenceError::Connection("IPC socket closed".into()))?;

        stream
            .write_all(request_json.as_bytes())
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC write failed: {e}")))?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC write failed: {e}")))?;
        stream
            .flush()
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC flush failed: {e}")))?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC read failed: {e}")))?;

        if line.is_empty() {
            *guard = None;
            return Err(InferenceError::Connection(
                "IPC socket closed by server".into(),
            ));
        }

        let response: InferenceResponse = serde_json::from_str(&line)
            .map_err(|e| InferenceError::Json(format!("IPC deserialize failed: {e}")))?;

        if response.id != id {
            return Err(InferenceError::Connection(format!(
                "IPC ID mismatch: expected {id}, got {}",
                response.id
            )));
        }

        match response.outcome {
            InferenceOutcome::Media { media } => Ok(media),
            InferenceOutcome::Error { error } => Err(error.into()),
            InferenceOutcome::Result { .. } => Err(InferenceError::Connection(
                "received Result outcome for a media request".into(),
            )),
            InferenceOutcome::Embeddings { .. } => Err(InferenceError::Connection(
                "received Embeddings outcome for a media request".into(),
            )),
            InferenceOutcome::ModelList { .. } => Err(InferenceError::Connection(
                "received ModelList outcome for a media request".into(),
            )),
        }
    }

    /// Generate media (image, video, speech, transcription) via the IPC bridge.
    ///
    /// `op` selects the backend method (e.g. "generate_image", "transcribe").
    /// `params` carries the op-specific fields. The zed process dispatches
    /// to its hKask `InferenceRouter` (fal.ai/DeepInfra backends).
    pub async fn media_generate(
        &self,
        op: &str,
        params: &MediaGenerateParams,
    ) -> Result<serde_json::Value, InferenceError> {
        self.call_media_generate(op, params).await
    }
}

impl InferencePort for InferenceIpcClient {
    fn generate(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let params = InferenceParams {
            prompt: Some(prompt.to_string()),
            messages: None,
            images: None,
            parameters: parameters.clone(),
            model_override: None,
            tools: tools.map(|t| t.to_vec()),
            embed_model: None,
            embed_texts: None,
            media_op: None,
            media_prompt: None,
            media_image_url: None,
            media_audio_url: None,
            media_text: None,
            media_voice: None,
            media_size: None,
            media_count: None,
            media_strength: None,
            media_scale: None,
            media_duration: None,
            media_object_description: None,
            media_language: None,
            media_workflow: None,
        };
        let this = self;
        async move { this.call(InferenceMethod::Generate, params).await }.boxed()
    }

    fn generate_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let params = InferenceParams {
            prompt: Some(prompt.to_string()),
            messages: None,
            images: None,
            parameters: parameters.clone(),
            model_override: model_override.map(|s| s.to_string()),
            tools: tools.map(|t| t.to_vec()),
            embed_model: None,
            embed_texts: None,
            media_op: None,
            media_prompt: None,
            media_image_url: None,
            media_audio_url: None,
            media_text: None,
            media_voice: None,
            media_size: None,
            media_count: None,
            media_strength: None,
            media_scale: None,
            media_duration: None,
            media_object_description: None,
            media_language: None,
            media_workflow: None,
        };
        let this = self;
        async move { this.call(InferenceMethod::GenerateWithModel, params).await }.boxed()
    }

    fn generate_with_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let params = InferenceParams {
            prompt: None,
            messages: Some(messages.to_vec()),
            images: None,
            parameters: parameters.clone(),
            model_override: model_override.map(|s| s.to_string()),
            tools: tools.map(|t| t.to_vec()),
            embed_model: None,
            embed_texts: None,
            media_op: None,
            media_prompt: None,
            media_image_url: None,
            media_audio_url: None,
            media_text: None,
            media_voice: None,
            media_size: None,
            media_count: None,
            media_strength: None,
            media_scale: None,
            media_duration: None,
            media_object_description: None,
            media_language: None,
            media_workflow: None,
        };
        let this = self;
        async move {
            this.call(InferenceMethod::GenerateWithMessages, params)
                .await
        }
        .boxed()
    }

    fn generate_vision(
        &self,
        prompt: &str,
        images: &[String],
        parameters: &LLMParameters,
        model_override: Option<&str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        let params = InferenceParams {
            prompt: Some(prompt.to_string()),
            messages: None,
            images: Some(images.to_vec()),
            parameters: parameters.clone(),
            model_override: model_override.map(|s| s.to_string()),
            tools: None,
            embed_model: None,
            embed_texts: None,
            media_op: None,
            media_prompt: None,
            media_image_url: None,
            media_audio_url: None,
            media_text: None,
            media_voice: None,
            media_size: None,
            media_count: None,
            media_strength: None,
            media_scale: None,
            media_duration: None,
            media_object_description: None,
            media_language: None,
            media_workflow: None,
        };
        let this = self;
        async move { this.call(InferenceMethod::GenerateVision, params).await }.boxed()
    }

    fn embed<'a>(&'a self, model: &str, texts: &[String]) -> hkask_types::EmbedFuture<'a> {
        let model = model.to_string();
        let texts = texts.to_vec();
        let this = self;
        async move { this.embed(&model, &texts).await }.boxed()
    }

    fn list_models<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Vec<hkask_types::ModelEntry>> + Send + 'a>,
    > {
        let this = self;
        Box::pin(async move {
            match this.call_list_models().await {
                Ok(entries) => entries
                    .into_iter()
                    .map(|e| {
                        let name = e.name.clone();
                        hkask_types::ModelEntry {
                            prefixed_name: name.clone(),
                            model: name.split('/').nth(1).unwrap_or(&name).to_string(),
                            supports_vision: e.supports_vision,
                        }
                    })
                    .collect(),
                Err(e) => {
                    tracing::warn!(target: "hkask.inference", error = %e, "IPC list_models failed — returning empty");
                    Vec::new()
                }
            }
        })
    }

    fn media_generate<'a>(
        &'a self,
        op: &str,
        params: &MediaGenerateParams,
    ) -> hkask_types::MediaFuture<'a> {
        let op = op.to_string();
        let params = params.clone();
        let this = self;
        async move { this.media_generate(&op, &params).await }.boxed()
    }
}
