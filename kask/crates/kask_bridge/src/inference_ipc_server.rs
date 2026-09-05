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
    BatchResultEntry, InferenceErrorPayload, InferenceMethod, InferenceOutcome, InferenceRequest,
    InferenceResponse, ModelListEntry, WorktreeThreadInfo,
};
use hkask_types::process_global::ProcessGlobal;
use hkask_types::{InferenceError, InferencePort, InferenceResult};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::oneshot;

use crate::inference_embedding::LanguageModelEmbeddingPort;

/// A request to spawn a worktree-backed agent thread, sent from the tokio
/// dispatch task to the GPUI-side task via a channel (same pattern as
/// `ListModels`). The GPUI-side task calls the `WorktreeSpawner` and returns
/// the result via the oneshot reply channel.
pub(crate) type WorktreeSpawnRequest = (
    String,         // prompt
    String,         // title
    Option<String>, // worktree_name
    Option<String>, // base_ref
    oneshot::Sender<Result<WorktreeThreadInfo, String>>,
);

/// A request to read a provider API key from zed's `CredentialsProvider`,
/// sent from the tokio dispatch task to the GPUI-side task via a channel
/// (same pattern as `ListModels`). The GPUI-side task reads the key from the
/// keychain and returns it via the oneshot reply channel.
///
/// `credential_url` is the keychain URL the API key is stored under — for
/// inference providers (openrouter, deepinfra), the provider's `api_url`
/// slot, resolved via `provider_by_credential_key` (one key, one location).
pub(crate) type BatchCredentialRequest = (
    String,                                  // credential_url
    oneshot::Sender<Result<String, String>>, // api_key or error
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
static WORKTREE_SPAWNER: ProcessGlobal<Arc<dyn WorktreeSpawner>> = ProcessGlobal::new();

/// Inject the global worktree spawner (composition root — `main.rs`). Called
/// when a workspace with an `AgentPanel` opens. Replaces any prior spawner
/// (e.g. when the user switches workspaces).
pub fn set_worktree_spawner(spawner: Option<Arc<dyn WorktreeSpawner>>) {
    WORKTREE_SPAWNER.set(spawner);
}

/// Read the global worktree spawner. Returns `None` when no workspace with an
/// `AgentPanel` is open. Called only by the GPUI-side IPC task in this crate
/// (`InferenceIpcServer::start`'s worktree spawn task); not re-exported.
pub(crate) fn shared_worktree_spawner() -> Option<Arc<dyn WorktreeSpawner>> {
    WORKTREE_SPAWNER.get()
}

/// The zed-side inference IPC server.
///
/// Listens on a Unix socket and dispatches inference requests to the
/// provided `InferencePort`. Each connection is handled in its own task.
///
/// # Socket cleanup
///
/// The socket file is intentionally leaked: `start` spawns a detached tokio
/// task that owns the `UnixListener`, and the GPUI-side channel tasks
/// (list_models, worktree_spawn, batch_credential) are **detached** in
/// `start` so they run for the process lifetime. This is load-bearing: a
/// GPUI `Task` is cancelled immediately when its handle is dropped (unlike a
/// tokio `JoinHandle`, whose drop detaches) — storing the handles in this
/// struct made the tasks' lifetime depend on the caller keeping the
/// `InferenceIpcServer` value alive, and every caller binds it as a
/// closure-local that drops at the end of startup, silently cancelling the
/// credential/list_models/worktree channels while the detached listener kept
/// serving (rerank then fails with "GPUI-side credential task dropped").
/// There is no `Drop` impl because it could never run — Rust does not drop
/// detached async tasks or process-global statics on process exit. The
/// socket lives in a per-user private tmpdir (pid + nonce, 0600 file inside
/// a 0700 dir, see `generate_socket_path` / `inference_socket_dir`), so the
/// OS reaps it on reboot or tmpdir cleanup. Adding a `Drop` impl that calls
/// `std::fs::remove_file` would be dead code and silently swallow the io
/// result (the `let _ =` trap).
pub struct InferenceIpcServer {
    /// The socket path — passed to MCP server child processes via env var.
    socket_path: PathBuf,
    /// The background listener task. Dropping a tokio `JoinHandle` detaches
    /// the task, so this field is decorative — kept for symmetry with the
    /// detached GPUI tasks above.
    _task: tokio::task::JoinHandle<()>,
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
    pub fn start(
        inference_port: Arc<dyn InferencePort>,
        embedding_port: Option<LanguageModelEmbeddingPort>,
        tool_port: Option<Arc<dyn hkask_tool_port::ToolPort>>,
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
            // Remove any stale socket file. A failure here (file doesn't
            // exist, permission denied) is expected on first launch — log
            // rather than silently discarding so a persistent permission
            // issue is visible.
            if let Err(e) = std::fs::remove_file(&socket_path_for_bind) {
                tracing::debug!(
                    target: "hkask.inference.ipc",
                    error = %e,
                    path = %socket_path_for_bind.display(),
                    "Stale socket removal (expected on first launch)"
                );
            }
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

        // Spawn a GPUI-side task for ListModels requests. `AsyncApp` is not
        // `Send`, so we can't pass it into tokio::spawn. Instead, this task
        // holds the `AsyncApp` and responds to channel requests — the same
        // pattern as `LanguageModelEmbeddingPort`.
        let (list_models_tx, mut list_models_rx) = tokio::sync::mpsc::unbounded_channel::<(
            tokio::sync::oneshot::Sender<Vec<ModelListEntry>>,
        )>();
        // Detached: the task must outlive the `InferenceIpcServer` value —
        // a GPUI `Task` is cancelled on handle drop (see the struct doc).
        cx.spawn(async move |cx| {
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
        })
        .detach();

        let list_models_tx = Arc::new(list_models_tx);

        // Worktree spawn channel — same pattern as `list_models_tx`. The
        // GPUI-side task holds `AsyncApp` and responds to channel requests;
        // the tokio-side dispatch sends requests via the channel. The GPUI
        // task looks up the active workspace's `AgentPanel` on each request
        // (the panel may not exist when the server starts, e.g. before the
        // user opens a project).
        let (worktree_spawn_tx, mut worktree_spawn_rx) =
            tokio::sync::mpsc::unbounded_channel::<WorktreeSpawnRequest>();
        // Detached: see the list_models task above.
        cx.spawn(async move |cx| {
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
        })
        .detach();

        let worktree_spawn_tx = Arc::new(worktree_spawn_tx);

        // Batch credential channel — same pattern as `list_models_tx`. The
        // GPUI-side task reads API keys from zed's `CredentialsProvider`
        // keychain and returns them via the oneshot reply channel. The tokio-
        // side dispatch uses this to get the provider API key for batch
        // inference calls — the key never leaves the zed process.
        let (batch_credential_tx, mut batch_credential_rx) =
            tokio::sync::mpsc::unbounded_channel::<BatchCredentialRequest>();
        // Detached: see the list_models task above. This is the channel the
        // rerank dispatch reads the OpenRouter key through — if this task
        // dies, every deep-strategy rerank fails with "GPUI-side credential
        // task dropped".
        cx.spawn(async move |cx| {
            while let Some((credential_url, reply)) = batch_credential_rx.recv().await {
                let credentials_provider = cx.update(|cx| zed_credentials_provider::global(cx));
                let result = credentials_provider
                    .read_credentials(&credential_url, cx)
                    .await;
                match result {
                    Ok(Some((_username, password_bytes))) => {
                        let password = String::from_utf8_lossy(&password_bytes).to_string();
                        let _ = reply.send(Ok(password));
                    }
                    Ok(None) => {
                        let _ = reply.send(Err(format!(
                            "credential '{credential_url}' not found in keychain"
                        )));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(format!(
                            "failed to read credential '{credential_url}': {e}"
                        )));
                    }
                }
            }
        })
        .detach();
        let batch_credential_tx = Arc::new(batch_credential_tx);

        let task = tokio_handle.spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let port = port.clone();
                        let emb_port = emb_port.clone();
                        let tools = tools.clone();
                        let list_models_tx = list_models_tx.clone();
                        let worktree_spawn_tx = worktree_spawn_tx.clone();
                        let batch_credential_tx = batch_credential_tx.clone();
                        tokio::spawn(async move {
                            handle_connection(
                                stream,
                                port,
                                emb_port,
                                tools,
                                list_models_tx,
                                Some(worktree_spawn_tx),
                                batch_credential_tx,
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
        })
    }

    /// The socket path — pass this to MCP server child processes via the
    /// `HKASK_INFERENCE_SOCKET` env var.
    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
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
    tool_port: Option<Arc<dyn hkask_tool_port::ToolPort>>,
    list_models_tx: Arc<
        tokio::sync::mpsc::UnboundedSender<(tokio::sync::oneshot::Sender<Vec<ModelListEntry>>,)>,
    >,
    worktree_spawn_tx: Option<Arc<tokio::sync::mpsc::UnboundedSender<WorktreeSpawnRequest>>>,
    batch_credential_tx: Arc<tokio::sync::mpsc::UnboundedSender<BatchCredentialRequest>>,
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
        // One in-flight request per connection. Monitor EOF while dispatch is
        // pending so dropping a client cancels queued/provider work immediately.
        let outcome = tokio::select! {
            result = dispatch(&port, embedding_port.as_ref(), tool_port.as_ref(),
                &list_models_tx, worktree_spawn_tx.as_ref(), &batch_credential_tx, request) => result,
            next = reader.read_line() => {
                match next {
                    Ok(None) => tracing::debug!(target: "reg.inference", "IPC caller disconnected; local dispatch cancelled"),
                    Ok(Some(_)) => tracing::warn!(target: "reg.inference", "Pipelined IPC requests are unsupported; closing connection"),
                    Err(error) => tracing::warn!(target: "reg.inference", %error, "IPC read failed during dispatch"),
                }
                return;
            }
        };

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

        // Classify peer-cancellation vs genuine write failure. The peer is a
        // child MCP server process that may close its socket at any time —
        // its own read timeout, a parent-side cancel, or process exit. EPIPE
        // / ConnectionReset on `write_all` is the *normal* way a peer cancels:
        // the request was already read and dispatched, and the server only
        // discovers the cancellation when it tries to write the response.
        // Logging this at `warn` produces a storm that blames the IPC layer
        // for what was a self-inflicted cancellation — the operator can't
        // distinguish "provider slow" from "IPC broken" from it. Peer
        // cancellation is logged at `debug` so it's available for diagnosis
        // without masquerading as a fault.
        let write_result = async {
            writer.write_all(response_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            Ok::<(), std::io::Error>(())
        }
        .await;
        if let Err(e) = write_result {
            let is_peer_cancellation = matches!(
                e.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
            );
            if is_peer_cancellation {
                tracing::debug!(
                    target: "reg.inference",
                    error = %e,
                    "Inference IPC peer closed before response written — \
                     client cancelled (e.g. its read deadline fired, or the \
                     child process exited). Not a server-side fault; closing \
                     this connection. If the client's deadline fired, check \
                     HKASK_INFERENCE_TIMEOUT_SECS alignment with the server's \
                     inference_timeout_secs."
                );
            } else {
                tracing::warn!(
                    target: "reg.inference",
                    error = %e,
                    "Inference IPC write failed — closing connection"
                );
            }
            break;
        }
    }
}

/// Dispatch a single request to the inference port.
async fn dispatch(
    port: &Arc<dyn InferencePort>,
    embedding_port: Option<&LanguageModelEmbeddingPort>,
    tool_port: Option<&Arc<dyn hkask_tool_port::ToolPort>>,
    list_models_tx: &Arc<
        tokio::sync::mpsc::UnboundedSender<(tokio::sync::oneshot::Sender<Vec<ModelListEntry>>,)>,
    >,
    worktree_spawn_tx: Option<&Arc<tokio::sync::mpsc::UnboundedSender<WorktreeSpawnRequest>>>,
    batch_credential_tx: &Arc<tokio::sync::mpsc::UnboundedSender<BatchCredentialRequest>>,
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
            Err(e) => {
                let (code, message) = match e {
                    hkask_types::EmbeddingGenerationError::InvalidRequest(m) => {
                        ("InvalidRequest", m)
                    }
                    hkask_types::EmbeddingGenerationError::Connection(m) => ("Connection", m),
                    hkask_types::EmbeddingGenerationError::Api(status, m) => {
                        ("Api", format!("status {status}: {m}"))
                    }
                    hkask_types::EmbeddingGenerationError::Json(m) => ("Json", m),
                    hkask_types::EmbeddingGenerationError::EmptyResponse => {
                        ("Api", "empty response from embedding model".to_string())
                    }
                    hkask_types::EmbeddingGenerationError::DimensionMismatch {
                        expected,
                        actual,
                    } => (
                        "Api",
                        format!("dimension mismatch: expected {expected}, got {actual}"),
                    ),
                };
                InferenceOutcome::Error {
                    error: InferenceErrorPayload {
                        code: code.to_string(),
                        message,
                    },
                }
            }
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
                    message: "GPUI-side list_models task dropped — channel closed \
                         (task cancelled or app shutting down)"
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
        if !crate::delegation_grants::parent_allows(params.tool_grant.as_deref(), &qualified) {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Auth".into(),
                    message: format!(
                        "Parent grant does not permit '{qualified}'. Configure kask.mcp.delegated_tools for the calling server; its request list cannot grant authority."
                    ),
                },
            };
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
                    message: "GPUI-side worktree_spawn task dropped — channel closed \
                         (task cancelled or app shutting down)"
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

    // GenerateBatch requests are dispatched to the provider's Batch API
    // (OpenRouter or DeepInfra). The zed side reads the API key from the
    // keychain via the GPUI-side credential channel, then calls
    // `hkask_inference::batch::submit_batch`. The MCP server never sees the
    // API key.
    if matches!(request.method, InferenceMethod::GenerateBatch) {
        let model = params.model_override.as_deref().unwrap_or("");
        let prompts = params.batch_prompts.as_deref().unwrap_or(&[]);
        let max_tokens = params.batch_max_tokens.unwrap_or(2000);
        let temperature = params.parameters.temperature;

        if prompts.is_empty() {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "InvalidArgument".to_string(),
                    message: "batch_prompts is empty — cannot submit an empty batch".to_string(),
                },
            };
        }

        // Detect the provider from the model name
        let Some((provider, clean_model)) = hkask_inference::batch::detect_batch_provider(model)
        else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "InvalidArgument".to_string(),
                    message: format!(
                        "model '{model}' is not batch-eligible — use a ':batch' suffix \
                         (OpenRouter) or 'DeepInfra/' prefix (DeepInfra), or set \
                         HKASK_BATCH_PROVIDER"
                    ),
                },
            };
        };

        // Read the API key from the keychain via the GPUI-side channel. One
        // key, one location: the provider's key lives at its `api_url`
        // keychain slot — the same slot zed's `ApiKeyState`, MCP env
        // injection, and the settings UI read.
        let credential_key = match provider {
            hkask_inference::batch::BatchProvider::OpenRouter => "openrouter",
            hkask_inference::batch::BatchProvider::DeepInfra => "deepinfra",
        };
        let Some(provider_descriptor) =
            crate::inference_providers::provider_by_credential_key(credential_key)
        else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Internal".to_string(),
                    message: format!(
                        "batch provider credential key '{credential_key}' has no \
                         INFERENCE_PROVIDERS entry — the descriptor table and \
                         hkask-inference's BatchProvider enum diverged"
                    ),
                },
            };
        };
        let credential_url = provider_descriptor.api_url;
        let (tx_reply, rx_reply) = oneshot::channel::<Result<String, String>>();
        if batch_credential_tx
            .send((credential_url.to_string(), tx_reply))
            .is_err()
        {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Connection".to_string(),
                    message: "GPUI-side credential task dropped — channel closed \
                         (task cancelled or app shutting down)"
                        .to_string(),
                },
            };
        }
        let api_key = match rx_reply.await {
            Ok(Ok(key)) => key,
            Ok(Err(e)) => {
                return InferenceOutcome::Error {
                    error: InferenceErrorPayload {
                        code: "PermissionDenied".to_string(),
                        message: format!(
                            "batch API requires {} (keychain slot {credential_url}): \
                             {e}. Set the API key via Settings → AI → LLM Providers.",
                            provider_descriptor.env_var
                        ),
                    },
                };
            }
            Err(e) => {
                return InferenceOutcome::Error {
                    error: InferenceErrorPayload {
                        code: "Connection".to_string(),
                        message: format!("credential channel failed: {e}"),
                    },
                };
            }
        };

        // Submit the batch and wait for results — pass the IPC
        // `BatchPromptEntry` directly; `submit_batch` accepts it.
        match hkask_inference::batch::submit_batch(
            provider,
            &api_key,
            &clean_model,
            prompts,
            max_tokens,
            temperature,
        )
        .await
        {
            Ok(batch_result) => {
                let results: Vec<BatchResultEntry> = batch_result
                    .results
                    .into_iter()
                    .map(|(custom_id, r)| match r {
                        Ok(success) => BatchResultEntry {
                            custom_id,
                            text: Some(success.text),
                            total_tokens: success.total_tokens,
                            error: None,
                        },
                        Err(err_msg) => BatchResultEntry {
                            custom_id,
                            text: None,
                            total_tokens: 0,
                            error: Some(err_msg),
                        },
                    })
                    .collect();
                tracing::info!(
                    target: "hkask.inference.batch",
                    succeeded = batch_result.succeeded,
                    failed = batch_result.failed,
                    "Batch completed"
                );
                return InferenceOutcome::BatchResults { results };
            }
            Err(e) => {
                return InferenceOutcome::Error {
                    error: InferenceErrorPayload {
                        code: "Internal".to_string(),
                        message: format!("batch API failed: {e}"),
                    },
                };
            }
        }
    }

    // Rerank requests are dispatched to the provider's rerank endpoint
    // (OpenRouter `/api/v1/rerank`). The zed side reads the API key from the
    // keychain via the GPUI-side credential channel, then calls
    // `hkask_inference::rerank::rerank_documents`. The MCP server never
    // sees the API key.
    if matches!(request.method, InferenceMethod::Rerank) {
        let model = params.rerank_model.as_deref().unwrap_or("");
        let query = params.rerank_query.as_deref().unwrap_or("");
        let documents = params.rerank_documents.as_deref().unwrap_or(&[]);

        if query.is_empty() || documents.is_empty() {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "InvalidArgument".to_string(),
                    message: "rerank requires rerank_query and at least one \
                         rerank_document"
                        .to_string(),
                },
            };
        }

        // Detect the provider from the model prefix. Only OpenRouter has a
        // rerank endpoint among the registered providers.
        let Some((_provider, clean_model)) = hkask_inference::rerank::detect_rerank_provider(model)
        else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "InvalidArgument".to_string(),
                    message: format!(
                        "rerank model '{model}' is not rerank-eligible — use an \
                         'OpenRouter/'-prefixed rerank model (e.g. \
                         OpenRouter/qwen/qwen3-reranker-8b)"
                    ),
                },
            };
        };

        // Read the API key from the keychain via the GPUI-side channel. One
        // key, one location: OpenRouter's key lives at its `api_url`
        // keychain slot — the same slot zed's `ApiKeyState`, MCP env
        // injection, and the settings UI read.
        let Some(openrouter_descriptor) =
            crate::inference_providers::provider_by_credential_key("openrouter")
        else {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Internal".to_string(),
                    message: "rerank provider 'openrouter' has no INFERENCE_PROVIDERS \
                             entry — the descriptor table diverged"
                        .to_string(),
                },
            };
        };
        let credential_url = openrouter_descriptor.api_url;
        let (tx_reply, rx_reply) = oneshot::channel::<Result<String, String>>();
        if batch_credential_tx
            .send((credential_url.to_string(), tx_reply))
            .is_err()
        {
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "Connection".to_string(),
                    message: "GPUI-side credential task dropped — channel closed \
                         (task cancelled or app shutting down)"
                        .to_string(),
                },
            };
        }
        let api_key = match rx_reply.await {
            Ok(Ok(key)) => key,
            Ok(Err(e)) => {
                return InferenceOutcome::Error {
                    error: InferenceErrorPayload {
                        code: "PermissionDenied".to_string(),
                        message: format!(
                            "rerank requires {} (keychain slot {credential_url}): \
                             {e}. Set the API key via Settings → AI → LLM Providers.",
                            openrouter_descriptor.env_var
                        ),
                    },
                };
            }
            Err(e) => {
                return InferenceOutcome::Error {
                    error: InferenceErrorPayload {
                        code: "Connection".to_string(),
                        message: format!("credential channel failed: {e}"),
                    },
                };
            }
        };

        match hkask_inference::rerank::rerank_documents(&api_key, &clean_model, query, documents)
            .await
        {
            Ok(scores) => {
                tracing::info!(
                    target: "hkask.inference.rerank",
                    scored = scores.len(),
                    model = %clean_model,
                    "Rerank completed"
                );
                return InferenceOutcome::RerankScores { scores };
            }
            Err(e) => {
                return InferenceOutcome::Error {
                    error: InferenceErrorPayload {
                        code: "Internal".to_string(),
                        message: format!("rerank API failed: {e}"),
                    },
                };
            }
        }
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
        // These variants are handled by early-return blocks above. If this
        // arm is reached, a future enum variant was added without a matching
        // early-return — return an error instead of panicking on a
        // peer-supplied value (DoS vector).
        InferenceMethod::Embed
        | InferenceMethod::ListModels
        | InferenceMethod::ToolInvoke
        | InferenceMethod::CreateWorktreeThread
        | InferenceMethod::GenerateBatch
        | InferenceMethod::Rerank => {
            tracing::error!(
                target: "reg.inference",
                method = ?request.method,
                "dispatch reached the unreachable arm — a new InferenceMethod variant \
                 likely lacks an early-return block"
            );
            return InferenceOutcome::Error {
                error: InferenceErrorPayload {
                    code: "NotImplemented".to_string(),
                    message: format!(
                        "inference method {:?} not implemented in dispatch",
                        request.method
                    ),
                },
            };
        }
    };

    match result {
        Ok(result) => InferenceOutcome::Result { result },
        Err(error) => InferenceOutcome::Error {
            error: InferenceErrorPayload::from(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_tool_port::{ToolFuture, ToolInfo, ToolPort, ToolPortError};
    use hkask_types::inference_ipc::InferenceParams;
    use hkask_types::{
        ChatMessage, ChatToolDefinition, InferenceResult, InferenceUsage, LLMParameters,
    };
    use std::future::Future;
    use std::pin::Pin;

    // ── Mock InferencePort ──────────────────────────────────────────
    //
    // A canned-response mock for testing `dispatch` without a real LLM.
    // Returns a fixed `InferenceResult` for every `generate*` call.

    struct CannedInferencePort;

    fn canned_result() -> InferenceResult {
        InferenceResult {
            text: "canned response".to_string(),
            model: "test-model".to_string(),
            usage: InferenceUsage::default(),
            finish_reason: "stop".to_string(),
            tool_calls: Vec::new(),
            reasoning: None,
            cost_usd: None,
        }
    }

    impl InferencePort for CannedInferencePort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            Box::pin(async { Ok(canned_result()) })
        }

        fn generate_with_messages(
            &self,
            _messages: &[ChatMessage],
            _parameters: &LLMParameters,
            _model_override: Option<&str>,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            Box::pin(async { Ok(canned_result()) })
        }

        fn generate_vision(
            &self,
            _prompt: &str,
            _images: &[String],
            _parameters: &LLMParameters,
            _model_override: Option<&str>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            Box::pin(async { Ok(canned_result()) })
        }
    }

    // ── Mock ToolPort ──────────────────────────────────────────────────────
    //
    // A canned-response mock for testing the `tool_invoke` dispatch path.
    // Returns a fixed JSON result for every call.

    struct CannedToolPort;

    impl ToolPort for CannedToolPort {
        fn invoke<'a>(
            &'a self,
            server: &'a str,
            tool: &'a str,
            _args: serde_json::Value,
            _agent: hkask_types::WebID,
        ) -> ToolFuture<'a, Result<serde_json::Value, ToolPortError>> {
            Box::pin(async move {
                Ok(serde_json::json!({"result": "ok", "server": server, "tool": tool}))
            })
        }

        fn discover_tools<'a>(&'a self) -> ToolFuture<'a, Vec<String>> {
            Box::pin(async { Vec::new() })
        }

        fn get_tool_info<'a>(&'a self, _tool_name: &'a str) -> ToolFuture<'a, Option<ToolInfo>> {
            Box::pin(async { None })
        }
    }

    struct RecordingToolPort(std::sync::atomic::AtomicUsize);
    impl ToolPort for RecordingToolPort {
        fn invoke<'a>(&'a self, _server: &'a str, _tool: &'a str, _args: serde_json::Value, _agent: hkask_types::WebID) -> ToolFuture<'a, Result<serde_json::Value, ToolPortError>> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async { Ok(serde_json::json!({"ok":true})) })
        }
        fn discover_tools<'a>(&'a self) -> ToolFuture<'a, Vec<String>> { Box::pin(async { Vec::new() }) }
        fn get_tool_info<'a>(&'a self, _: &'a str) -> ToolFuture<'a, Option<ToolInfo>> { Box::pin(async { None }) }
    }

    /// expect: "A child cannot enlarge its parent-held grant or invoke tools with inference-only access" [P1]
    #[tokio::test]
    async fn ipc_child_cannot_expand_parent_grant() {
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let recording = Arc::new(RecordingToolPort(std::sync::atomic::AtomicUsize::new(0)));
        let tools: Arc<dyn ToolPort> = recording.clone();
        let server = format!("grant-test-{}", uuid::Uuid::new_v4());
        let token = crate::delegation_grants::grant_for_server(&server, &["kanban/read".into()]).expect("grant");
        for grant in [None, Some(token.clone())] {
            let mut request = make_tool_invoke_request("kanban", "write", Some(vec!["kanban/write".into()]));
            request.params.tool_grant = grant;
            let outcome = dispatch(&port, None, Some(&tools), &make_list_models_tx(), None, &make_batch_credential_tx(), request).await;
            let InferenceOutcome::Error { error } = outcome else { panic!("unauthorized dispatch succeeded"); };
            assert_eq!(error.code, "Auth");
        }
        assert_eq!(recording.0.load(std::sync::atomic::Ordering::SeqCst), 0);
        let mut request = make_tool_invoke_request("kanban", "read", Some(vec!["kanban/read".into()]));
        request.params.tool_grant = Some(token);
        assert!(matches!(dispatch(&port, None, Some(&tools), &make_list_models_tx(), None, &make_batch_credential_tx(), request).await, InferenceOutcome::ToolResult { .. }));
        assert_eq!(recording.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        crate::revoke_delegation_grant(&server);
    }

    // ── CappedReader tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn capped_reader_reads_normal_line() {
        let input = b"hello world\n";
        let mut reader = CappedReader::new(&input[..]);
        let line = reader.read_line().await.unwrap().unwrap();
        assert_eq!(line, "hello world");
    }

    #[tokio::test]
    async fn capped_reader_returns_none_on_eof() {
        let input = b"";
        let mut reader = CappedReader::new(&input[..]);
        assert!(reader.read_line().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn capped_reader_returns_none_on_eof_after_line() {
        let input = b"first\n";
        let mut reader = CappedReader::new(&input[..]);
        let line = reader.read_line().await.unwrap().unwrap();
        assert_eq!(line, "first");
        // No more data — clean EOF.
        assert!(reader.read_line().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn capped_reader_rejects_oversized_line() {
        // Construct a line larger than MAX_IPC_LINE_BYTES without a newline.
        let oversized = vec![b'A'; (MAX_IPC_LINE_BYTES + 1) as usize];
        let mut reader = CappedReader::new(&oversized[..]);
        let result = reader.read_line().await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn capped_reader_accepts_line_at_exact_boundary() {
        // A line of exactly MAX_IPC_LINE_BYTES followed by a newline is valid.
        let mut input = vec![b'B'; MAX_IPC_LINE_BYTES as usize];
        input.push(b'\n');
        let mut reader = CappedReader::new(&input[..]);
        let line = reader.read_line().await.unwrap().unwrap();
        assert_eq!(line.len(), MAX_IPC_LINE_BYTES as usize);
    }

    // ─ dispatch: tool_invoke authority boundary tests ──────────────────
    //
    // These tests pin the fail-closed `tool_allowlist` enforcement at the
    // IPC dispatch boundary. The enforcement code is at lines ~682-706 of
    // this file. A regression that weakens the gate (e.g. removing the
    // allowlist check, or defaulting to allow-all) would go undetected
    // without these tests.
    //
    // Referenced in DIVERGENCE.md D8 as `dispatch_tool_invoke_rejects_unallowed_tool`
    // and in D23 as `dispatch_generate_returns_canned_result`.

    fn make_list_models_tx()
    -> Arc<tokio::sync::mpsc::UnboundedSender<(tokio::sync::oneshot::Sender<Vec<ModelListEntry>>,)>>
    {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<(
            tokio::sync::oneshot::Sender<Vec<ModelListEntry>>,
        )>();
        Arc::new(tx)
    }

    fn make_batch_credential_tx() -> Arc<tokio::sync::mpsc::UnboundedSender<BatchCredentialRequest>>
    {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<BatchCredentialRequest>();
        Arc::new(tx)
    }

    fn make_tool_invoke_request(
        server: &str,
        tool: &str,
        allowlist: Option<Vec<String>>,
    ) -> InferenceRequest {
        InferenceRequest {
            id: 1,
            method: InferenceMethod::ToolInvoke,
            params: InferenceParams {
                tool_server: Some(server.to_string()),
                tool_name: Some(tool.to_string()),
                tool_args: Some(serde_json::Value::Null),
                tool_allowlist: allowlist,
                ..Default::default()
            },
        }
    }

    #[tokio::test]
    async fn dispatch_tool_invoke_rejects_unallowed_tool() {
        // The tool is not in the allowlist — dispatch must fail closed
        // with a "ToolPort" error, never calling the tool port.
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let tool_port: Arc<dyn ToolPort> = Arc::new(CannedToolPort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let request = make_tool_invoke_request(
            "kanban",
            "kanban_task_create",
            Some(vec!["swarm/swarm_delegate".to_string()]),
        );

        let outcome = dispatch(
            &port,
            None,
            Some(&tool_port),
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "ToolPort");
                assert!(
                    error
                        .message
                        .contains("not in the delegated tool allowlist")
                );
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_tool_invoke_rejects_missing_allowlist() {
        // A missing allowlist is a protocol violation — fail closed,
        // never an implicit grant-all.
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let tool_port: Arc<dyn ToolPort> = Arc::new(CannedToolPort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let request = make_tool_invoke_request("kanban", "kanban_task_create", None);

        let outcome = dispatch(
            &port,
            None,
            Some(&tool_port),
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "ToolPort");
                assert!(error.message.contains("missing tool_allowlist"));
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_tool_invoke_rejects_empty_allowlist() {
        // An empty allowlist is also a protocol violation — fail closed.
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let tool_port: Arc<dyn ToolPort> = Arc::new(CannedToolPort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let request = make_tool_invoke_request("kanban", "kanban_task_create", Some(vec![]));

        let outcome = dispatch(
            &port,
            None,
            Some(&tool_port),
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "ToolPort");
                assert!(error.message.contains("missing tool_allowlist"));
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_tool_invoke_allows_listed_tool() {
        // The tool IS in the allowlist — dispatch must succeed and return
        // the tool result.
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let tool_port: Arc<dyn ToolPort> = Arc::new(CannedToolPort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let mut request = make_tool_invoke_request(
            "kanban",
            "kanban_task_create",
            Some(vec!["kanban/kanban_task_create".to_string()]),
        );

        request.params.tool_grant = crate::delegation_grants::grant_for_server("allow-test", &["kanban/kanban_task_create".into()]);

        let outcome = dispatch(
            &port,
            None,
            Some(&tool_port),
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::ToolResult { result } => {
                assert_eq!(result["result"], "ok");
                assert_eq!(result["tool"], "kanban_task_create");
            }
            other => panic!("expected tool result outcome, got {other:?}"),
        }
    }

    // ── dispatch: missing-port error paths ──────────────────────────────

    #[tokio::test]
    async fn dispatch_tool_invoke_errors_without_tool_port() {
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let request = make_tool_invoke_request(
            "kanban",
            "kanban_task_create",
            Some(vec!["kanban/kanban_task_create".to_string()]),
        );

        let outcome = dispatch(
            &port,
            None,
            None, // no tool port
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "Connection");
                assert!(error.message.contains("tool dispatch not configured"));
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    /// expect: "IPC rejects a different embedding destination before HTTP dispatch" [P1]
    #[tokio::test]
    async fn ipc_embedding_provider_mismatch_is_invalid_request() {
        let http_client =
            http_client::FakeHttpClient::create(|_| async { panic!("mismatch reached HTTP") });
        let provider = crate::INFERENCE_PROVIDERS
            .iter()
            .find(|provider| provider.id == "OpenRouter")
            .expect("registered provider");
        let embedding = crate::LanguageModelEmbeddingPort::new(
            crate::ResolvedEmbeddingCredentials {
                provider,
                api_key: "fixture-key".into(),
            },
            http_client,
            tokio::runtime::Handle::current(),
        );
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let outcome = dispatch(
            &port,
            Some(&embedding),
            None,
            &make_list_models_tx(),
            None,
            &make_batch_credential_tx(),
            InferenceRequest {
                id: 1,
                method: InferenceMethod::Embed,
                params: InferenceParams {
                    embed_model: Some("ollama/local-model".into()),
                    embed_texts: Some(vec!["private source".into()]),
                    ..Default::default()
                },
            },
        )
        .await;
        let InferenceOutcome::Error { error } = outcome else {
            panic!("mismatch must fail");
        };
        assert_eq!(error.code, "InvalidRequest");
        assert!(error.message.contains("OpenRouter"));
    }

    #[tokio::test]
    async fn dispatch_embed_errors_without_embedding_port() {
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let request = InferenceRequest {
            id: 1,
            method: InferenceMethod::Embed,
            params: InferenceParams {
                embed_model: Some("test/model".to_string()),
                embed_texts: Some(vec!["hello".to_string()]),
                ..Default::default()
            },
        };

        let outcome = dispatch(
            &port,
            None, // no embedding port
            None,
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "Connection");
                assert!(error.message.contains("embedding port not configured"));
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    // ── dispatch: generate paths ────────────────────────────────────────

    #[tokio::test]
    async fn dispatch_generate_returns_canned_result() {
        // Pins the basic `generate` dispatch path — the InferencePort is
        // called and the result is returned as `InferenceOutcome::Result`.
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let request = InferenceRequest {
            id: 42,
            method: InferenceMethod::Generate,
            params: InferenceParams {
                prompt: Some("hello".to_string()),
                parameters: LLMParameters::default(),
                ..Default::default()
            },
        };

        let outcome = dispatch(
            &port,
            None,
            None,
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Result { result } => {
                assert_eq!(result.text, "canned response");
                assert_eq!(result.model, "test-model");
            }
            other => panic!("expected result outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_generate_with_messages_returns_canned_result() {
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let request = InferenceRequest {
            id: 1,
            method: InferenceMethod::GenerateWithMessages,
            params: InferenceParams {
                messages: Some(vec![
                    ChatMessage::system("You are a test."),
                    ChatMessage::user("hello"),
                ]),
                parameters: LLMParameters::default(),
                ..Default::default()
            },
        };

        let outcome = dispatch(
            &port,
            None,
            None,
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Result { result } => {
                assert_eq!(result.text, "canned response");
            }
            other => panic!("expected result outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_generate_vision_returns_canned_result() {
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let request = InferenceRequest {
            id: 1,
            method: InferenceMethod::GenerateVision,
            params: InferenceParams {
                prompt: Some("describe this image".to_string()),
                images: Some(vec!["base64data".to_string()]),
                parameters: LLMParameters::default(),
                ..Default::default()
            },
        };

        let outcome = dispatch(
            &port,
            None,
            None,
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Result { result } => {
                assert_eq!(result.text, "canned response");
            }
            other => panic!("expected result outcome, got {other:?}"),
        }
    }

    // ── dispatch: tool_invoke missing fields ────────────────────────────

    #[tokio::test]
    async fn dispatch_tool_invoke_errors_without_tool_server() {
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let tool_port: Arc<dyn ToolPort> = Arc::new(CannedToolPort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let request = InferenceRequest {
            id: 1,
            method: InferenceMethod::ToolInvoke,
            params: InferenceParams {
                tool_server: None, // missing
                tool_name: Some("kanban_task_create".to_string()),
                tool_allowlist: Some(vec!["kanban/kanban_task_create".to_string()]),
                ..Default::default()
            },
        };

        let outcome = dispatch(
            &port,
            None,
            Some(&tool_port),
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "ToolPort");
                assert!(error.message.contains("missing tool_server"));
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_tool_invoke_errors_without_tool_name() {
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let tool_port: Arc<dyn ToolPort> = Arc::new(CannedToolPort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let request = InferenceRequest {
            id: 1,
            method: InferenceMethod::ToolInvoke,
            params: InferenceParams {
                tool_server: Some("kanban".to_string()),
                tool_name: None, // missing
                tool_allowlist: Some(vec!["kanban/kanban_task_create".to_string()]),
                ..Default::default()
            },
        };

        let outcome = dispatch(
            &port,
            None,
            Some(&tool_port),
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "ToolPort");
                assert!(error.message.contains("missing tool_name"));
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    // ── dispatch: early-return coverage for non-generate methods ───────
    //
    // P17 / P3: the `dispatch` function has a defensive error arm at the
    // bottom of the final `match request.method` that catches
    // `Embed | ListModels | ToolInvoke | CreateWorktreeThread` and returns
    // `InferenceOutcome::Error` instead of panicking (the prior `unreachable!`
    // was a DoS vector on peer-supplied values). That arm is only reachable
    // if a non-generate method bypasses its early-return block — which the
    // early-returns make impossible. These tests pin that each non-generate
    // method is caught by its early-return (and thus the defensive arm is
    // never hit): if a future refactor removes an early-return, the
    // defensive arm would be reached and these tests would still pass —
    // but the defensive arm's error message is distinct, so a regression
    // test that asserts the early-return's specific error message catches
    // the removal.

    #[tokio::test]
    async fn dispatch_list_models_errors_when_channel_dropped() {
        // `ListModels` is caught by an early-return that sends on
        // `list_models_tx`. If the receiver was dropped (server shutting
        // down), dispatch must return a Connection error — not reach the
        // defensive arm. This pins the early-return for `ListModels`.
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        // Create a channel whose receiver is immediately dropped — `send`
        // will return `Err`.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<(
            tokio::sync::oneshot::Sender<Vec<ModelListEntry>>,
        )>();
        drop(rx);
        let list_models_tx = Arc::new(tx);
        let batch_credential_tx = make_batch_credential_tx();

        let request = InferenceRequest {
            id: 1,
            method: InferenceMethod::ListModels,
            params: InferenceParams::default(),
        };

        let outcome = dispatch(
            &port,
            None,
            None,
            &list_models_tx,
            None,
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "Connection");
                assert!(
                    error.message.contains("list_models task dropped"),
                    "expected list_models task-dropped error, got: {}",
                    error.message
                );
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_create_worktree_thread_errors_without_spawn_port() {
        // `CreateWorktreeThread` is caught by an early-return that checks
        // for `worktree_spawn_tx`. If the port is absent (no active
        // workspace), dispatch must return a Connection error — not reach
        // the defensive arm. This pins the early-return for
        // `CreateWorktreeThread`.
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let request = InferenceRequest {
            id: 1,
            method: InferenceMethod::CreateWorktreeThread,
            params: InferenceParams {
                worktree_prompt: Some("do a thing".to_string()),
                worktree_title: Some("Test Task".to_string()),
                ..Default::default()
            },
        };

        let outcome = dispatch(
            &port,
            None,
            None,
            &list_models_tx,
            None, // no worktree spawn port
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "Connection");
                assert!(
                    error.message.contains("worktree spawn port not configured"),
                    "expected worktree-spawn-port-not-configured error, got: {}",
                    error.message
                );
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_create_worktree_thread_errors_when_channel_dropped() {
        // `CreateWorktreeThread` early-return: the spawn port is present but
        // the receiver was dropped (server shutting down). Dispatch must
        // return a Connection error — not reach the defensive arm.
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let list_models_tx = make_list_models_tx();
        let batch_credential_tx = make_batch_credential_tx();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<WorktreeSpawnRequest>();
        drop(rx);
        let worktree_spawn_tx = Arc::new(tx);

        let request = InferenceRequest {
            id: 1,
            method: InferenceMethod::CreateWorktreeThread,
            params: InferenceParams {
                worktree_prompt: Some("do a thing".to_string()),
                worktree_title: Some("Test Task".to_string()),
                ..Default::default()
            },
        };

        let outcome = dispatch(
            &port,
            None,
            None,
            &list_models_tx,
            Some(&worktree_spawn_tx),
            &batch_credential_tx,
            request,
        )
        .await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "Connection");
                assert!(
                    error.message.contains("worktree_spawn task dropped"),
                    "expected worktree-spawn-task-dropped error, got: {}",
                    error.message
                );
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    // ── Socket path / directory tests ──────────────────────────────────

    #[test]
    fn generate_socket_path_produces_unique_paths() {
        let path_a = generate_socket_path().unwrap();
        // A tiny delay ensures the nonce (nanosecond timestamp) differs.
        std::thread::sleep(std::time::Duration::from_millis(1));
        let path_b = generate_socket_path().unwrap();
        assert_ne!(path_a, path_b, "socket paths must be unique");
    }

    #[test]
    fn generate_socket_path_is_in_private_dir() {
        let path = generate_socket_path().unwrap();
        let parent = path.parent().unwrap();
        // The parent directory must exist (ensure_private_dir created it).
        assert!(parent.exists(), "socket parent dir must exist");
        // Verify the directory is owner-only (0700) on unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(parent).unwrap();
            let mode = metadata.permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "socket dir must be 0700, got {mode:o}");
        }
        // Clean up the directory we created.
        let _ = std::fs::remove_dir(parent);
    }

    #[test]
    fn ensure_private_dir_creates_0700_dir() {
        let temp = std::env::temp_dir();
        let test_dir = temp.join(format!("kask-test-ensure-private-{}", std::process::id()));
        // Clean up any stale dir from a prior run.
        let _ = std::fs::remove_dir_all(&test_dir);

        ensure_private_dir(&test_dir).unwrap();

        assert!(test_dir.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&test_dir).unwrap();
            let mode = metadata.permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "dir must be 0700, got {mode:o}");
        }

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn ensure_private_dir_tightens_existing_dir() {
        let temp = std::env::temp_dir();
        let test_dir = temp.join(format!("kask-test-ensure-tighten-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);

        // Create the dir with overly-permissive mode first.
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o755)
                .create(&test_dir)
                .unwrap();
        }

        // ensure_private_dir should tighten it to 0700.
        ensure_private_dir(&test_dir).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&test_dir).unwrap();
            let mode = metadata.permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "dir must be tightened to 0700, got {mode:o}");
        }

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    // ── Embedding error classification test ────────────────────────────
    //
    // Pins the fix for the review finding that all embedding errors were
    // labeled "Connection". The dispatch must now classify
    // EmbeddingGenerationError variants into meaningful error codes.

    #[test]
    fn embed_error_classifies_json_as_json_not_connection() {
        // The classification logic in `dispatch` maps each
        // EmbeddingGenerationError variant to a distinct error code.
        // Previously all variants were labeled "Connection", misleading
        // operators into diagnosing network issues for parse errors.
        let err = hkask_types::EmbeddingGenerationError::Json("test".to_string());
        let (code, message) = match err {
            hkask_types::EmbeddingGenerationError::InvalidRequest(m) => ("InvalidRequest", m),
            hkask_types::EmbeddingGenerationError::Connection(m) => ("Connection", m),
            hkask_types::EmbeddingGenerationError::Api(s, m) => ("Api", format!("status {s}: {m}")),
            hkask_types::EmbeddingGenerationError::Json(m) => ("Json", m),
            hkask_types::EmbeddingGenerationError::EmptyResponse => {
                ("Api", "empty response".to_string())
            }
            hkask_types::EmbeddingGenerationError::DimensionMismatch { expected, actual } => {
                ("Api", format!("dim mismatch: {expected} vs {actual}"))
            }
        };
        assert_eq!(code, "Json");
        assert_eq!(message, "test");
    }
}
