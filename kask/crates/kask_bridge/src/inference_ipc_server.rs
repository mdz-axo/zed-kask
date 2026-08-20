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
//!   │           └── LanguageModelInferencePort → zed's LanguageModelRegistry
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
    ModelListEntry, WorktreeThreadInfo,
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
    String,         // prompt
    String,         // title
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

/// Process-global worktree spawner. Set by `main.rs` when a workspace with an
/// `AgentPanel` opens; read by the GPUI-side IPC task when a
/// `CreateWorktreeThread` request arrives. Mirrors the `set_tool_invoker` /
/// `shared_tool_invoker` pattern in `hkask-tool-invoker` (Mutex-based,
/// re-settable). When `None`, worktree spawn requests return an error and the
/// MCP server falls back to the in-memory `LazyLocalSwarmRuntime` path.
static WORKTREE_SPAWNER: std::sync::Mutex<Option<Arc<dyn WorktreeSpawner>>> =
    std::sync::Mutex::new(None);

/// Inject the global worktree spawner (composition root — `main.rs`). Called
/// when a workspace with an `AgentPanel` opens. Replaces any prior spawner
/// (e.g. when the user switches workspaces).
pub fn set_worktree_spawner(spawner: Option<Arc<dyn WorktreeSpawner>>) {
    *WORKTREE_SPAWNER.lock().expect("WORKTREE_SPAWNER poisoned") = spawner;
}

/// Read the global worktree spawner. Returns `None` when no workspace with an
/// `AgentPanel` is open. Called only by the GPUI-side IPC task in this crate
/// (`InferenceIpcServer::start`'s worktree spawn task); not re-exported.
pub(crate) fn shared_worktree_spawner() -> Option<Arc<dyn WorktreeSpawner>> {
    WORKTREE_SPAWNER
        .lock()
        .expect("WORKTREE_SPAWNER poisoned")
        .clone()
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
    /// the `LanguageModelInferencePort` backed by zed's `LanguageModelRegistry`).
    /// `embedding_port` is the port to dispatch embedding requests to (the
    /// `LanguageModelEmbeddingPort`). When `None`, `embed` requests return an
    /// error.
    /// (image, video, speech, transcription via registered `MediaProvider`
    /// backends). When `None`,.
    /// `tool_port` is the governed `McpRuntime` (as `ToolPort`) used for
    /// `tool_invoke` requests from MCP servers that run agent loops (e.g.
    /// `hkask-mcp-swarm`'s local delegate). When `None`, `tool_invoke`
    /// requests return an error. The zed side mints the OCAP panel token —
    /// the child process never holds token material.
    /// `skill_exec_port` runs `skill_execute` requests through the zed-side
    /// `ManifestExecutor` (its own enforcement). When `None`,
    /// `skill_execute` requests return an error.
    pub fn start(
        inference_port: Arc<dyn InferencePort>,
        embedding_port: Option<LanguageModelEmbeddingPort>,
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
        let emb_port = embedding_port;
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
                let Some(spawner) = shared_worktree_spawner() else {
                    let _ = reply.send(Err(
                        "worktree spawner not configured (no active workspace)".to_string(),
                    ));
                    continue;
                };
                let task = spawner.spawn(prompt, title, worktree_name, base_ref, cx);
                let result = task.await;
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
                        let tools = tools.clone();
                        let skill_exec = skill_exec.clone();
                        let list_models_tx = list_models_tx.clone();
                        let worktree_spawn_tx = worktree_spawn_tx.clone();
                        tokio::spawn(async move {
                            handle_connection(
                                stream,
                                port,
                                emb_port,
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

    // Tool dispatch requests route to the `McpRuntime` (as `ToolPort`) on the
    // zed side. The child MCP server (e.g. the swarm server's local delegate
    // loop) holds no credential — the `tool_allowlist` check below IS the
    // authority boundary for this dispatch, and it is enforced here rather than
    // in the child so it does not depend on the child's own matching being
    // correct.
    //
    // This replaced a `DelegationToken` capability check inside
    // `McpRuntime::invoke` that could not deny anything: the token's
    // `resource_id` was set from the same `tool` value passed to `invoke`, so
    // the check compared a value against itself. The allowlist below is the
    // real gate because the caller does not choose its contents.
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
        // Accounting identity for the call meter — not a credential.
        let webid = hkask_types::WebID::from_persona(b"kask-panel");
        return match tool_port.invoke(&server, &tool, args, webid).await {
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
    // the injected `SkillExecPort`). The cascade runs with its own enforcement
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
        let (tx_reply, rx_reply) = oneshot::channel::<Result<WorktreeThreadInfo, String>>();
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
                    message: "GPUI-side worktree_spawn task dropped reply channel".to_string(),
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
