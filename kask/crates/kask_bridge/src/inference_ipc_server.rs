//! `InferenceIpcServer` — the zed-side listener that serves inference requests
//! from MCP server child processes over a Unix socket.
//!
//! When zed launches an MCP server, it creates a Unix socket, starts this
//! server listening on it, and passes the socket path to the child process
//! via the `HKASK_INFERENCE_SOCKET` env var. The MCP server connects and
//! sends inference requests; this server dispatches them to zed's
//! `InferencePort` (which uses `LanguageModelRegistry` with guard,
//! and zed's configured API keys).
//!
//! ## Architecture
//!
//! ```text
//! zed process
//!   ├── InferenceIpcServer (Unix socket listener)
//!   │     └── dispatches to Arc<dyn InferencePort>
//!   │           └── GuardedInferencePort → zed's LanguageModelRegistry
//!   │
//!   └── spawns MCP server child process
//!         └── InferenceIpcClient (connects to the socket)
//!               └── implements InferencePort
//! ```
//!
//! ## Connection handling
//!
//! Each MCP server gets its own socket. The server accepts connections in a
//! background task and handles each connection in its own task. Requests are
//! processed sequentially per connection (the protocol is request-response).

use std::path::PathBuf;
use std::sync::Arc;

use hkask_types::inference_ipc::{
    InferenceErrorPayload, InferenceMethod, InferenceOutcome, InferenceRequest, InferenceResponse,
    ModelListEntry,
};
use hkask_types::{InferenceError, InferencePort, InferenceResult};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::oneshot;

use crate::inference::LanguageModelEmbeddingPort;

/// The zed-side inference IPC server.
///
/// Listens on a Unix socket and dispatches inference requests to the
/// provided `InferencePort`. Each connection is handled in its own task.
pub struct InferenceIpcServer {
    /// The socket path — passed to MCP server child processes via env var.
    socket_path: PathBuf,
    /// The background listener task.
    _task: tokio::task::JoinHandle<()>,
    _list_models_task: gpui::Task<()>,
}

/// Maximum size of a single newline-delimited IPC message.
///
/// Requests carry base64-encoded images and multi-message transcripts, so
/// this is generous; 16 MiB caps unbounded `read_line` growth (CWE-400).
/// Duplicated in `hkask-inference/src/inference_ipc_client.rs` because the
/// shared types crate is owned by another workstream.
const MAX_IPC_LINE_BYTES: u64 = 16 * 1024 * 1024;

/// Error returned by [`CappedReader::read_line`] when the peer sends more
/// than `MAX_IPC_LINE_BYTES` without a newline.
const LINE_TOO_LONG: &str = "IPC line exceeds MAX_IPC_LINE_BYTES";

/// A connection-side reader that hands out one capped line at a time.
struct CappedReader<R> {
    inner: BufReader<R>,
}

impl<R: tokio::io::AsyncRead + Unpin> CappedReader<R> {
    fn new(reader: R) -> Self {
        Self {
            inner: BufReader::new(reader),
        }
    }

    /// Read one newline-delimited message, capped at `MAX_IPC_LINE_BYTES`.
    ///
    /// Returns `Ok(None)` on clean EOF before any bytes. An oversized line
    /// is an error; the connection is unusable afterward because the
    /// buffered remainder cannot be re-synchronized to a message boundary.
    async fn read_line(&mut self) -> Result<Option<String>, std::io::Error> {
        let mut line = String::new();
        // Read at most cap+1 bytes so a line of exactly cap bytes followed by
        // a newline is accepted, but anything longer is detected.
        let mut capped = (&mut self.inner).take(MAX_IPC_LINE_BYTES + 1);
        let bytes_read = capped.read_line(&mut line).await?;
        if bytes_read == 0 {
            return Ok(None);
        }
        if !line.ends_with('\n') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                LINE_TOO_LONG,
            ));
        }
        line.pop();
        Ok(Some(line))
    }
}

