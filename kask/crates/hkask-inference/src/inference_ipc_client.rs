//! `InferenceIpcClient` — an `InferencePort` implementation that delegates
//! to a Unix socket connection back to the zed process.
//!
//! This is the MCP-server side of the inference IPC bridge. When zed launches
//! an MCP server child process, it passes a Unix socket path via the
//! `HKASK_INFERENCE_SOCKET` env var. The MCP server constructs an
//! calls are routed back to zed's `LanguageModelRegistry` (with guard,
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
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

/// Maximum size of a single newline-delimited IPC response line.
///
/// Responses carry generated text and base64 media, so this is generous;
/// 16 MiB caps unbounded `read_line` growth (CWE-400). Must match the
/// server side in `kask_bridge/src/inference_ipc_server.rs`; duplicated here
/// because the shared types crate is owned by another workstream.
const MAX_IPC_LINE_BYTES: u64 = 16 * 1024 * 1024;

/// Maximum time to wait for a single IPC response line.
///
/// Generous enough for long-running inference, but prevents the MCP server
/// from blocking forever if the zed process hangs. On timeout the returned
/// `std::io::Error` is treated by callers as a read failure, which nulls the
/// cached stream so the next call reconnects instead of retrying on a dead
/// connection.
const IPC_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Read one newline-delimited response from the socket, capped at
/// `MAX_IPC_LINE_BYTES`. Returns `None` when the server closed the
/// connection before sending any bytes; a line without a terminating
/// newline (overlong or truncated) is an error.
async fn read_response_line(stream: &mut UnixStream) -> Result<Option<String>, std::io::Error> {
    // +1 so a line of exactly cap bytes followed by a newline is accepted
    // while anything longer is detected as missing-newline.
    let mut reader = BufReader::new(stream.take(MAX_IPC_LINE_BYTES + 1));
    let mut line = String::new();
    let bytes_read = tokio::time::timeout(IPC_READ_TIMEOUT, reader.read_line(&mut line))
        .await
        .map_err(|_elapsed| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("IPC read timed out after {}s", IPC_READ_TIMEOUT.as_secs()),
            )
        })??;
    if bytes_read == 0 {
        return Ok(None);
    }
    if !line.ends_with('\n') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "IPC response exceeds MAX_IPC_LINE_BYTES or is truncated",
        ));
    }
    line.pop();
    Ok(Some(line))
}

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

        // Read the response line. Null the cached stream on every error branch
        // (read failure, clean EOF, parse failure, ID mismatch) so the next call
        // reconnects instead of retrying on a dead/half-consumed stream.
        let line = match read_response_line(stream).await {
            Ok(line) => line,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Connection(format!("IPC read failed: {e}")));
            }
        };

        let Some(line) = line else {
            *guard = None;
            return Err(InferenceError::Connection(
                "IPC socket closed by server".into(),
            ));
        };

        let response: InferenceResponse = match serde_json::from_str(&line) {
            Ok(response) => response,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Json(format!("IPC deserialize failed: {e}")));
            }
        };

        if response.id != id {
            *guard = None;
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
            InferenceOutcome::ToolResult { .. } => Err(InferenceError::Connection(
                "received ToolResult outcome for a non-tool-invoke request".into(),
            )),
            InferenceOutcome::SkillResult { .. } => Err(InferenceError::Connection(
                "received SkillResult outcome for a non-skill-execute request".into(),
            )),
            InferenceOutcome::WorktreeThread { .. } => Err(InferenceError::Connection(
                "received WorktreeThread outcome for a non-worktree-thread request".into(),
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
                embed_model: Some(model.to_string()),
                embed_texts: Some(texts.to_vec()),
                ..Default::default()
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

        let line = match read_response_line(stream).await {
            Ok(line) => line,
            Err(e) => {
                *guard = None;
                return Err(EmbeddingGenerationError::Connection(format!(
                    "IPC read failed: {e}"
                )));
            }
        };

        let Some(line) = line else {
            *guard = None;
            return Err(EmbeddingGenerationError::Connection(
                "IPC socket closed by server".into(),
            ));
        };

        let response: InferenceResponse = match serde_json::from_str(&line) {
            Ok(response) => response,
            Err(e) => {
                *guard = None;
                return Err(EmbeddingGenerationError::Json(format!(
                    "IPC deserialize failed: {e}"
                )));
            }
        };

        if response.id != id {
            *guard = None;
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
            InferenceOutcome::ToolResult { .. } => Err(EmbeddingGenerationError::Connection(
                "received ToolResult outcome for an embed request".into(),
            )),
            InferenceOutcome::SkillResult { .. } => Err(EmbeddingGenerationError::Connection(
                "received SkillResult outcome for an embed request".into(),
            )),
            InferenceOutcome::WorktreeThread { .. } => Err(EmbeddingGenerationError::Connection(
                "received WorktreeThread outcome for an embed request".into(),
            )),
        }
    }

    /// Generate embeddings for a batch of texts via the IPC bridge.
    ///
    /// `model` is the provider-prefixed model string (e.g.
    /// `ollama/nomic-embed-text`). The zed process strips the
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
                ..Default::default()
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

        let line = match read_response_line(stream).await {
            Ok(line) => line,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Connection(format!("IPC read failed: {e}")));
            }
        };

        let Some(line) = line else {
            *guard = None;
            return Err(InferenceError::Connection(
                "IPC socket closed by server".into(),
            ));
        };

        let response: InferenceResponse = match serde_json::from_str(&line) {
            Ok(response) => response,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Json(format!("IPC deserialize failed: {e}")));
            }
        };

        if response.id != id {
            *guard = None;
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
            InferenceOutcome::ToolResult { .. } => Err(InferenceError::Connection(
                "received ToolResult outcome for a list_models request".into(),
            )),
            InferenceOutcome::SkillResult { .. } => Err(InferenceError::Connection(
                "received SkillResult outcome for a list_models request".into(),
            )),
            InferenceOutcome::WorktreeThread { .. } => Err(InferenceError::Connection(
                "received WorktreeThread outcome for a list_models request".into(),
            )),
        }
    }

    /// Send a media-generation request and receive the response.
    ///
    /// `params` carries the op-specific fields. The server-side dispatch
    /// reads only the fields relevant to each op.
        &self,
        op: &str,
    ) -> Result<serde_json::Value, InferenceError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = InferenceRequest {
            id,
            params: InferenceParams {
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
                media_language: params.language.clone(),
                ..Default::default()
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

        let line = match read_response_line(stream).await {
            Ok(line) => line,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Connection(format!("IPC read failed: {e}")));
            }
        };

        let Some(line) = line else {
            *guard = None;
            return Err(InferenceError::Connection(
                "IPC socket closed by server".into(),
            ));
        };

        let response: InferenceResponse = match serde_json::from_str(&line) {
            Ok(response) => response,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Json(format!("IPC deserialize failed: {e}")));
            }
        };

        if response.id != id {
            *guard = None;
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
            InferenceOutcome::ToolResult { .. } => Err(InferenceError::Connection(
                "received ToolResult outcome for a media request".into(),
            )),
            InferenceOutcome::SkillResult { .. } => Err(InferenceError::Connection(
                "received SkillResult outcome for a media request".into(),
            )),
            InferenceOutcome::WorktreeThread { .. } => Err(InferenceError::Connection(
                "received WorktreeThread outcome for a media request".into(),
            )),
        }
    }

    /// Generate media (image, video, speech, transcription) via the IPC bridge.
    ///
    /// `params` carries the op-specific fields. The zed process dispatches
        &self,
        op: &str,
    ) -> Result<serde_json::Value, InferenceError> {
    }

    /// Invoke a governed MCP tool on the zed side via the IPC bridge.
    ///
    /// name, `args` the JSON arguments. `allowed` is the caller's declared
    /// `server/tool` allowlist (the delegated agent's `mcp_tools`) — the zed
    /// side refuses any tool outside it before minting the OCAP panel token,
    /// so the allowlist is enforced at the dispatch boundary, not only inside
    /// the child. Returns the tool's JSON output.
    pub async fn invoke_tool(
        &self,
        server: &str,
        tool: &str,
        args: serde_json::Value,
        allowed: &[String],
    ) -> Result<serde_json::Value, InferenceError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = InferenceRequest {
            id,
            method: InferenceMethod::ToolInvoke,
            params: InferenceParams {
                tool_server: Some(server.to_string()),
                tool_name: Some(tool.to_string()),
                tool_args: Some(args),
                tool_allowlist: Some(allowed.to_vec()),
                ..Default::default()
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

        let line = match read_response_line(stream).await {
            Ok(line) => line,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Connection(format!("IPC read failed: {e}")));
            }
        };

        let Some(line) = line else {
            *guard = None;
            return Err(InferenceError::Connection(
                "IPC socket closed by server".into(),
            ));
        };

        let response: InferenceResponse = match serde_json::from_str(&line) {
            Ok(response) => response,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Json(format!("IPC deserialize failed: {e}")));
            }
        };

        if response.id != id {
            *guard = None;
            return Err(InferenceError::Connection(format!(
                "IPC ID mismatch: expected {id}, got {}",
                response.id
            )));
        }

        match response.outcome {
            InferenceOutcome::ToolResult { result } => Ok(result),
            InferenceOutcome::Error { error } => Err(error.into()),
            other => Err(InferenceError::Connection(format!(
                "received non-tool-invoke outcome for a tool-invoke request: {other:?}"
            ))),
        }
    }

    /// Execute an hKask skill cascade on the zed side via the IPC bridge.
    ///
    /// `name` is the skill id (e.g. "grill-me"), `task` the text the cascade
    /// acts on. The zed process runs the skill through its global
    /// `ManifestExecutor` (call-cap/OCAP enforcement on that side). Returns the
    /// cascade's final output text.
    pub async fn execute_skill(&self, name: &str, task: &str) -> Result<String, InferenceError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = InferenceRequest {
            id,
            method: InferenceMethod::SkillExecute,
            params: InferenceParams {
                skill_name: Some(name.to_string()),
                skill_task: Some(task.to_string()),
                ..Default::default()
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

        let line = match read_response_line(stream).await {
            Ok(line) => line,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Connection(format!("IPC read failed: {e}")));
            }
        };

        let Some(line) = line else {
            *guard = None;
            return Err(InferenceError::Connection(
                "IPC socket closed by server".into(),
            ));
        };

        let response: InferenceResponse = match serde_json::from_str(&line) {
            Ok(response) => response,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Json(format!("IPC deserialize failed: {e}")));
            }
        };

        if response.id != id {
            *guard = None;
            return Err(InferenceError::Connection(format!(
                "IPC ID mismatch: expected {id}, got {}",
                response.id
            )));
        }

        match response.outcome {
            InferenceOutcome::SkillResult { result } => Ok(result),
            InferenceOutcome::Error { error } => Err(error.into()),
            other => Err(InferenceError::Connection(format!(
                "received non-skill-execute outcome for a skill-execute request: {other:?}"
            ))),
        }
    }

    /// Create a sibling agent thread in a new git worktree workspace on the
    /// zed side. The thread runs in an isolated worktree (separate from the
    /// user's working tree). Used by `kanban_task_spawn` to isolate spawned
    /// agents (P1: worktree/terminal model). Returns the new thread's id and
    /// worktree path.
    pub async fn create_worktree_thread(
        &self,
        prompt: &str,
        title: &str,
        worktree_name: Option<&str>,
        base_ref: Option<&str>,
    ) -> Result<hkask_types::inference_ipc::WorktreeThreadInfo, InferenceError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = InferenceRequest {
            id,
            method: InferenceMethod::CreateWorktreeThread,
            params: InferenceParams {
                worktree_prompt: Some(prompt.to_string()),
                worktree_title: Some(title.to_string()),
                worktree_name: worktree_name.map(str::to_string),
                worktree_base_ref: base_ref.map(str::to_string),
                ..Default::default()
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

        let line = match read_response_line(stream).await {
            Ok(line) => line,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Connection(format!("IPC read failed: {e}")));
            }
        };
        let Some(line) = line else {
            *guard = None;
            return Err(InferenceError::Connection(
                "IPC socket closed by server".into(),
            ));
        };
        let response: InferenceResponse = match serde_json::from_str(&line) {
            Ok(response) => response,
            Err(e) => {
                *guard = None;
                return Err(InferenceError::Json(format!("IPC deserialize failed: {e}")));
            }
        };
        if response.id != id {
            *guard = None;
            return Err(InferenceError::Connection(format!(
                "IPC ID mismatch: expected {id}, got {}",
                response.id
            )));
        }
        match response.outcome {
            InferenceOutcome::WorktreeThread { thread } => Ok(thread),
            InferenceOutcome::Error { error } => Err(error.into()),
            other => Err(InferenceError::Connection(format!(
                "received non-worktree-thread outcome for a worktree-thread request: {other:?}"
            ))),
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
            parameters: parameters.clone(),
            tools: tools.map(|t| t.to_vec()),
            ..Default::default()
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
            parameters: parameters.clone(),
            model_override: model_override.map(|s| s.to_string()),
            tools: tools.map(|t| t.to_vec()),
            ..Default::default()
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
            messages: Some(messages.to_vec()),
            parameters: parameters.clone(),
            model_override: model_override.map(|s| s.to_string()),
            tools: tools.map(|t| t.to_vec()),
            ..Default::default()
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
            images: Some(images.to_vec()),
            parameters: parameters.clone(),
            model_override: model_override.map(|s| s.to_string()),
            ..Default::default()
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
        Box<
            dyn std::future::Future<Output = Result<Vec<hkask_types::ModelEntry>, InferenceError>>
                + Send
                + 'a,
        >,
    > {
        let this = self;
        Box::pin(async move {
            let entries = this.call_list_models().await.map_err(|e| {
                tracing::warn!(
                    target: "hkask.inference",
                    error = %e,
                    "IPC list_models failed — returning Err (not empty vec)"
                );
                InferenceError::Connection(format!("list_models IPC failed: {e}"))
            })?;
            Ok(entries
                .into_iter()
                .map(|e| {
                    let name = e.name.clone();
                    hkask_types::ModelEntry {
                        prefixed_name: name.clone(),
                        model: name.split('/').nth(1).unwrap_or(&name).to_string(),
                        supports_vision: e.supports_vision,
                    }
                })
                .collect())
        })
    }

        &'a self,
        op: &str,
        let op = op.to_string();
        let params = params.clone();
        let this = self;
    }
}

