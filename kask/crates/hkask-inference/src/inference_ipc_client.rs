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
//! The client opens a **new connection per request**. This allows concurrent
//! requests to run in parallel — the server side already spawns a task per
//! connection (`handle_connection`), so multiple in-flight requests are
//! handled independently. Unix domain socket `connect()` is a kernel-level
//! operation with no network round-trip, so the per-request overhead is
//! negligible (microseconds) compared to the inference call itself (seconds).
//!
//! The previous design held a single `Mutex<UnixStream>` which serialized all
//! requests — even with concurrent `tokio::spawn` tasks, only one request
//! could be in flight at a time, making parallel embedding of large corpora
//! impractical.
//!
//! ## Why not streaming?
//!
//! Streaming is not supported over IPC — the server side collects the stream
//! and returns a single `InferenceResult`. This matches the existing
//! `LanguageModelInferencePort` pattern and is sufficient for MCP server use
//! cases (OCR, classification, summarization, etc.).
//!
//! ## Transport vs. outcome errors
//!
//! Every IPC method shares one transport skeleton — open a connection,
//! serialize the request, write + flush, read the response line, deserialize,
//! verify the correlation id — owned by [`InferenceIpcClient::ipc_roundtrip`].
//! Transport failures (a dead socket, a malformed line, an id mismatch) are
//! [`IpcTransportError`]s, mapped to each method's error type by a `From` impl.
//! What the method then does with the validated [`InferenceResponse`] — which
//! [`InferenceOutcome`] variant it expected — is irreducibly per-method, so
//! the outcome match stays at the call site. The matches are exhaustive (every
//! variant named) so that adding a new `InferenceOutcome` variant is a
//! compile error in every caller, not a silent fall-through.

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
    InferenceResult, ToolDispatchPort,
};
use std::future::Future;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

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

/// A transport-layer failure in the IPC roundtrip — distinct from an
/// [`InferenceOutcome::Error`] returned by the bridge. [`ipc_roundtrip`]
/// returns this; each method maps it to its own error type via a `From` impl,
/// so the transport skeleton stays error-type-agnostic.
///
/// [`ipc_roundtrip`]: InferenceIpcClient::ipc_roundtrip
#[derive(Debug)]
enum IpcTransportError {
    /// Serialize or deserialize failure — maps to the `Json` variant of the
    /// method's error type.
    Json(String),
    /// Connection-level failure (write/flush/read/closed/id-mismatch) — maps to
    /// the `Connection` variant of the method's error type.
    Connection(String),
}

impl From<IpcTransportError> for InferenceError {
    fn from(e: IpcTransportError) -> Self {
        match e {
            IpcTransportError::Json(m) => InferenceError::Json(m),
            IpcTransportError::Connection(m) => InferenceError::Connection(m),
        }
    }
}

impl From<IpcTransportError> for EmbeddingGenerationError {
    fn from(e: IpcTransportError) -> Self {
        match e {
            IpcTransportError::Json(m) => EmbeddingGenerationError::Json(m),
            IpcTransportError::Connection(m) => EmbeddingGenerationError::Connection(m),
        }
    }
}

/// Format the diagnostic message for an [`InferenceOutcome`] variant that the
/// request did not expect. Centralizes the "unexpected outcome" wording so the
/// per-method outcome matches stay exhaustive (every variant named) without
/// duplicating the message string across five call sites. Takes `method` by
/// reference so the caller can keep its owned value for the match arms.
fn unexpected_outcome_msg(method: &InferenceMethod, variant: &'static str) -> String {
    format!("received {variant} outcome for a {method:?} request")
}

/// Strip the provider prefix (the first `/`-segment) from a model id.
///
/// `"OpenRouter/z-ai/glm-5.2"` → `"z-ai/glm-5.2"`; `"ollama/qwen3-embedding:0.6b"`
/// → `"qwen3-embedding:0.6b"`; `"no-slash"` → `"no-slash"`. Used by `list_models`
/// to produce the `ModelEntry.model` ("raw model name without prefix") from
/// the bridge's `ModelListEntry.name` ("full name with provider prefix").
///
/// Strips only the first segment: a model id may itself contain a slash
/// (OpenRouter's `vendor/model` convention), so `split_once` is correct where
/// the prior `split('/').nth(1)` truncated `OpenRouter/z-ai/glm-5.2` to `z-ai`.
fn strip_provider_prefix(name: &str) -> &str {
    match name.split_once('/') {
        Some((_, rest)) => rest,
        None => name,
    }
}