/// The directory inference IPC sockets live in. Private to the current user
/// (mode 0700) so other local users cannot reach the socket — the socket
/// drives LLM calls billed to the operator's API keys.
fn inference_socket_dir() -> Result<PathBuf, std::io::Error> {
    let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime_dir) if !runtime_dir.is_empty() => PathBuf::from(runtime_dir).join("kask"),
        _ => {
            let uid = own_uid();
            std::env::temp_dir().join(format!("kask-inference-{uid}"))
        }
    };

    ensure_private_dir(&dir)?;
    Ok(dir)
}

/// Our real uid without `unsafe`/`libc` (both forbidden in hkask crates).
/// std exposes uid only through `MetadataExt` on files, so on Linux we read
/// the owner of `/proc/self`. Elsewhere there is no std-only source; the
/// fallback directory name uses 0, which is still per-user-private because
/// the directory is created mode 0700.
#[cfg(target_os = "linux")]
fn own_uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self")
        .map(|metadata| metadata.uid())
        .unwrap_or(0)
}

#[cfg(not(target_os = "linux"))]
fn own_uid() -> u32 {
    0
}

/// Create `dir` (and parents) with mode 0700, or tighten an existing dir to
/// 0700. Fails rather than proceeding with a world-accessible directory.
fn ensure_private_dir(dir: &std::path::Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(dir).or_else(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            Ok(())
        } else {
            Err(e)
        }
    })?;

    let current = std::fs::metadata(dir)?.permissions().mode() & 0o777;
    if current != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
            std::io::Error::other(format!(
                "inference socket dir {} exists with mode {current:o} and chmod to 0700 failed: {e}",
                dir.display()
            ))
        })?;
    }
    Ok(())
}

