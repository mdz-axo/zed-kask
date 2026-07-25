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
    ChatMessage, ChatToolDefinition, InferenceError, InferencePort, InferenceResult,
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
        }
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
        };
        let this = self;
        async move { this.call(InferenceMethod::GenerateVision, params).await }.boxed()
    }
}