/// An `InferencePort` that delegates to a Unix socket connection back to zed.
///
/// Construct with `InferenceIpcClient::connect()` or
/// `InferenceIpcClient::from_env()`.
///
/// Opens a new connection per request so concurrent callers are not
/// serialized behind a single stream lock. The server side spawns a task
/// per connection, so parallel requests are handled independently.
#[derive(Clone)]
pub struct InferenceIpcClient {
    /// The socket path — each `ipc_roundtrip` opens a fresh connection here.
    socket_path: Arc<std::path::PathBuf>,
    /// Next request ID. Shared across clones so each request gets a unique id.
    next_id: Arc<AtomicU64>,
}

impl InferenceIpcClient {
    /// Connect to a Unix socket at the given path.
    ///
    /// Verifies the socket is reachable by opening and immediately closing a
    /// test connection. The actual socket path is stored for per-request
    /// connections in `ipc_roundtrip`.
    pub async fn connect(socket_path: &Path) -> Result<Self, InferenceError> {
        // Verify the socket is reachable — open a connection and drop it.
        let _ = UnixStream::connect(socket_path)
            .await
            .map_err(|e| InferenceError::Connection(format!("IPC connect failed: {e}")))?;
        Ok(Self {
            socket_path: Arc::new(socket_path.to_path_buf()),
            next_id: Arc::new(AtomicU64::new(1)),
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

    /// Send a request and receive the validated response.
    ///
    /// Opens a **new connection** to the IPC socket for each call, so
    /// concurrent callers are not serialized behind a single stream lock.
    /// The server side spawns a task per connection, so parallel requests
    /// are handled independently. Unix domain socket `connect()` is a
    /// kernel-level operation with no network round-trip — the overhead is
    /// negligible compared to the inference call itself.
    ///
    /// Takes `method` by reference (cloning once for the wire request) so the
    /// caller keeps its owned value for the per-method outcome match. The
    /// outcome classification is irreducibly per-method — each expects a
    /// different `InferenceOutcome` variant and success type — so it lives at
    /// the call site.
    async fn ipc_roundtrip(
        &self,
        method: &InferenceMethod,
        params: InferenceParams,
    ) -> Result<InferenceResponse, IpcTransportError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = InferenceRequest {
            id,
            method: method.clone(),
            params,
        };
        let request_json = serde_json::to_string(&request)
            .map_err(|e| IpcTransportError::Json(format!("IPC serialize failed: {e}")))?;

        // Open a fresh connection for this request. Each connection is
        // handled by its own server-side task, so concurrent callers run
        // in parallel.
        let mut stream = UnixStream::connect(&*self.socket_path)
            .await
            .map_err(|e| IpcTransportError::Connection(format!("IPC connect failed: {e}")))?;

        // Send the request as a single line.
        stream
            .write_all(request_json.as_bytes())
            .await
            .map_err(|e| IpcTransportError::Connection(format!("IPC write failed: {e}")))?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|e| IpcTransportError::Connection(format!("IPC write failed: {e}")))?;
        stream
            .flush()
            .await
            .map_err(|e| IpcTransportError::Connection(format!("IPC flush failed: {e}")))?;

        let line = read_response_line(&mut stream)
            .await
            .map_err(|e| IpcTransportError::Connection(format!("IPC read failed: {e}")))?;
        let line = match line {
            Some(line) => line,
            None => {
                return Err(IpcTransportError::Connection(
                    "IPC socket closed by server".into(),
                ));
            }
        };

        let response: InferenceResponse = serde_json::from_str(&line)
            .map_err(|e| IpcTransportError::Json(format!("IPC deserialize failed: {e}")))?;

        if response.id != id {
            return Err(IpcTransportError::Connection(format!(
                "IPC ID mismatch: expected {id}, got {}",
                response.id
            )));
        }

        Ok(response)
    }

    /// Send a generate request and return the result.
    async fn call(
        &self,
        method: InferenceMethod,
        params: InferenceParams,
    ) -> Result<InferenceResult, InferenceError> {
        let response = self.ipc_roundtrip(&method, params).await?;
        match response.outcome {
            InferenceOutcome::Result { result } => Ok(result),
            InferenceOutcome::Error { error } => Err(error.into()),
            InferenceOutcome::Embeddings { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "Embeddings"),
            )),
            InferenceOutcome::ModelList { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "ModelList"),
            )),
            InferenceOutcome::Media { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "Media"),
            )),
            InferenceOutcome::ToolResult { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "ToolResult"),
            )),
            InferenceOutcome::WorktreeThread { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "WorktreeThread"),
            )),
        }
    }

    /// Send an embedding request and receive the response.
    async fn call_embed(
        &self,
        model: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, EmbeddingGenerationError> {
        let method = InferenceMethod::Embed;
        let params = InferenceParams {
            embed_model: Some(model.to_string()),
            embed_texts: Some(texts.to_vec()),
            ..Default::default()
        };
        let response = self.ipc_roundtrip(&method, params).await?;
        match response.outcome {
            InferenceOutcome::Embeddings { embeddings } => Ok(embeddings),
            InferenceOutcome::Error { error } => Err(EmbeddingGenerationError::Connection(
                format!("{}: {}", error.code, error.message),
            )),
            InferenceOutcome::Result { .. } => Err(EmbeddingGenerationError::Connection(
                unexpected_outcome_msg(&method, "Result"),
            )),
            InferenceOutcome::ModelList { .. } => Err(EmbeddingGenerationError::Connection(
                unexpected_outcome_msg(&method, "ModelList"),
            )),
            InferenceOutcome::Media { .. } => Err(EmbeddingGenerationError::Connection(
                unexpected_outcome_msg(&method, "Media"),
            )),
            InferenceOutcome::ToolResult { .. } => Err(EmbeddingGenerationError::Connection(
                unexpected_outcome_msg(&method, "ToolResult"),
            )),
            InferenceOutcome::WorktreeThread { .. } => Err(EmbeddingGenerationError::Connection(
                unexpected_outcome_msg(&method, "WorktreeThread"),
            )),
        }
    }

    /// Generate embeddings for a batch of texts via the IPC bridge.
    ///
    /// `model` is the provider-prefixed model string (e.g.
    /// `DEFAULT_EMBEDDING_MODEL`). The zed process strips the
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
        let method = InferenceMethod::ListModels;
        let response = self
            .ipc_roundtrip(&method, InferenceParams::default())
            .await?;
        match response.outcome {
            InferenceOutcome::ModelList { models } => Ok(models),
            InferenceOutcome::Error { error } => Err(error.into()),
            InferenceOutcome::Result { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "Result"),
            )),
            InferenceOutcome::Embeddings { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "Embeddings"),
            )),
            InferenceOutcome::Media { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "Media"),
            )),
            InferenceOutcome::ToolResult { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "ToolResult"),
            )),
            InferenceOutcome::WorktreeThread { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "WorktreeThread"),
            )),
        }
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
        let method = InferenceMethod::ToolInvoke;
        let params = InferenceParams {
            tool_server: Some(server.to_string()),
            tool_name: Some(tool.to_string()),
            tool_args: Some(args),
            tool_allowlist: Some(allowed.to_vec()),
            ..Default::default()
        };
        let response = self.ipc_roundtrip(&method, params).await?;
        match response.outcome {
            InferenceOutcome::ToolResult { result } => Ok(result),
            InferenceOutcome::Error { error } => Err(error.into()),
            InferenceOutcome::Result { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "Result"),
            )),
            InferenceOutcome::Embeddings { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "Embeddings"),
            )),
            InferenceOutcome::ModelList { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "ModelList"),
            )),
            InferenceOutcome::Media { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "Media"),
            )),
            InferenceOutcome::WorktreeThread { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "WorktreeThread"),
            )),
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
        let method = InferenceMethod::CreateWorktreeThread;
        let params = InferenceParams {
            worktree_prompt: Some(prompt.to_string()),
            worktree_title: Some(title.to_string()),
            worktree_name: worktree_name.map(str::to_string),
            worktree_base_ref: base_ref.map(str::to_string),
            ..Default::default()
        };
        let response = self.ipc_roundtrip(&method, params).await?;
        match response.outcome {
            InferenceOutcome::WorktreeThread { thread } => Ok(thread),
            InferenceOutcome::Error { error } => Err(error.into()),
            InferenceOutcome::Result { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "Result"),
            )),
            InferenceOutcome::Embeddings { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "Embeddings"),
            )),
            InferenceOutcome::ModelList { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "ModelList"),
            )),
            InferenceOutcome::Media { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "Media"),
            )),
            InferenceOutcome::ToolResult { .. } => Err(InferenceError::Connection(
                unexpected_outcome_msg(&method, "ToolResult"),
            )),
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
                        model: strip_provider_prefix(&name).to_string(),
                        supports_vision: e.supports_vision,
                    }
                })
                .collect())
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    /// Test harness: binds a Unix listener, returns the socket path and a
    /// handle that accepts one connection and feeds it a canned response.
    ///
    /// The client opens a fresh connection per `ipc_roundtrip` call, so each
    /// test spawns a listener that accepts exactly one connection, writes the
    /// pre-buffered response, and closes.
    struct TestBridge {
        path: std::path::PathBuf,
        _dir: tempfile::TempDir,
    }

    impl TestBridge {
        /// Create a test bridge that accepts one connection and writes
        /// `response_bytes` to it.
        fn with_response(response_bytes: Vec<u8>) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("test.sock");
            let listener = UnixListener::bind(&path).unwrap();
            tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                stream.write_all(&response_bytes).await.unwrap();
                // Keep the stream open until the client reads — don't drop
                // immediately. The client's `read_response_line` will get
                // the data before EOF.
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            });
            Self { path, _dir: dir }
        }

        /// Create a client connected to this bridge's socket.
        fn client(&self) -> InferenceIpcClient {
            InferenceIpcClient {
                socket_path: Arc::new(self.path.clone()),
                next_id: Arc::new(AtomicU64::new(1)),
            }
        }
    }

    /// Serialize a newline-terminated response line for the bridge end.
    fn response_line(outcome: InferenceOutcome, id: u64) -> String {
        let resp = InferenceResponse { id, outcome };
        serde_json::to_string(&resp).unwrap() + "\n"
    }

    #[test]
    fn strip_provider_prefix_strips_first_segment_only() {
        // The bug at the old inline `split('/').nth(1)`: this returned "z-ai".
        assert_eq!(
            strip_provider_prefix("OpenRouter/z-ai/glm-5.2"),
            "z-ai/glm-5.2"
        );
        assert_eq!(
            strip_provider_prefix("ollama/qwen3-embedding:0.6b"),
            "qwen3-embedding:0.6b"
        );
        assert_eq!(strip_provider_prefix("no-slash"), "no-slash");
        assert_eq!(strip_provider_prefix("/leading-slash"), "leading-slash");
        assert_eq!(strip_provider_prefix("trailing-slash/"), "");
    }

    #[test]
    fn unexpected_outcome_msg_names_method_and_variant() {
        let msg = unexpected_outcome_msg(&InferenceMethod::Generate, "Embeddings");
        assert!(
            msg.contains("Embeddings"),
            "msg should name the variant: {msg}"
        );
        assert!(
            msg.contains("Generate"),
            "msg should name the method: {msg}"
        );
    }

    #[tokio::test]
    async fn ipc_roundtrip_returns_validated_response() {
        let bridge = TestBridge::with_response(
            response_line(
                InferenceOutcome::ToolResult {
                    result: serde_json::Value::Null,
                },
                1,
            )
            .into_bytes(),
        );
        let client = bridge.client();
        let response = client
            .ipc_roundtrip(&InferenceMethod::ToolInvoke, InferenceParams::default())
            .await
            .expect("happy path returns the validated response");
        assert_eq!(response.id, 1);
        assert!(matches!(
            response.outcome,
            InferenceOutcome::ToolResult { .. }
        ));
    }

    #[tokio::test]
    async fn ipc_roundtrip_returns_connection_error_on_dead_socket() {
        // A path that doesn't exist — connect will fail.
        let client = InferenceIpcClient {
            socket_path: Arc::new(std::path::PathBuf::from("/nonexistent/ipc/sock")),
            next_id: Arc::new(AtomicU64::new(1)),
        };
        let err = client
            .ipc_roundtrip(&InferenceMethod::Generate, InferenceParams::default())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            IpcTransportError::Connection(ref m) if m.contains("IPC connect failed")
        ));
    }

    #[tokio::test]
    async fn ipc_roundtrip_returns_error_on_id_mismatch() {
        let bridge = TestBridge::with_response(
            response_line(
                InferenceOutcome::ToolResult {
                    result: serde_json::Value::Null,
                },
                999, // wrong id — client expects 1
            )
            .into_bytes(),
        );
        let client = bridge.client();
        let err = client
            .ipc_roundtrip(&InferenceMethod::Generate, InferenceParams::default())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            IpcTransportError::Connection(ref m) if m.contains("ID mismatch")
        ));
    }

    #[tokio::test]
    async fn ipc_roundtrip_returns_error_on_malformed_json() {
        let bridge = TestBridge::with_response(b"not valid json\n".to_vec());
        let client = bridge.client();
        let err = client
            .ipc_roundtrip(&InferenceMethod::Generate, InferenceParams::default())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            IpcTransportError::Json(ref m) if m.contains("deserialize failed")
        ));
    }

    #[tokio::test]
    async fn call_rejects_unexpected_outcome() {
        let bridge = TestBridge::with_response(
            response_line(
                InferenceOutcome::Embeddings {
                    embeddings: vec![vec![0.0]],
                },
                1,
            )
            .into_bytes(),
        );
        let client = bridge.client();
        // `call` with `Generate` expects `Result`; feed `Embeddings`.
        let err = client
            .call(InferenceMethod::Generate, InferenceParams::default())
            .await
            .unwrap_err();
        assert!(
            matches!(err, InferenceError::Connection(ref m) if m.contains("Embeddings") && m.contains("Generate")),
            "unexpected-outcome error should name both the variant and the method: {err:?}"
        );
    }
}