/// Verify the connecting peer is the same unix user as this process.
///
/// On Linux, `SO_PEERCRED` (via tokio's safe `peer_cred()`) gives the peer
/// uid; a mismatch is logged and rejected. On other unix platforms tokio has
/// no safe peer-credential API, so we warn and rely on the 0700 directory +
/// 0600 socket file as the access-control boundary.
#[cfg(target_os = "linux")]
fn peer_is_owner(stream: &tokio::net::UnixStream) -> bool {
    match stream.peer_cred() {
        Ok(cred) if cred.uid() == own_uid() => true,
        Ok(cred) => {
            tracing::warn!(
                target: "reg.inference",
                peer_uid = cred.uid(),
                "Inference IPC rejected connection from different uid"
            );
            false
        }
        Err(e) => {
            tracing::warn!(
                target: "reg.inference",
                error = %e,
                "Inference IPC peer_cred failed — rejecting connection"
            );
            false
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn peer_is_owner(_stream: &tokio::net::UnixStream) -> bool {
    // Warn once, not per connection — an accept loop would otherwise emit a
    // warn storm for a platform property that never changes at runtime.
    static WARN_ONCE: std::sync::Once = std::sync::Once::new();
    WARN_ONCE.call_once(|| {
        tracing::warn!(
            target: "reg.inference",
            "Inference IPC peer-credential check unavailable on this platform — relying on filesystem permissions"
        );
    });
    true
}

impl InferenceIpcServer {
    /// Start listening on a new Unix socket.
    ///
    /// The socket path is randomly generated inside a per-user private
    /// directory. The socket is removed when the server is dropped.
    ///
    /// `inference_port` is the port to dispatch chat requests to (typically
    /// the `GuardedInferencePort` wrapping `LanguageModelInferencePort`).
    /// `embedding_port` is the port to dispatch embedding requests to (the
    /// `LanguageModelEmbeddingPort`). When `None`, `embed` requests return an
    /// error.
    /// `media_router` is the hKask `MediaRouter` used for media generation
    /// (image, video, speech, transcription via fal.ai/DeepInfra). When `None`,.
    /// `media_generate` requests return an error.
    /// `tool_port` is the governed `McpRuntime` (as `ToolPort`) used for
    /// `tool_invoke` requests from MCP servers that run agent loops (e.g.
    /// `hkask-mcp-swarm`'s local delegate). When `None`, `tool_invoke`
    /// requests return an error. The zed side mints the OCAP panel token —
    /// the child process never holds token material.
    /// `skill_exec_port` runs `skill_execute` requests through the zed-side
    /// `ManifestExecutor` (its own gas/OCAP enforcement). When `None`,
    /// `skill_execute` requests return an error.
    pub fn start(
        inference_port: Arc<dyn InferencePort>,
        embedding_port: Option<LanguageModelEmbeddingPort>,
        media_router: Option<Arc<hkask_inference::MediaRouter>>,
        tool_port: Option<Arc<dyn hkask_capability::ToolPort>>,
        skill_exec_port: Option<Arc<dyn hkask_types::SkillExecPort>>,
        cx: &gpui::App,
    ) -> Result<Self, std::io::Error> {
        // Generate a unique socket path inside a per-user private directory
        // so other local users cannot connect and spend the operator's API
        // quota.
        let socket_path = generate_socket_path()?;

        // Bind the listener on the tokio runtime (via gpui_tokio, not GPUI's
        // background executor — UnixListener::bind and accept require a tokio
        // reactor, and GPUI's executor is not tokio).
        let tokio_handle = gpui_tokio::Tokio::handle(cx);

        // Use a oneshot channel to get the bind result synchronously.
        let (tx, rx) = std::sync::mpsc::channel();
        let socket_path_for_bind = socket_path.clone();
        tokio_handle.spawn(async move {
            // Remove any stale socket file.
            let _ = std::fs::remove_file(&socket_path_for_bind);
            let result = UnixListener::bind(&socket_path_for_bind);
            let _ = tx.send(result);
        });

        let listener = rx
            .recv()
            .map_err(|e| std::io::Error::other(format!("IPC socket bind channel failed: {e}")))?
            .map_err(|e| {
                std::io::Error::other(format!("Failed to bind inference IPC socket: {e}"))
            })?;

        // Belt-and-braces: the parent dir is 0700, but also pin the socket
        // itself to owner-only in case the dir mode is ever relaxed.
        std::fs::set_permissions(
            &socket_path,
            <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o600),
        )
        .map_err(|e| {
            std::io::Error::other(format!(
                "Failed to set 0600 on inference IPC socket {}: {e}",
                socket_path.display()
            ))
        })?;

        let port = inference_port.clone();
        let emb_port = embedding_port.clone();
        let media = media_router.clone();
        let tools = tool_port.clone();
        let skill_exec = skill_exec_port.clone();

        // Spawn a GPUI-side task for ListModels requests. `AsyncApp` is not
        // `Send`, so we can't pass it into tokio::spawn. Instead, this task
        // holds the `AsyncApp` and responds to channel requests — the same
        // pattern as `LanguageModelEmbeddingPort`.
        let (list_models_tx, mut list_models_rx) = tokio::sync::mpsc::unbounded_channel::<(
            tokio::sync::oneshot::Sender<Vec<ModelListEntry>>,
        )>();
        let list_models_task = cx.spawn(async move |cx| {
            while let Some(reply) = list_models_rx.recv().await {
                let result = cx.update(|cx| {
                    let registry = language_model::LanguageModelRegistry::read_global(cx);
                    registry
                        .providers()
                        .into_iter()
                        .flat_map(|provider| {
                            let provider_id = provider.id().0.clone();
                            provider.provided_models(cx).into_iter().map(move |model| {
                                ModelListEntry {
                                    name: format!("{}/{}", provider_id, model.name().0),
                                    provider: provider_id.to_string(),
                                    supports_vision: model.supports_images(),
                                }
                            })
                        })
                        .collect::<Vec<_>>()
                });
                let _ = reply.0.send(result);
            }
        });

        let list_models_tx = Arc::new(list_models_tx);
        let task = tokio_handle.spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let port = port.clone();
                        let emb_port = emb_port.clone();
                        let media = media.clone();
                        let tools = tools.clone();
                        let skill_exec = skill_exec.clone();
                        let list_models_tx = list_models_tx.clone();
                        tokio::spawn(async move {
                            handle_connection(
                                stream,
                                port,
                                emb_port,
                                media,
                                tools,
                                skill_exec,
                                list_models_tx,
                            )
                            .await;
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.inference",
                            error = %e,
                            "Inference IPC accept failed — stopping listener"
                        );
                        break;
                    }
                }
            }
        });

        Ok(Self {
            socket_path,
            _task: task,
            _list_models_task: list_models_task,
        })
    }

    /// The socket path — pass this to MCP server child processes via the
    /// `HKASK_INFERENCE_SOCKET` env var.
    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }
}