impl ToolDispatchPort for InferenceIpcClient {
    fn invoke_tool<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
        args: serde_json::Value,
        allowed: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn Future<Output = Result<serde_json::Value, InferenceError>> + Send + 'a>,
    > {
        let server = server.to_string();
        let tool = tool.to_string();
        let allowed = allowed.to_vec();
        Box::pin(async move { self.invoke_tool(&server, &tool, args, &allowed).await })
    }
}

impl SkillExecPort for InferenceIpcClient {
    fn execute_skill<'a>(
        &'a self,
        name: &'a str,
        task: &'a str,
    ) -> std::pin::Pin<
        Box<dyn Future<Output = Result<String, hkask_types::SkillExecError>> + Send + 'a>,
    > {
        let name = name.to_string();
        let task = task.to_string();
        Box::pin(async move {
            self.execute_skill(&name, &task)
                .await
                .map_err(hkask_types::SkillExecError::from)
        })
    }
}

impl hkask_types::WorktreeSpawnPort for InferenceIpcClient {
    fn create_worktree_thread<'a>(
        &'a self,
        prompt: &'a str,
        title: &'a str,
        worktree_name: Option<&'a str>,
        base_ref: Option<&'a str>,
    ) -> std::pin::Pin<
        Box<dyn Future<Output = Result<String, hkask_types::InferenceError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let info = self
                .create_worktree_thread(prompt, title, worktree_name, base_ref)
                .await?;
            Ok(info.message)
        })
    }
}
