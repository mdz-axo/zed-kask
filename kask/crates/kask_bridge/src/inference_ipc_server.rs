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
    InferenceErrorPayload, InferenceMethod, InferenceOutcome, InferenceRequest,
    InferenceResponse, ModelListEntry, WorktreeThreadInfo,
};
use hkask_types::{InferenceError, InferencePort, InferenceResult};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::oneshot;

use crate::inference::LanguageModelEmbeddingPort;

/// A request to spawn a worktree-backed agent thread, sent from the tokio
/// dispatch task to the GPUI-side task via a channel (same pattern as
/// `ListModels`). The GPUI-side task calls the `WorktreeSpawner` and returns
/// the result via the oneshot reply channel.
pub type WorktreeSpawnRequest = (
    String, // prompt
    String, // title
    Option<String>, // worktree_name
    Option<String>, // base_ref
    oneshot::Sender<Result<WorktreeThreadInfo, String>>,
);

/// Spawns a worktree-backed agent thread. Implemented by `main.rs` using
/// `AgentPanelSiblingHost` (which `kask_bridge` can't depend on directly due to
/// a cyclic dependency via `auto_update` → `kask_bridge`). The impl holds a
/// `WeakEntity<AgentPanel>` + `AnyWindowHandle` (both `Send + Sync`) and calls
/// `SiblingThreadHost::create_sibling_thread` inside the GPUI task.
pub trait WorktreeSpawner: Send + Sync {
    /// Create a worktree-backed agent thread. Called from the GPUI-side task
    /// with `&mut AsyncApp`. Returns a `gpui::Task` that resolves to the
    /// thread info or an error message.
    fn spawn(
        &self,
        prompt: String,
        title: String,
        worktree_name: Option<String>,
        base_ref: Option<String>,
        cx: &mut gpui::AsyncApp,
    ) -> gpui::Task<Result<WorktreeThreadInfo, String>>;
}

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
    _worktree_spawn_task: gpui::Task<()>,
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
        worktree_spawner: Option<Arc<dyn WorktreeSpawner>>,
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
        let emb_port = embedding_port;
        let media = media_router;
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
                            let provider_id = provider.id().0;
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

        // Worktree spawn channel — same pattern as `list_models_tx`. The
        // GPUI-side task holds `AsyncApp` and responds to channel requests;
        // the tokio-side dispatch sends requests via the channel. The GPUI
        // task looks up the active workspace's `AgentPanel` on each request
        // (the panel may not exist when the server starts, e.g. before the
        // user opens a project).
        let (worktree_spawn_tx, mut worktree_spawn_rx) =
            tokio::sync::mpsc::unbounded_channel::<WorktreeSpawnRequest>();
        let worktree_spawn_task = cx.spawn(async move |cx| {
            while let Some((prompt, title, worktree_name, base_ref, reply)) =
                worktree_spawn_rx.recv().await
            {
                let result = cx.update(|cx| {
                    use agent::SiblingThreadHost;
                    // Find the active window's MultiWorkspace → workspace →
                    // AgentPanel. Same pattern as `spawn_alert_toast_drainer`
                    // in main.rs.
                    let window = cx.active_window().ok_or_else(|| {
                        "no active window — cannot spawn worktree thread".to_string()
                    })?;
                    let multi_workspace = window
                        .downcast::<MultiWorkspace>()
                        .ok_or_else(|| {
                            "active window is not a MultiWorkspace".to_string()
                        })?;
                    multi_workspace.update(cx, |multi_workspace, _window, cx| {
                        let workspace = multi_workspace.workspace();
                        workspace.update(cx, |workspace, cx| {
                            let panel = workspace
                                .panel::<agent_ui::AgentPanel>()
                                .ok_or_else(|| {
                                    "no agent panel in active workspace".to_string()
                                })?;
                            let host = agent_ui::AgentPanelSiblingHost::new(
                                panel.downgrade(),
                                window,
                            );
                            let request = agent::SiblingThreadRequest {
                                title: title.into(),
                                prompt,
                                agent_id: None,
                                model: None,
                                use_new_worktree: true,
                                worktree_name,
                                base_ref,
                            };
                            let info = host
                                .create_sibling_thread(request, cx)
                                .await
                                .map_err(|e| e.to_string())?;
                            Ok(WorktreeThreadInfo {
                                message: format!(
                                    "Worktree thread created: {} ({})",
                                    info.title, info.agent_id
                                ),
                            })
                        })
                    })
                });
                let result = match result {
                    Ok(Ok(Ok(info))) => Ok(info),
                    Ok(Ok(Err(msg))) => Err(msg),
                    Ok(Err(msg)) => Err(msg),
                    Err(e) => Err(format!("GPUI update failed: {e}")),
                };
                let _ = reply.send(result);
            }
        });

        let worktree_spawn_tx = Arc::new(worktree_spawn_tx);
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
                        let worktree_spawn_tx = worktree_spawn_tx.clone();
                        tokio::spawn(async move {
                            handle_connection(
                                stream,
                                port,
                                emb_port,
                                media,
                                tools,
                                skill_exec,
                                list_models_tx,
                                Some(worktree_spawn_tx),
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
            _worktree_spawn_task: worktree_spawn_task,
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
    worktree_spawn_tx: Option<Arc<tokio::sync::mpsc::UnboundedSender<WorktreeSpawnRequest>>>,
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
            worktree_spawn_tx.as_ref(),
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
    worktree_spawn_tx: Option<&Arc<tokio::sync::mpsc::UnboundedSender<WorktreeSpawnRequest>>>,
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
    //
    // Design tradeoff (R4): the token's `resource_id` is the tool name
    // only, not the `server/tool` pair — `is_valid_for` checks
    // `resource_id == tool` without server scoping. The token authorizes
    // the tool on any server, but `McpRuntime::invoke` routes to the
    // specific `server` parameter, so the actual tool called is on the
    // specified server. The card's `mcp_tools` allowlist (in the swarm
    // delegate loop) gates the full `server/tool` pair before the model's
    // call reaches this dispatch — the allowlist is the effective gate.
    // The `PanelToolInvoker` (kask panel's own calls) uses the identical
    // tool-name-only scoping, so the IPC bridge is consistent with the
    // panel. Adding server-scoping would require changing
    // `panel_default_token` in `hkask-capability` and all callers — a
    // cross-crate change that would tighten the OCAP token without
    // changing the threat model (same-uid processes are trusted).
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
        // The child's declared `server/tool` allowlist is enforced HERE, at
        // the dispatch boundary, before any token is minted — a tool outside
        // it is never authorized, so the allowlist does not depend on the
        // child's in-process matching being correct. Fail closed: a missing
        // or empty allowlist is a protocol violation (the child must declare
        // what it may dispatch), never an implicit grant-all. This is the
        // enforcement point for the .rules "advertised invariants need
        // enforcement points" trap on the delegated-tool authority claim.
        let qualified = format!("{server}/{tool}");
        match &params.tool_allowlist {
            Some(allowlist) if !allowlist.is_empty() => {
                if !allowlist.iter().any(|a| a == &qualified) {
                    return InferenceOutcome::Error {
                        error: InferenceErrorPayload {
                            code: "ToolPort".to_string(),
                            message: format!(
                                "tool '{qualified}' is not in the delegated tool allowlist — \
                                 refused before minting the panel token"
                            ),
                        },
                    };
                }
            }
            _ => {
                return InferenceOutcome::Error {
                    error: InferenceErrorPayload {
                        code: "ToolPort".to_string(),
                        message: "tool_invoke request missing tool_allowlist — the delegated \
                            tool allowlist must be declared per request (fail closed)"
                            .to_string(),
                    },
                };
            }
        }
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
                    message: e.to_string(),
                },
            },
        };
    }

    // CreateWorktreeThread requests are dispatched via the GPUI context —
    // they call `SiblingThreadHost::create_sibling_thread` on the zed side,
    // which needs `AsyncApp` (not `Send`). Same channel pattern as
    // `ListModels`.
    if matches!(request.method, InferenceMethod::CreateWorktreeThread) {
        let Some(ref tx) = worktree_spawn_tx else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Connection".to_string(),
                    message: "worktree spawn port not configured on the zed side \
                              (no active workspace or SiblingThreadHost)"
                        .to_string(),
                },
            };
        };
        let prompt = params.worktree_prompt.as_deref().unwrap_or("");
        let title = params.worktree_title.as_deref().unwrap_or("Kanban Task");
        let name = params.worktree_name.clone();
        let base_ref = params.worktree_base_ref.clone();
        let (tx_reply, rx_reply) =
            oneshot::channel::<Result<WorktreeThreadInfo, String>>();
        if tx
            .send((
                prompt.to_string(),
                title.to_string(),
                name,
                base_ref,
                tx_reply,
            ))
            .is_err()
        {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Connection".to_string(),
                    message: "GPUI-side worktree_spawn task dropped — server shutting down"
                        .to_string(),
                },
            };
        }
        return match rx_reply.await {
            Ok(Ok(thread)) => InferenceOutcome::WorktreeThread { thread },
            Ok(Err(msg)) => InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "WorktreeSpawn".to_string(),
                    message: msg,
                },
            },
            Err(_) => InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Connection".to_string(),
                    message: "GPUI-side worktree_spawn task dropped reply channel"
                        .to_string(),
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
        | InferenceMethod::SkillExecute
        | InferenceMethod::CreateWorktreeThread => unreachable!(),
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

    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;

    use hkask_capability::ToolInfo;
    use hkask_capability::{DelegationToken, ToolFuture, ToolPort, ToolPortError};
    use hkask_types::inference_ipc::InferenceParams;
    use hkask_types::{InferenceUsage, LLMParameters, SkillExecError, SkillExecPort};
    use tokio::sync::mpsc;

    /// RR-0031: the inference IPC socket directory must be owner-private
    /// (mode 0700) so other local users cannot reach the socket — the socket
    /// drives LLM calls billed to the operator's API keys. Restored from the
    /// pre-consolidation test suite so the regression library keys on this
    /// exact test name.
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

    // ── Stub ports ───────────────────────────────────────────────────────
    //
    // The IPC server dispatches to three trait objects (`InferencePort`,
    // `ToolPort`, `SkillExecPort`). These stubs record their inputs and
    // return canned outputs so the dispatch routing can be exercised without
    // a real LLM backend, MCP runtime, or manifest executor.

    /// `InferencePort` stub that records the prompt and returns a canned
    /// `InferenceResult`. Only `generate` is implemented — the dispatch tests
    /// for `Generate` route through it.
    struct StubInferencePort {
        recorded_prompt: Mutex<Option<String>>,
        result_text: String,
    }

    impl StubInferencePort {
        fn new(result_text: &str) -> Self {
            Self {
                recorded_prompt: Mutex::new(None),
                result_text: result_text.to_string(),
            }
        }

        fn recorded_prompt(&self) -> Option<String> {
            self.recorded_prompt.lock().expect("prompt lock").clone()
        }
    }

    impl InferencePort for StubInferencePort {
        fn generate(
            &self,
            prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            *self.recorded_prompt.lock().expect("prompt lock") = Some(prompt.to_string());
            let result_text = self.result_text.clone();
            Box::pin(async move {
                Ok(InferenceResult {
                    text: result_text,
                    model: "stub-model".to_string(),
                    usage: InferenceUsage {
                        prompt_tokens: 1,
                        completion_tokens: 2,
                        total_tokens: 3,
                    },
                    finish_reason: "stop".to_string(),
                    token_probabilities: None,
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    /// `ToolPort` stub that records `(server, tool, args)` and returns a canned
    /// JSON value. The OCAP token is ignored — the stub does not enforce
    /// capabilities (that is `McpRuntime`'s job in production).
    struct StubToolPort {
        recorded: Mutex<Option<(String, String, serde_json::Value)>>,
        output: serde_json::Value,
    }

    impl StubToolPort {
        fn new(output: serde_json::Value) -> Self {
            Self {
                recorded: Mutex::new(None),
                output,
            }
        }

        fn recorded(&self) -> Option<(String, String, serde_json::Value)> {
            self.recorded.lock().expect("tool lock").clone()
        }
    }

    impl ToolPort for StubToolPort {
        fn invoke<'a>(
            &'a self,
            server: &'a str,
            tool: &'a str,
            args: serde_json::Value,
            _token: &'a DelegationToken,
        ) -> ToolFuture<'a, Result<serde_json::Value, ToolPortError>> {
            *self.recorded.lock().expect("tool lock") =
                Some((server.to_string(), tool.to_string(), args));
            let output = self.output.clone();
            Box::pin(async move { Ok(output) })
        }

        fn discover_tools<'a>(&'a self) -> ToolFuture<'a, Vec<String>> {
            Box::pin(async move { Vec::new() })
        }

        fn get_tool_info<'a>(&'a self, _tool_name: &'a str) -> ToolFuture<'a, Option<ToolInfo>> {
            Box::pin(async move { None })
        }
    }

    /// `SkillExecPort` stub that records `(name, task)` and returns canned text.
    struct StubSkillExecPort {
        recorded: Mutex<Option<(String, String)>>,
        output: String,
    }

    impl StubSkillExecPort {
        fn new(output: &str) -> Self {
            Self {
                recorded: Mutex::new(None),
                output: output.to_string(),
            }
        }

        fn recorded(&self) -> Option<(String, String)> {
            self.recorded.lock().expect("skill lock").clone()
        }
    }

    impl SkillExecPort for StubSkillExecPort {
        fn execute_skill<'a>(
            &'a self,
            name: &'a str,
            task: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<String, SkillExecError>> + Send + 'a>> {
            *self.recorded.lock().expect("skill lock") = Some((name.to_string(), task.to_string()));
            let output = self.output.clone();
            Box::pin(async move { Ok(output) })
        }
    }

    /// Build `InferenceParams` with only `prompt` set and everything else
    /// defaulted/None. Callers override specific fields as needed.
    fn params_with_prompt(prompt: &str) -> InferenceParams {
        InferenceParams {
            prompt: Some(prompt.to_string()),
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
            tool_server: None,
            tool_name: None,
            tool_args: None,
            tool_allowlist: None,
            skill_name: None,
            skill_task: None,
                worktree_prompt: None,
                worktree_title: None,
                worktree_name: None,
                worktree_base_ref: None,
        }
    }

    /// Build the `list_models_tx` channel the dispatch function requires. The
    /// receiver is dropped — only `ListModels` requests use it, and these
    /// tests never send one.
    fn dummy_list_models_tx() -> Arc<mpsc::UnboundedSender<(oneshot::Sender<Vec<ModelListEntry>>,)>>
    {
        let (tx, _rx) = mpsc::unbounded_channel::<(oneshot::Sender<Vec<ModelListEntry>>,)>();
        Arc::new(tx)
    }

    // ── dispatch (pure function) tests ────────────────────────────────────

    /// M4: a `Generate` request routes to the `InferencePort` and returns the
    /// stub's canned `InferenceResult`, recording the prompt.
    #[tokio::test]
    async fn dispatch_generate_returns_canned_result() {
        let stub = Arc::new(StubInferencePort::new("canned-output"));
        let port: Arc<dyn InferencePort> = stub.clone();
        let list_models_tx = dummy_list_models_tx();

        let request = InferenceRequest {
            id: 1,
            method: InferenceMethod::Generate,
            params: params_with_prompt("hello inference"),
        };

        let outcome = dispatch(&port, None, None, None, None, &list_models_tx, request).await;

        match outcome {
            InferenceOutcome::Result { result } => {
                assert_eq!(result.text, "canned-output");
                assert_eq!(result.model, "stub-model");
            }
            other => panic!("expected Result outcome, got {other:?}"),
        }
        assert_eq!(stub.recorded_prompt().as_deref(), Some("hello inference"));
    }

    /// M4: a `ToolInvoke` request routes to the `ToolPort` (after the
    /// delegated-tool allowlist gate), recording `(server, tool, args)` and
    /// returning a `ToolResult` outcome.
    #[tokio::test]
    async fn dispatch_tool_invoke_returns_canned_result() {
        let stub = Arc::new(StubToolPort::new(
            serde_json::json!({ "rows": 7, "result": "inner" }),
        ));
        let tool_port: Arc<dyn ToolPort> = stub.clone();
        let inference_port: Arc<dyn InferencePort> = Arc::new(StubInferencePort::new("unused"));
        let list_models_tx = dummy_list_models_tx();

        let mut params = params_with_prompt("");
        params.tool_server = Some("test-server".to_string());
        params.tool_name = Some("test-tool".to_string());
        params.tool_args = Some(serde_json::json!({ "q": "rust" }));
        params.tool_allowlist = Some(vec!["test-server/test-tool".to_string()]);

        let request = InferenceRequest {
            id: 2,
            method: InferenceMethod::ToolInvoke,
            params,
        };

        let outcome = dispatch(
            &inference_port,
            None,
            None,
            Some(&tool_port),
            None,
            &list_models_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::ToolResult { result } => {
                assert_eq!(result["rows"], 7);
                assert_eq!(result["result"], "inner");
            }
            other => panic!("expected ToolResult outcome, got {other:?}"),
        }
        let recorded = stub.recorded().expect("tool port was not called");
        assert_eq!(recorded.0, "test-server");
        assert_eq!(recorded.1, "test-tool");
        assert_eq!(recorded.2["q"], "rust");
    }

    /// M4: a `SkillExecute` request routes to the `SkillExecPort`, recording
    /// `(name, task)` and returning a `SkillResult` outcome.
    #[tokio::test]
    async fn dispatch_skill_execute_returns_canned_result() {
        let stub = Arc::new(StubSkillExecPort::new("cascade-output"));
        let skill_port: Arc<dyn SkillExecPort> = stub.clone();
        let inference_port: Arc<dyn InferencePort> = Arc::new(StubInferencePort::new("unused"));
        let list_models_tx = dummy_list_models_tx();

        let mut params = params_with_prompt("");
        params.skill_name = Some("grill-me".to_string());
        params.skill_task = Some("audit this dispatch".to_string());

        let request = InferenceRequest {
            id: 3,
            method: InferenceMethod::SkillExecute,
            params,
        };

        let outcome = dispatch(
            &inference_port,
            None,
            None,
            None,
            Some(&skill_port),
            &list_models_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::SkillResult { result } => {
                assert_eq!(result, "cascade-output");
            }
            other => panic!("expected SkillResult outcome, got {other:?}"),
        }
        let recorded = stub.recorded().expect("skill port was not called");
        assert_eq!(recorded.0, "grill-me");
        assert_eq!(recorded.1, "audit this dispatch");
    }

    /// M4: a `ToolInvoke` request with no tool port configured returns an
    /// `Error` outcome — the dispatch never panics, it fail-closes with a
    /// diagnostic. This is the dispatch-level error-return path (the closed
    /// `InferenceMethod` enum has no "unknown" variant, so the analogous
    /// error path is the missing-port arm).
    #[tokio::test]
    async fn dispatch_tool_invoke_without_port_returns_error() {
        let inference_port: Arc<dyn InferencePort> = Arc::new(StubInferencePort::new("unused"));
        let list_models_tx = dummy_list_models_tx();

        let mut params = params_with_prompt("");
        params.tool_server = Some("any".to_string());
        params.tool_name = Some("any".to_string());
        params.tool_allowlist = Some(vec!["any/any".to_string()]);

        let request = InferenceRequest {
            id: 4,
            method: InferenceMethod::ToolInvoke,
            params,
        };

        let outcome = dispatch(
            &inference_port,
            None,
            None,
            None,
            None,
            &list_models_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "Connection");
                assert!(
                    error.message.contains("tool dispatch not configured"),
                    "unexpected error message: {error:?}",
                );
            }
            other => panic!("expected Error outcome, got {other:?}"),
        }
    }

    /// M4: a `ToolInvoke` request whose `tool_allowlist` does not contain the
    /// qualified `server/tool` is refused *before* the panel token is minted —
    /// the dispatched-tool allowlist is enforced at the dispatch boundary.
    #[tokio::test]
    async fn dispatch_tool_invoke_rejects_unallowed_tool() {
        let stub = Arc::new(StubToolPort::new(serde_json::json!("must-not-be-called")));
        let tool_port: Arc<dyn ToolPort> = stub.clone();
        let inference_port: Arc<dyn InferencePort> = Arc::new(StubInferencePort::new("unused"));
        let list_models_tx = dummy_list_models_tx();

        let mut params = params_with_prompt("");
        params.tool_server = Some("server-a".to_string());
        params.tool_name = Some("tool-x".to_string());
        params.tool_allowlist = Some(vec!["server-a/tool-y".to_string()]);

        let request = InferenceRequest {
            id: 5,
            method: InferenceMethod::ToolInvoke,
            params,
        };

        let outcome = dispatch(
            &inference_port,
            None,
            None,
            Some(&tool_port),
            None,
            &list_models_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "ToolPort");
                assert!(
                    error.message.contains("server-a/tool-x"),
                    "error should name the refused tool, got: {error:?}",
                );
            }
            other => panic!("expected Error outcome, got {other:?}"),
        }
        assert!(stub.recorded().is_none(), "tool port must not be invoked");
    }

    // ── CappedReader line-length enforcement ──────────────────────────────

    /// M4: a line exceeding `MAX_IPC_LINE_BYTES` without a newline is rejected
    /// with `InvalidData` / `LINE_TOO_LONG`. This is the CWE-400 unbounded
    /// read guard; the connection is unusable afterward (the buffered
    /// remainder cannot be re-synchronized to a message boundary).
    #[tokio::test]
    async fn capped_reader_rejects_oversized_line() {
        use tokio::io::{AsyncWriteExt, duplex};

        // Buffer large enough to hold the oversized payload in one write so
        // `write_all` does not block.
        let capacity = (MAX_IPC_LINE_BYTES as usize) + 64;
        let (read, mut write) = duplex(capacity);
        let oversized = vec![b'a'; (MAX_IPC_LINE_BYTES as usize) + 1];
        write
            .write_all(&oversized)
            .await
            .expect("write fits buffer");
        write.flush().await.expect("flush");
        drop(write); // signal EOF so the reader can drain

        let mut reader = CappedReader::new(read);
        let result = reader.read_line().await;
        let err = result.expect_err("oversized line must error");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(err.to_string(), LINE_TOO_LONG);
    }

    /// M4: a line within the cap followed by a newline is accepted.
    #[tokio::test]
    async fn capped_reader_accepts_line_within_cap() {
        use tokio::io::{AsyncWriteExt, duplex};

        let capacity = 1024;
        let (read, mut write) = duplex(capacity);
        write.write_all(b"short line\n").await.expect("write");
        write.flush().await.expect("flush");
        drop(write);

        let mut reader = CappedReader::new(read);
        let line = reader
            .read_line()
            .await
            .expect("read within cap")
            .expect("some line");
        assert_eq!(line, "short line");
    }

    // ── handle_connection (real wire format) tests ────────────────────────

    /// M4: end-to-end through a Unix socket pair. A `Generate` request is
    /// serialized to the real newline-delimited JSON wire format, written to
    /// the client end, and the response is read back and parsed — exercising
    /// `peer_is_owner`, `CappedReader`, `dispatch`, and the response
    /// serialization in one pass.
    #[tokio::test]
    async fn handle_connection_end_to_end_generate() {
        let (client, server) = tokio::net::UnixStream::pair().expect("socket pair");
        let port: Arc<dyn InferencePort> = Arc::new(StubInferencePort::new("hello-from-bridge"));
        let list_models_tx = dummy_list_models_tx();
        let handle = tokio::spawn(handle_connection(
            server,
            port,
            None,
            None,
            None,
            None,
            list_models_tx,
        ));

        let request = InferenceRequest {
            id: 42,
            method: InferenceMethod::Generate,
            params: params_with_prompt("hi"),
        };
        let line = serde_json::to_string(&request).expect("serialize request") + "\n";

        let (read, mut write) = tokio::io::split(client);
        write
            .write_all(line.as_bytes())
            .await
            .expect("write request");
        write.flush().await.expect("flush request");

        let mut response = String::new();
        let mut buf_reader = BufReader::new(read);
        buf_reader
            .read_line(&mut response)
            .await
            .expect("read response");

        let parsed: InferenceResponse = serde_json::from_str(&response).expect("parse response");
        assert_eq!(parsed.id, 42);
        match parsed.outcome {
            InferenceOutcome::Result { result } => assert_eq!(result.text, "hello-from-bridge"),
            other => panic!("expected Result outcome, got {other:?}"),
        }

        // Drop the client halves so the server's read loop sees EOF and exits.
        drop(write);
        drop(buf_reader);
        handle.await.expect("handle join");
    }

    /// M4: a line carrying an unknown `method` variant fails to deserialize as
    /// an `InferenceRequest` and is *skipped* (the protocol writes no response
    /// for unparseable lines — see `handle_connection`). The connection
    /// survives, and a subsequent valid request is answered. `InferenceMethod`
    /// is a closed enum (no `#[serde(other)]`), so an unknown method string is
    /// a parse failure, not a dispatchable variant; this test pins the actual
    /// behavior rather than fabricating an error response.
    #[tokio::test]
    async fn handle_connection_skips_unparseable_line() {
        let (client, server) = tokio::net::UnixStream::pair().expect("socket pair");
        let port: Arc<dyn InferencePort> = Arc::new(StubInferencePort::new("after-skip"));
        let list_models_tx = dummy_list_models_tx();
        let handle = tokio::spawn(handle_connection(
            server,
            port,
            None,
            None,
            None,
            None,
            list_models_tx,
        ));

        // Malformed request: unknown method variant — deserialization fails.
        let bad = b"{\"id\":1,\"method\":\"totally_made_up\",\"params\":{}}\n";
        // Valid request that follows it.
        let good_request = InferenceRequest {
            id: 2,
            method: InferenceMethod::Generate,
            params: params_with_prompt("second chance"),
        };
        let good = serde_json::to_string(&good_request).expect("serialize") + "\n";

        let (read, mut write) = tokio::io::split(client);
        write.write_all(bad).await.expect("write bad line");
        write
            .write_all(good.as_bytes())
            .await
            .expect("write good line");
        write.flush().await.expect("flush");

        // Exactly one response should arrive — for id=2 (the valid request).
        let mut response = String::new();
        let mut buf_reader = BufReader::new(read);
        buf_reader
            .read_line(&mut response)
            .await
            .expect("read response");

        let parsed: InferenceResponse = serde_json::from_str(&response).expect("parse response");
        assert_eq!(
            parsed.id, 2,
            "only the valid (id=2) request may be answered; got id={}",
            parsed.id,
        );
        match parsed.outcome {
            InferenceOutcome::Result { result } => assert_eq!(result.text, "after-skip"),
            other => panic!("expected Result outcome, got {other:?}"),
        }

        drop(write);
        drop(buf_reader);
        handle.await.expect("handle join");
    }
}