impl Drop for InferenceIpcServer {
    fn drop(&mut self) {
        // Clean up the socket file.
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Generate a unique Unix socket path inside the per-user private socket
/// directory (see [`inference_socket_dir`]).
fn generate_socket_path() -> Result<PathBuf, std::io::Error> {
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Ok(inference_socket_dir()?.join(format!("kask-inference-{pid}-{nonce}.sock")))
}

/// Handle a single connection from an MCP server.
///
/// Reads newline-delimited JSON requests, dispatches them to the inference
/// port, and writes newline-delimited JSON responses.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    port: Arc<dyn InferencePort>,
    embedding_port: Option<LanguageModelEmbeddingPort>,
    media_router: Option<Arc<hkask_inference::MediaRouter>>,
    tool_port: Option<Arc<dyn hkask_capability::ToolPort>>,
    skill_exec_port: Option<Arc<dyn hkask_types::SkillExecPort>>,
    list_models_tx: Arc<
        tokio::sync::mpsc::UnboundedSender<(tokio::sync::oneshot::Sender<Vec<ModelListEntry>>,)>,
    >,
) {
    if !peer_is_owner(&stream) {
        return;
    }

    let (reader, mut writer) = stream.into_split();
    let mut reader = CappedReader::new(reader);

    loop {
        let line = match reader.read_line().await {
            Ok(None) => {
                // Connection closed.
                break;
            }
            Ok(Some(line)) => line,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                tracing::warn!(
                    target: "reg.inference",
                    "Inference IPC line exceeded {MAX_IPC_LINE_BYTES} bytes — closing connection"
                );
                break;
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.inference",
                    error = %e,
                    "Inference IPC read failed — closing connection"
                );
                break;
            }
        };

        let request: InferenceRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    target: "reg.inference",
                    error = %e,
                    line = %line,
                    "Inference IPC parse failed — skipping"
                );
                continue;
            }
        };

        let id = request.id;
        let outcome = dispatch(
            &port,
            embedding_port.as_ref(),
            media_router.as_ref(),
            tool_port.as_ref(),
            skill_exec_port.as_ref(),
            &list_models_tx,
            request,
        )
        .await;

        let response = InferenceResponse { id, outcome };
        let response_json = match serde_json::to_string(&response) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    target: "reg.inference",
                    error = %e,
                    "Inference IPC response serialize failed — skipping"
                );
                continue;
            }
        };

        if let Err(e) = writer.write_all(response_json.as_bytes()).await {
            tracing::warn!(
                target: "reg.inference",
                error = %e,
                "Inference IPC write failed — closing connection"
            );
            break;
        }
        if let Err(e) = writer.write_all(b"\n").await {
            tracing::warn!(
                target: "reg.inference",
                error = %e,
                "Inference IPC write failed — closing connection"
            );
            break;
        }
        if let Err(e) = writer.flush().await {
            tracing::warn!(
                target: "reg.inference",
                error = %e,
                "Inference IPC flush failed — closing connection"
            );
            break;
        }
    }
}

/// Dispatch a single request to the inference port.
async fn dispatch(
    port: &Arc<dyn InferencePort>,
    embedding_port: Option<&LanguageModelEmbeddingPort>,
    media_router: Option<&Arc<hkask_inference::MediaRouter>>,
    tool_port: Option<&Arc<dyn hkask_capability::ToolPort>>,
    skill_exec_port: Option<&Arc<dyn hkask_types::SkillExecPort>>,
    list_models_tx: &Arc<
        tokio::sync::mpsc::UnboundedSender<(tokio::sync::oneshot::Sender<Vec<ModelListEntry>>,)>,
    >,
    request: InferenceRequest,
) -> InferenceOutcome {
    let params = request.params;

    // Embedding requests are dispatched separately — they return
    // `InferenceOutcome::Embeddings`, not `InferenceOutcome::Result`.
    if matches!(request.method, InferenceMethod::Embed) {
        let Some(emb_port) = embedding_port else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Connection".to_string(),
                    message: "embedding port not configured on the zed side \
                        — the IPC server was started without an embedding port. \
                        This indicates a startup wiring bug."
                        .to_string(),
                },
            };
        };
        let model = params.embed_model.as_deref().unwrap_or("");
        let texts = params.embed_texts.as_deref().unwrap_or(&[]);
        return match emb_port.embed(model, texts).await {
            Ok(embeddings) => InferenceOutcome::Embeddings { embeddings },
            Err(e) => InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Connection".to_string(),
                    message: e.to_string(),
                },
            },
        };
    }

    // ListModels requests are dispatched via the GPUI context — they read
    // zed's `LanguageModelRegistry` directly (not through InferencePort).
    if matches!(request.method, InferenceMethod::ListModels) {
        let (tx_reply, rx_reply) = oneshot::channel::<Vec<ModelListEntry>>();
        if list_models_tx.send((tx_reply,)).is_err() {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Connection".to_string(),
                    message: "GPUI-side list_models task dropped — server shutting down"
                        .to_string(),
                },
            };
        }
        match rx_reply.await {
            Ok(models) => return InferenceOutcome::ModelList { models },
            Err(e) => {
                return InferenceOutcome::Error {
                    error: InferenceErrorPayload {
                        code: "Connection".to_string(),
                        message: format!("list_models channel failed: {e}"),
                    },
                };
            }
        }
    }

    // Media generation requests are dispatched to the hKask `MediaRouter`,
    // which holds the fal.ai/DeepInfra backends. Unlike `ListModels`, the
    // `MediaRouter` is `Send + Sync` and needs no GPUI access, so it can
    // be called directly from the tokio task.
    if matches!(request.method, InferenceMethod::MediaGenerate) {
        let Some(media) = media_router else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Connection".to_string(),
                    message: "media router not configured on the zed side \
                        — the IPC server was started without a media router. \
                        This indicates a startup wiring bug."
                        .to_string(),
                },
            };
        };
        let op = params.media_op.as_deref().unwrap_or("");
        let result = dispatch_media(media, op, &params).await;
        return match result {
            Ok(value) => InferenceOutcome::Media { media: value },
            Err(error) => InferenceOutcome::Error {
                error: InferenceErrorPayload::from(error),
            },
        };
    }

    // Tool dispatch requests route to the governed `McpRuntime` (as
    // `ToolPort`) on the zed side. The child MCP server (e.g. the swarm
    // server's local delegate loop) never holds token material — the panel
    // default token is minted here, giving the dispatch the same authority
    // as the kask panel's own tool calls (OCAP + gas + reg spans all apply
    // inside `McpRuntime::invoke`).
    if matches!(request.method, InferenceMethod::ToolInvoke) {
        let Some(tool_port) = tool_port else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Connection".to_string(),
                    message: "tool dispatch not configured on the zed side — the IPC server \
                        was started without a tool port. This indicates a startup wiring bug."
                        .to_string(),
                },
            };
        };
        let Some(server) = params.tool_server.clone() else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "ToolPort".to_string(),
                    message: "tool_invoke request missing tool_server".to_string(),
                },
            };
        };
        let Some(tool) = params.tool_name.clone() else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "ToolPort".to_string(),
                    message: "tool_invoke request missing tool_name".to_string(),
                },
            };
        };
        let args = params.tool_args.unwrap_or(serde_json::Value::Null);
        let webid = hkask_types::WebID::from_persona(b"kask-panel");
        let token = hkask_capability::panel_default_token(
            hkask_capability::DelegationResource::Tool,
            tool.clone(),
            hkask_capability::DelegationAction::Execute,
            webid,
            webid,
        );
        return match tool_port.invoke(&server, &tool, args, &token).await {
            Ok(value) => InferenceOutcome::ToolResult { result: value },
            Err(e) => InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "ToolPort".to_string(),
                    message: e.to_string(),
                },
            },
        };
    }

    // Skill-execute requests route to the zed-side `ManifestExecutor` (via
    // the injected `SkillExecPort`). The cascade runs with its own gas/OCAP
    // enforcement on the zed side — the child process never holds token
    // material. Used by `hkask-mcp-swarm`'s local delegate to run an agent's
    // declared `skills` against the task before the LLM call.
    if matches!(request.method, InferenceMethod::SkillExecute) {
        let Some(skill_exec_port) = skill_exec_port else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Connection".to_string(),
                    message: "skill execution not configured on the zed side — the IPC \
                        server was started without a skill exec port. This indicates a \
                        startup wiring bug."
                        .to_string(),
                },
            };
        };
        let Some(name) = params.skill_name.clone() else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "SkillExec".to_string(),
                    message: "skill_execute request missing skill_name".to_string(),
                },
            };
        };
        let task = params.skill_task.unwrap_or_default();
        return match skill_exec_port.execute_skill(&name, &task).await {
            Ok(result) => InferenceOutcome::SkillResult { result },
            Err(e) => InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "SkillExec".to_string(),
                    message: e,
                },
            },
        };
    }

    let result: Result<InferenceResult, InferenceError> = match request.method {
        InferenceMethod::Generate => {
            let prompt = params.prompt.as_deref().unwrap_or("");
            let tools = params.tools.as_deref();
            port.generate(prompt, &params.parameters, tools).await
        }
        InferenceMethod::GenerateWithModel => {
            let prompt = params.prompt.as_deref().unwrap_or("");
            let tools = params.tools.as_deref();
            port.generate_with_model(
                prompt,
                &params.parameters,
                params.model_override.as_deref(),
                tools,
            )
            .await
        }
        InferenceMethod::GenerateWithMessages => {
            let messages = params.messages.as_deref().unwrap_or(&[]);
            let tools = params.tools.as_deref();
            port.generate_with_messages(
                messages,
                &params.parameters,
                params.model_override.as_deref(),
                tools,
            )
            .await
        }
        InferenceMethod::GenerateVision => {
            let prompt = params.prompt.as_deref().unwrap_or("");
            let images = params.images.as_deref().unwrap_or(&[]);
            port.generate_vision(
                prompt,
                images,
                &params.parameters,
                params.model_override.as_deref(),
            )
            .await
        }
        // Already handled above — unreachable.
        InferenceMethod::Embed
        | InferenceMethod::ListModels
        | InferenceMethod::MediaGenerate
        | InferenceMethod::ToolInvoke
        | InferenceMethod::SkillExecute => unreachable!(),
    };

    match result {
        Ok(result) => InferenceOutcome::Result { result },
        Err(error) => InferenceOutcome::Error {
            error: InferenceErrorPayload::from(error),
        },
    }
}

/// Dispatch a media-generation request to the hKask `MediaRouter`.
///
/// `op` selects the backend method. The `InferenceParams` media_* fields
/// carry the op-specific arguments; only the fields relevant to each op
/// are read.
async fn dispatch_media(
    media: &Arc<hkask_inference::MediaRouter>,
    op: &str,
    params: &hkask_types::inference_ipc::InferenceParams,
) -> Result<serde_json::Value, InferenceError> {
    match op {
        "generate_image" => {
            let prompt = params.media_prompt.as_deref().unwrap_or("");
            media
                .generate_image(prompt, params.media_size.as_deref(), params.media_count)
                .await
        }
        "image_to_image" => {
            let image_url = params.media_image_url.as_deref().unwrap_or("");
            let prompt = params.media_prompt.as_deref().unwrap_or("");
            media
                .image_to_image(image_url, prompt, params.media_strength)
                .await
        }
        "remove_background" => {
            let image_url = params.media_image_url.as_deref().unwrap_or("");
            media.remove_background(image_url).await
        }
        "upscale" => {
            let image_url = params.media_image_url.as_deref().unwrap_or("");
            media.upscale(image_url, params.media_scale).await
        }
        "generate_video" => {
            let prompt = params.media_prompt.as_deref().unwrap_or("");
            media.generate_video(prompt, params.media_duration).await
        }
        "image_to_video" => {
            let image_url = params.media_image_url.as_deref().unwrap_or("");
            media
                .image_to_video(
                    image_url,
                    params.media_prompt.as_deref(),
                    params.media_duration,
                )
                .await
        }
        "generate_speech" => {
            let text = params.media_text.as_deref().unwrap_or("");
            let voice = params.media_voice.as_deref().unwrap_or("Rachel");
            media.generate_speech(text, voice).await
        }
        "segment_object" => {
            let image_url = params.media_image_url.as_deref().unwrap_or("");
            let object_description = params.media_object_description.as_deref().unwrap_or("");
            media.segment_object(image_url, object_description).await
        }
        "transcribe" => {
            let audio_url = params.media_audio_url.as_deref().unwrap_or("");
            media
                .transcribe(audio_url, params.media_language.as_deref())
                .await
        }
        "execute_workflow" => {
            let workflow = params
                .media_workflow
                .clone()
                .unwrap_or(serde_json::Value::Null);
            let result = media.execute_workflow(&workflow).await?;
            // `WorkflowResult` is defined in `hkask-inference` and isn't part
            // of the IPC protocol; serialize it to JSON for transport.
            Ok(serde_json::to_value(result).map_err(|e| {
                InferenceError::Json(format!("WorkflowResult serialize failed: {e}"))
            })?)
        }
        other => Err(InferenceError::Connection(format!(
            "unknown media op: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_dir_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = inference_socket_dir().expect("socket dir must be creatable");
        let mode = std::fs::metadata(&dir)
            .expect("socket dir must exist")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "socket dir must be owner-only, got {mode:o}");
    }

    #[tokio::test]
    async fn capped_reader_rejects_overlong_line() {
        let payload = vec![b'x'; (MAX_IPC_LINE_BYTES + 10) as usize];
        let cursor = std::io::Cursor::new(payload);
        let mut reader = CappedReader::new(cursor);
        let result = reader.read_line().await;
        assert!(matches!(result, Err(e) if e.kind() == std::io::ErrorKind::InvalidData));
    }

    #[tokio::test]
    async fn capped_reader_accepts_normal_lines() {
        let cursor = std::io::Cursor::new(b"hello\nworld\n".to_vec());
        let mut reader = CappedReader::new(cursor);
        assert_eq!(reader.read_line().await.unwrap().as_deref(), Some("hello"));
        assert_eq!(reader.read_line().await.unwrap().as_deref(), Some("world"));
        assert_eq!(reader.read_line().await.unwrap(), None);
    }

    #[tokio::test]
    async fn capped_reader_eof_without_newline_is_error() {
        let cursor = std::io::Cursor::new(b"no-newline".to_vec());
        let mut reader = CappedReader::new(cursor);
        let result = reader.read_line().await;
        assert!(matches!(result, Err(e) if e.kind() == std::io::ErrorKind::InvalidData));
    }
}
