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
//! MCP children share the parent's listener and open a fresh connection per
//! request. EOF cancels pending local dispatch; pipelining is refused. Cancellation
//! cannot reverse work already accepted by a provider or tool, so no unknown-effect
//! request is automatically replayed.

use std::path::PathBuf;
use std::sync::Arc;

use hkask_types::inference_ipc::{
    InferenceErrorPayload, InferenceMethod, InferenceOutcome, InferenceRequest, InferenceResponse,
    ModelListEntry, WorktreeThreadInfo,
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
pub(crate) struct WorktreeSpawnRequest {
    prompt: String,
    title: String,
    worktree_name: Option<String>,
    base_ref: Option<String>,
    grant: String,
    allowed_tools: Vec<String>,
    reply: oneshot::Sender<Result<WorktreeThreadInfo, InferenceErrorPayload>>,
}

const WORKTREE_QUEUE_CAPACITY: usize = 32;

impl WorktreeSpawnRequest {
    /// Recheck revocation and cancellation at the actual admission boundary.
    async fn execute<F, Fut>(self, spawn: F)
    where
        F: FnOnce(String, String, Option<String>, Option<String>, Vec<String>) -> Fut,
        Fut: std::future::Future<Output = Result<WorktreeThreadInfo, String>>,
    {
        if self.reply.is_closed() {
            return;
        }
        let result = match crate::delegation_grants::worktree_tools(
            Some(&self.grant),
            Some(&self.allowed_tools),
        ) {
            Some(tools) => spawn(
                self.prompt,
                self.title,
                self.worktree_name,
                self.base_ref,
                tools,
            )
            .await
            .map_err(|message| InferenceErrorPayload {
                code: "WorktreeSpawn".into(),
                message,
                status: None,
            }),
            None => Err(InferenceErrorPayload {
                code: "Auth".into(),
                message: "Parent worktree grant was revoked before execution".into(),
                status: None,
            }),
        };
        if self.reply.send(result).is_err() {
            tracing::debug!(target: "reg.inference", "Worktree caller disconnected after admission; effects may have occurred");
        }
    }
}

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
        allowed_tools: Vec<String>,
        cx: &mut gpui::AsyncApp,
    ) -> gpui::Task<Result<WorktreeThreadInfo, String>>;
}

/// Process-global worktree spawner. Set by `main.rs` when a workspace with an
/// `AgentPanel` opens; read by the GPUI-side IPC task at admission. When absent,
/// worktree spawn fails visibly; the caller must not start a fallback executor.
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
/// (list_models, worktree_spawn, provider_credential) are **detached** in
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

        // Delegated `host/skill` activation reads the GPUI-held skill catalog;
        // same channel pattern as `ListModels` below.
        let (skill_tx, mut skill_rx) =
            tokio::sync::mpsc::unbounded_channel::<crate::host_skill_tools::SkillRequest>();
        // Detached: see the list_models task below.
        cx.spawn(async move |cx| {
            while let Some((name, reply)) = skill_rx.recv().await {
                let result = agent::activate_delegated_skill(name, cx).await;
                if reply.send(result).is_err() {
                    tracing::debug!(target: "reg.inference", "host/skill caller dropped before reply");
                }
            }
        })
        .detach();
        let tools = tool_port.map(|inner| {
            Arc::new(crate::host_skill_tools::HostSkillToolPort {
                inner,
                skill_tx: skill_tx.clone(),
            }) as Arc<dyn hkask_tool_port::ToolPort>
        });
        // zed-kask: P7h — the serving backend lives behind a swappable slot
        // so the model-resolved re-wire replaces the ports in place
        // (`swap_inference_ipc_ports`) instead of starting a second server
        // at a new socket path. The per-instance path made the no-op→real
        // transition an env CHANGE — restart-by-design — killing and
        // respawning every MCP child launched against the no-op server
        // (measured 2026-10-02: 30 restart events, portfolio 4× in 13 s,
        // ~2.5 min to fleet stability).
        let backend_slot = Arc::new(IpcBackendSlot::new(
            IpcBackend {
                inference: inference_port,
                embedding: embedding_port,
                tools,
            },
            skill_tx,
        ));
        {
            let mut guard = match IPC_BACKEND_SLOT.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    tracing::warn!(
                        target: "hkask.inference.ipc",
                        "IPC_BACKEND_SLOT mutex poisoned — recovering via into_inner"
                    );
                    poisoned.into_inner()
                }
            };
            if guard.is_some() {
                tracing::warn!(
                    target: "hkask.inference.ipc",
                    "a second InferenceIpcServer::start is replacing the running server's \
                     backend slot — one server per process is the design (P7h); the previous \
                     socket is orphaned"
                );
            }
            *guard = Some(backend_slot.clone());
        }

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
            tokio::sync::mpsc::channel::<WorktreeSpawnRequest>(WORKTREE_QUEUE_CAPACITY);
        // Detached: see the list_models task above.
        cx.spawn(async move |cx| {
            while let Some(request) = worktree_spawn_rx.recv().await {
                request
                    .execute(|prompt, title, name, base_ref, tools| {
                        match shared_worktree_spawner() {
                            Some(spawner) => {
                                spawner.spawn(prompt, title, name, base_ref, tools, cx)
                            }
                            None => gpui::Task::ready(Err(
                                "worktree spawner not configured (no active workspace)".into(),
                            )),
                        }
                    })
                    .await;
            }
        })
        .detach();

        let worktree_spawn_tx = Arc::new(worktree_spawn_tx);

        // Batch credential channel — same pattern as `list_models_tx`. The
        // GPUI-side task reads API keys from zed's `CredentialsProvider`
        // keychain and returns them via the oneshot reply channel. The tokio-
        // side dispatch uses this to get the provider API key for batch
        // inference calls — the key never leaves the zed process.
        let (provider_credential_tx, mut provider_credential_rx) =
            tokio::sync::mpsc::unbounded_channel::<BatchCredentialRequest>();
        // Detached: see the list_models task above. This is the channel the
        // rerank dispatch reads the OpenRouter key through — if this task
        // dies, every deep-strategy rerank fails with "GPUI-side credential
        // task dropped".
        cx.spawn(async move |cx| {
            while let Some((credential_url, reply)) = provider_credential_rx.recv().await {
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
        let provider_credential_tx = Arc::new(provider_credential_tx);

        let task = tokio_handle.spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let backend_slot = backend_slot.clone();
                        let list_models_tx = list_models_tx.clone();
                        let worktree_spawn_tx = worktree_spawn_tx.clone();
                        let provider_credential_tx = provider_credential_tx.clone();
                        tokio::spawn(async move {
                            handle_connection(
                                stream,
                                backend_slot,
                                list_models_tx,
                                Some(worktree_spawn_tx),
                                provider_credential_tx,
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

/// The per-request dispatch targets of the running inference IPC server.
/// Held behind [`IpcBackendSlot`] so the model-resolved re-wire replaces
/// them in place — the socket path (and therefore every MCP child's
/// `HKASK_INFERENCE_SOCKET` env) never changes across the no-op→real
/// transition (P7h).
#[derive(Clone)]
pub(crate) struct IpcBackend {
    pub(crate) inference: Arc<dyn InferencePort>,
    pub(crate) embedding: Option<LanguageModelEmbeddingPort>,
    pub(crate) tools: Option<Arc<dyn hkask_tool_port::ToolPort>>,
}

/// The swappable backend holder shared by the accept loop and the
/// model-resolved re-wire. `skill_tx` is the delegated `host/skill`
/// activation channel created once at [`InferenceIpcServer::start`] (its
/// GPUI-side task is model-independent); every tool port installed through
/// [`IpcBackendSlot::swap`] is wrapped in `HostSkillToolPort` against that
/// same channel.
pub(crate) struct IpcBackendSlot {
    backend: std::sync::RwLock<IpcBackend>,
    skill_tx: tokio::sync::mpsc::UnboundedSender<crate::host_skill_tools::SkillRequest>,
}

impl IpcBackendSlot {
    pub(crate) fn new(
        backend: IpcBackend,
        skill_tx: tokio::sync::mpsc::UnboundedSender<crate::host_skill_tools::SkillRequest>,
    ) -> Self {
        Self {
            backend: std::sync::RwLock::new(backend),
            skill_tx,
        }
    }

    /// Replace the serving backend. In-flight requests complete on the
    /// backend they started with; each connection reads the backend per
    /// request, so subsequent requests dispatch to the new one.
    pub(crate) fn swap(
        &self,
        inference: Arc<dyn InferencePort>,
        embedding: Option<LanguageModelEmbeddingPort>,
        tool: Option<Arc<dyn hkask_tool_port::ToolPort>>,
    ) {
        let tools = tool.map(|inner| {
            Arc::new(crate::host_skill_tools::HostSkillToolPort {
                inner,
                skill_tx: self.skill_tx.clone(),
            }) as Arc<dyn hkask_tool_port::ToolPort>
        });
        let mut guard = match self.backend.write() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!(
                    target: "hkask.inference.ipc",
                    "IpcBackendSlot lock poisoned — recovering via into_inner"
                );
                poisoned.into_inner()
            }
        };
        *guard = IpcBackend {
            inference,
            embedding,
            tools,
        };
    }

    /// A snapshot of the current backend. Cheap: three `Arc`/`Option`
    /// clones.
    pub(crate) fn current(&self) -> IpcBackend {
        match self.backend.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => {
                tracing::warn!(
                    target: "hkask.inference.ipc",
                    "IpcBackendSlot lock poisoned — recovering via into_inner"
                );
                poisoned.into_inner().clone()
            }
        }
    }
}

/// The running server's swappable backend — populated by the first
/// [`InferenceIpcServer::start`] in the process. The model-resolved re-wire
/// reaches the running server through this slot instead of starting a second
/// server (P7h).
static IPC_BACKEND_SLOT: std::sync::Mutex<Option<Arc<IpcBackendSlot>>> =
    std::sync::Mutex::new(None);

/// Swap the serving ports of the running inference IPC server — the
/// model-resolved re-wire path (`wire_kask_inference_stack` in zed's
/// `main.rs`). The socket path is unchanged, so no MCP child's env changes
/// and the env-diff restart logic in `sync_kask_mcp_runtime_servers`
/// restarts nothing: the no-op→real transition is a port swap behind one
/// stable socket, not a server replacement (P7h — the per-instance socket
/// path made this transition an env CHANGE; measured 2026-10-02: 30 restart
/// events, portfolio 4× in 13 s, ~2.5 min to fleet stability).
///
/// Returns `false` when no server is running (the no-op start failed, or
/// the model was already configured at deferred-task time so no no-op
/// server was started) — the caller falls back to
/// [`InferenceIpcServer::start`].
pub fn swap_inference_ipc_ports(
    inference_port: Arc<dyn InferencePort>,
    embedding_port: Option<LanguageModelEmbeddingPort>,
    tool_port: Option<Arc<dyn hkask_tool_port::ToolPort>>,
) -> bool {
    let guard = match IPC_BACKEND_SLOT.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!(
                target: "hkask.inference.ipc",
                "IPC_BACKEND_SLOT mutex poisoned — recovering via into_inner"
            );
            poisoned.into_inner()
        }
    };
    match guard.as_ref() {
        Some(slot) => {
            slot.swap(inference_port, embedding_port, tool_port);
            true
        }
        None => false,
    }
}

/// Generate the inference IPC socket path — STABLE for the process
/// lifetime (pid-keyed, no per-instance nonce). The no-op→real port
/// transition swaps the serving backend behind this one socket (see
/// [`IpcBackendSlot`]), so `HKASK_INFERENCE_SOCKET` never changes after the
/// first set and the env-diff restart logic in the zed-side observer sees
/// no change (P7h: the former per-instance nonce made the model-resolved
/// re-wire an env CHANGE — restart-by-design — killing and respawning every
/// MCP child that launched against the no-op server). A second editor
/// process gets a different pid, hence a different path; a stale socket
/// from a crashed process with a reused pid is removed at bind (see
/// [`InferenceIpcServer::start`]).
fn generate_socket_path() -> Result<PathBuf, std::io::Error> {
    let pid = std::process::id();
    Ok(inference_socket_dir()?.join(format!("kask-inference-{pid}.sock")))
}

/// Handle a single connection from an MCP server.
///
/// Reads newline-delimited JSON requests, dispatches them to the inference
/// port, and writes newline-delimited JSON responses.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    backend_slot: Arc<IpcBackendSlot>,
    list_models_tx: Arc<
        tokio::sync::mpsc::UnboundedSender<(tokio::sync::oneshot::Sender<Vec<ModelListEntry>>,)>,
    >,
    worktree_spawn_tx: Option<Arc<tokio::sync::mpsc::Sender<WorktreeSpawnRequest>>>,
    provider_credential_tx: Arc<tokio::sync::mpsc::UnboundedSender<BatchCredentialRequest>>,
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
                    error_class = ?e.classify(),
                    line_number = e.line(),
                    column = e.column(),
                    "Inference IPC parse failed — skipping (request payload withheld)"
                );
                continue;
            }
        };

        let id = request.id;
        // Read the backend per request so a mid-connection port swap (the
        // model-resolved re-wire) applies from the next request; the
        // in-flight request completes on the backend it started with.
        let backend = backend_slot.current();
        // One in-flight request per connection. Monitor EOF while dispatch is
        // pending so dropping a client cancels queued/provider work immediately.
        let outcome = tokio::select! {
            result = dispatch(&backend.inference, backend.embedding.as_ref(), backend.tools.as_ref(),
                &list_models_tx, worktree_spawn_tx.as_ref(), &provider_credential_tx, request) => result,
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

/// Error outcome from a code and message — the one-line form of the
/// per-branch failure construction in `dispatch` (29 former 5-6-line sites).
fn ipc_error(code: &str, message: impl Into<String>) -> InferenceOutcome {
    InferenceOutcome::Error {
        error: InferenceErrorPayload {
            code: code.to_string(),
            message: message.into(),
            status: None,
        },
    }
}

/// Classify an embedding failure into the IPC error outcome. The `Api`
/// status travels in the payload's structured `status` field so the client
/// reconstructs `EmbeddingGenerationError::Api(status, _)` instead of
/// re-parsing the message string; `EmptyResponse` and `DimensionMismatch`
/// carry no HTTP status and stay status-less `Api`-coded payloads.
fn embed_error_outcome(error: hkask_types::EmbeddingGenerationError) -> InferenceOutcome {
    let (code, message, status) = match error {
        hkask_types::EmbeddingGenerationError::InvalidRequest(m) => ("InvalidRequest", m, None),
        hkask_types::EmbeddingGenerationError::Connection(m) => ("Connection", m, None),
        hkask_types::EmbeddingGenerationError::Api(status, m) => ("Api", m, Some(status)),
        hkask_types::EmbeddingGenerationError::Json(m) => ("Json", m, None),
        hkask_types::EmbeddingGenerationError::EmptyResponse => (
            "Api",
            "empty response from embedding model".to_string(),
            None,
        ),
        hkask_types::EmbeddingGenerationError::DimensionMismatch { expected, actual } => (
            "Api",
            format!("dimension mismatch: expected {expected}, got {actual}"),
            None,
        ),
    };
    InferenceOutcome::Error {
        error: InferenceErrorPayload {
            code: code.to_string(),
            message,
            status,
        },
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
    worktree_spawn_tx: Option<&Arc<tokio::sync::mpsc::Sender<WorktreeSpawnRequest>>>,
    provider_credential_tx: &Arc<tokio::sync::mpsc::UnboundedSender<BatchCredentialRequest>>,
    request: InferenceRequest,
) -> InferenceOutcome {
    let params = request.params;

    // Embedding requests are dispatched separately — they return
    // `InferenceOutcome::Embeddings`, not `InferenceOutcome::Result`.
    if matches!(request.method, InferenceMethod::Embed) {
        let Some(emb_port) = embedding_port else {
            return ipc_error(
                "Connection",
                "embedding port not configured on the zed side — the IPC server was started without an embedding port. This indicates a startup wiring bug.",
            );
        };
        let model = params.embed_model.as_deref().unwrap_or("");
        let texts = params.embed_texts.as_deref().unwrap_or(&[]);
        let dimensions = params.embed_dimensions;
        return match emb_port
            .embed_with_dimensions(model, texts, dimensions)
            .await
        {
            Ok(batch) => InferenceOutcome::Embeddings {
                embeddings: batch.vectors,
                requested_model: batch.requested_model,
                actual_model: batch.actual_model,
            },
            Err(e) => embed_error_outcome(e),
        };
    }

    // ListModels requests are dispatched via the GPUI context — they read
    // zed's `LanguageModelRegistry` directly (not through InferencePort).
    if matches!(request.method, InferenceMethod::ListModels) {
        let (tx_reply, rx_reply) = oneshot::channel::<Vec<ModelListEntry>>();
        if list_models_tx.send((tx_reply,)).is_err() {
            return ipc_error(
                "Connection",
                "GPUI-side list_models task dropped — channel closed (task cancelled or app shutting down)",
            );
        }
        match rx_reply.await {
            Ok(models) => return InferenceOutcome::ModelList { models },
            Err(e) => {
                return ipc_error("Connection", format!("list_models channel failed: {e}"));
            }
        }
    }

    if matches!(request.method, InferenceMethod::ToolDefinition) {
        let Some(tool_port) = tool_port else {
            return ipc_error(
                "Connection",
                "tool definition lookup not configured on the zed side",
            );
        };
        let Some(server) = params.tool_server.as_deref() else {
            return ipc_error("ToolPort", "tool_definition request missing tool_server");
        };
        let Some(tool) = params.tool_name.as_deref() else {
            return ipc_error("ToolPort", "tool_definition request missing tool_name");
        };
        let qualified = format!("{server}/{tool}");
        if !params
            .tool_allowlist
            .as_ref()
            .is_some_and(|allowed| allowed.iter().any(|name| name == &qualified))
            || !crate::delegation_grants::parent_allows(params.tool_grant.as_deref(), &qualified)
        {
            return ipc_error(
                "Auth",
                format!(
                    "tool definition '{qualified}' is not permitted by the card allowlist and parent grant"
                ),
            );
        }
        // Server-scoped lookup: the caller names the server, so a same-named
        // tool on another server can neither answer this request nor shadow it.
        let Some(info) = tool_port.get_tool_info(server, tool).await else {
            return ipc_error(
                "ToolPort",
                format!("tool definition '{qualified}' not found on server '{server}'"),
            );
        };
        if !info.input_schema.is_object() {
            return ipc_error(
                "ToolPort",
                format!("tool definition '{qualified}' has a non-object schema"),
            );
        }
        let mut input_schema = info.input_schema;
        language_model_core::tool_schema::normalize_tool_schema(&mut input_schema);
        return InferenceOutcome::ToolDefinition {
            definition: hkask_types::ChatToolDefinition {
                tool_type: "function".into(),
                function: hkask_types::ChatToolFunction {
                    name: qualified,
                    description: info.description,
                    parameters: input_schema,
                },
            },
        };
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
            return ipc_error(
                "Connection",
                "tool dispatch not configured on the zed side — the IPC server was started without a tool port. This indicates a startup wiring bug.",
            );
        };
        let Some(server) = params.tool_server.clone() else {
            return ipc_error("ToolPort", "tool_invoke request missing tool_server");
        };
        let Some(tool) = params.tool_name.clone() else {
            return ipc_error("ToolPort", "tool_invoke request missing tool_name");
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
                    return ipc_error(
                        "ToolPort",
                        format!(
                            "tool '{qualified}' is not in the delegated tool allowlist — refused before minting the panel token"
                        ),
                    );
                }
            }
            _ => {
                return ipc_error(
                    "ToolPort",
                    "tool_invoke request missing tool_allowlist — the delegated tool allowlist must be declared per request (fail closed)",
                );
            }
        }
        if !crate::delegation_grants::parent_allows(params.tool_grant.as_deref(), &qualified) {
            return ipc_error(
                "Auth",
                format!(
                    "Parent grant does not permit '{qualified}'. Configure kask.mcp.delegated_tools for the calling server; its request list cannot grant authority."
                ),
            );
        }
        // Accounting identity for the call meter — not a credential.
        let webid = hkask_types::WebID::from_persona(b"kask-panel");
        return match tool_port.invoke(&server, &tool, args, webid).await {
            Ok(value) => InferenceOutcome::ToolResult { result: value },
            Err(e) => ipc_error("ToolPort", e.to_string()),
        };
    }

    // CreateWorktreeThread requests are dispatched via the GPUI context —
    // they call `SiblingThreadHost::create_sibling_thread` on the zed side,
    // which needs `AsyncApp` (not `Send`). Same channel pattern as
    // `ListModels`.
    if matches!(request.method, InferenceMethod::CreateWorktreeThread) {
        let Some(allowed_tools) = crate::delegation_grants::worktree_tools(
            params.tool_grant.as_deref(),
            params.tool_allowlist.as_deref(),
        ) else {
            return ipc_error(
                "Auth",
                "Worktree creation requires parent-granted host/create_worktree_thread authority and an explicit tool narrowing",
            );
        };
        let Some(ref tx) = worktree_spawn_tx else {
            return ipc_error(
                "Connection",
                "worktree spawn port not configured on the zed side (no active workspace or SiblingThreadHost)",
            );
        };
        let prompt = params.worktree_prompt.as_deref().unwrap_or("");
        let title = params.worktree_title.as_deref().unwrap_or("Kanban Task");
        let name = params.worktree_name.clone();
        let base_ref = params.worktree_base_ref.clone();
        let (tx_reply, rx_reply) = oneshot::channel();
        if let Err(error) = tx.try_send(WorktreeSpawnRequest {
            prompt: prompt.to_string(),
            title: title.to_string(),
            worktree_name: name,
            base_ref,
            grant: params.tool_grant.clone().unwrap_or_default(),
            allowed_tools,
            reply: tx_reply,
        }) {
            return ipc_error(
                "WorktreeSpawn",
                format!("Worktree request not admitted: {error}"),
            );
        }
        return match rx_reply.await {
            Ok(Ok(thread)) => InferenceOutcome::WorktreeThread { thread },
            Ok(Err(error)) => InferenceOutcome::Error { error },
            Err(_) => ipc_error(
                "Connection",
                "GPUI-side worktree_spawn task dropped reply channel",
            ),
        };
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
            return ipc_error(
                "InvalidArgument",
                "rerank requires rerank_query and at least one rerank_document",
            );
        }

        // Detect the provider from the model prefix. Only OpenRouter has a
        // rerank endpoint among the registered providers.
        let Some((_provider, clean_model)) = hkask_inference::rerank::detect_rerank_provider(model)
        else {
            return ipc_error(
                "InvalidArgument",
                format!(
                    "rerank model '{model}' is not rerank-eligible — use an 'OpenRouter/'-prefixed rerank model (e.g. OpenRouter/qwen/qwen3-reranker-8b)"
                ),
            );
        };

        // Read the API key from the keychain via the GPUI-side channel. One
        // key, one location: OpenRouter's key lives at its `api_url`
        // keychain slot — the same slot zed's `ApiKeyState`, MCP env
        // injection, and the settings UI read.
        let Some(openrouter_descriptor) =
            crate::inference_providers::provider_by_credential_key("openrouter")
        else {
            return ipc_error(
                "Internal",
                "rerank provider 'openrouter' has no INFERENCE_PROVIDERS entry — the descriptor table diverged",
            );
        };
        let credential_url = openrouter_descriptor.api_url;
        let (tx_reply, rx_reply) = oneshot::channel::<Result<String, String>>();
        if provider_credential_tx
            .send((credential_url.to_string(), tx_reply))
            .is_err()
        {
            return ipc_error(
                "Connection",
                "GPUI-side credential task dropped — channel closed (task cancelled or app shutting down)",
            );
        }
        let api_key = match rx_reply.await {
            Ok(Ok(key)) => key,
            Ok(Err(e)) => {
                return ipc_error(
                    "PermissionDenied",
                    format!(
                        "rerank requires {} (keychain slot {credential_url}): {e}. Set the API key via Settings → AI → LLM Providers.",
                        openrouter_descriptor.env_var
                    ),
                );
            }
            Err(e) => {
                return ipc_error("Connection", format!("credential channel failed: {e}"));
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
                return ipc_error("Internal", format!("rerank API failed: {e}"));
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
        | InferenceMethod::ToolDefinition
        | InferenceMethod::CreateWorktreeThread
        | InferenceMethod::Rerank => {
            tracing::error!(
                target: "reg.inference",
                method = ?request.method,
                "dispatch reached the unreachable arm — a new InferenceMethod variant \
                 likely lacks an early-return block"
            );
            return ipc_error(
                "NotImplemented",
                format!(
                    "inference method {:?} not implemented in dispatch",
                    request.method
                ),
            );
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

        fn get_tool_info<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
        ) -> ToolFuture<'a, Option<ToolInfo>> {
            Box::pin(async { None })
        }
    }

    struct SchemaToolPort;
    impl ToolPort for SchemaToolPort {
        fn invoke<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: serde_json::Value,
            _: hkask_types::WebID,
        ) -> ToolFuture<'a, Result<serde_json::Value, ToolPortError>> {
            Box::pin(async { panic!("metadata lookup must not invoke a tool") })
        }
        fn get_tool_info<'a>(
            &'a self,
            server: &'a str,
            name: &'a str,
        ) -> ToolFuture<'a, Option<ToolInfo>> {
            Box::pin(async move {
                (server == "fixture" && name == "required_argument").then(|| ToolInfo {
                    name: name.to_string(),
                    description: "Requires a query".into(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "$defs": {"LookupQuery": {"type": "string"}},
                        "properties": {"query": {"$ref": "#/$defs/LookupQuery"}},
                        "required": ["query"]
                    }),
                })
            })
        }
    }

    /// A definition read is subject to the same card and parent grants as invocation.
    #[tokio::test]
    async fn tool_definition_requires_both_grants_and_returns_real_schema() {
        let inference: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let tools: Arc<dyn ToolPort> = Arc::new(SchemaToolPort);
        let server = format!("schema-test-{}", uuid::Uuid::new_v4());
        let token = crate::delegation_grants::grant_for_server(
            &server,
            &["fixture/required_argument".into()],
        )
        .expect("grant");
        let request = |allowlist: Vec<String>, grant: Option<String>| InferenceRequest {
            id: 1,
            method: InferenceMethod::ToolDefinition,
            params: InferenceParams {
                tool_server: Some("fixture".into()),
                tool_name: Some("required_argument".into()),
                tool_allowlist: Some(allowlist),
                tool_grant: grant,
                ..Default::default()
            },
        };
        for (allowed, grant) in [
            (vec!["fixture/required_argument".into()], None),
            (vec![], Some(token.clone())),
        ] {
            let result = dispatch(
                &inference,
                None,
                Some(&tools),
                &make_list_models_tx(),
                None,
                &make_provider_credential_tx(),
                request(allowed, grant),
            )
            .await;
            assert!(matches!(result, InferenceOutcome::Error { error } if error.code == "Auth"));
        }
        let result = dispatch(
            &inference,
            None,
            Some(&tools),
            &make_list_models_tx(),
            None,
            &make_provider_credential_tx(),
            request(vec!["fixture/required_argument".into()], Some(token)),
        )
        .await;
        let InferenceOutcome::ToolDefinition { definition } = result else {
            panic!("authorized metadata lookup must return the definition");
        };
        assert_eq!(definition.function.name, "fixture/required_argument");
        assert_eq!(definition.function.description, "Requires a query");
        assert_eq!(definition.function.parameters["required"][0], "query");
        assert_eq!(
            definition.function.parameters["properties"]["query"]["type"],
            "string"
        );
        assert!(definition.function.parameters.get("$defs").is_none());
        crate::revoke_delegation_grant(&server);
    }

    struct RecordingToolPort(std::sync::atomic::AtomicUsize);
    impl ToolPort for RecordingToolPort {
        fn invoke<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
            _args: serde_json::Value,
            _agent: hkask_types::WebID,
        ) -> ToolFuture<'a, Result<serde_json::Value, ToolPortError>> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async { Ok(serde_json::json!({"ok":true})) })
        }
        fn get_tool_info<'a>(
            &'a self,
            _server: &'a str,
            _tool: &'a str,
        ) -> ToolFuture<'a, Option<ToolInfo>> {
            Box::pin(async { None })
        }
    }

    /// expect: "A child cannot enlarge its parent-held grant or invoke tools with inference-only access" [P1]
    #[tokio::test]
    async fn ipc_child_cannot_expand_parent_grant() {
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let recording = Arc::new(RecordingToolPort(std::sync::atomic::AtomicUsize::new(0)));
        let tools: Arc<dyn ToolPort> = recording.clone();
        let server = format!("grant-test-{}", uuid::Uuid::new_v4());
        let token = crate::delegation_grants::grant_for_server(&server, &["kanban/read".into()])
            .expect("grant");
        for grant in [None, Some(token.clone())] {
            let mut request =
                make_tool_invoke_request("kanban", "write", Some(vec!["kanban/write".into()]));
            request.params.tool_grant = grant;
            let outcome = dispatch(
                &port,
                None,
                Some(&tools),
                &make_list_models_tx(),
                None,
                &make_provider_credential_tx(),
                request,
            )
            .await;
            let InferenceOutcome::Error { error } = outcome else {
                panic!("unauthorized dispatch succeeded");
            };
            assert_eq!(error.code, "Auth");
        }
        assert_eq!(recording.0.load(std::sync::atomic::Ordering::SeqCst), 0);
        let mut request =
            make_tool_invoke_request("kanban", "read", Some(vec!["kanban/read".into()]));
        request.params.tool_grant = Some(token);
        assert!(matches!(
            dispatch(
                &port,
                None,
                Some(&tools),
                &make_list_models_tx(),
                None,
                &make_provider_credential_tx(),
                request
            )
            .await,
            InferenceOutcome::ToolResult { .. }
        ));
        assert_eq!(recording.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        crate::revoke_delegation_grant(&server);
    }

    /// expect: "Malformed IPC requests never log grant tokens or prompt bodies" [P1]
    #[test]
    fn malformed_request_logging_withholds_payload() {
        let source = include_str!("inference_ipc_server.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("implementation");
        let parse = source
            .split("let request: InferenceRequest =")
            .nth(1)
            .expect("request parser")
            .split("let id = request.id")
            .next()
            .expect("parse block");
        assert!(parse.contains("error_class = ?e.classify()"));
        assert!(!parse.contains("line = %line"));
        assert!(
            !parse.contains("error = %e"),
            "serde errors can echo invalid field values"
        );
    }

    struct PendingInferencePort {
        started: tokio::sync::Notify,
        capacity: Arc<tokio::sync::Semaphore>,
    }

    impl InferencePort for PendingInferencePort {
        fn generate(
            &self,
            _: &str,
            _: &LLMParameters,
            _: Option<&[ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            Box::pin(async move {
                let _permit = self.capacity.acquire().await.expect("capacity open");
                self.started.notify_one();
                std::future::pending().await
            })
        }
    }

    /// expect: "Disconnecting an IPC caller drops in-progress dispatch and releases its capacity" [P1]
    #[tokio::test]
    async fn ipc_disconnect_drops_pending_dispatch() {
        for pipeline in [false, true] {
            let (mut client, server) = tokio::net::UnixStream::pair().expect("Unix socket pair");
            let pending = Arc::new(PendingInferencePort {
                started: tokio::sync::Notify::new(),
                capacity: Arc::new(tokio::sync::Semaphore::new(1)),
            });
            let port: Arc<dyn InferencePort> = pending.clone();
            let (skill_tx, _skill_rx) = tokio::sync::mpsc::unbounded_channel();
            let backend_slot = Arc::new(IpcBackendSlot::new(
                IpcBackend {
                    inference: port,
                    embedding: None,
                    tools: None,
                },
                skill_tx,
            ));
            let handler = tokio::spawn(handle_connection(
                server,
                backend_slot,
                make_list_models_tx(),
                None,
                make_provider_credential_tx(),
            ));
            let request = InferenceRequest {
                id: 1,
                method: InferenceMethod::Generate,
                params: InferenceParams {
                    prompt: Some("hold".into()),
                    ..Default::default()
                },
            };
            let line = serde_json::to_string(&request).expect("request") + "\n";
            client.write_all(line.as_bytes()).await.expect("write");
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                pending.started.notified(),
            )
            .await
            .expect("dispatch started");
            assert_eq!(pending.capacity.available_permits(), 0);
            if pipeline {
                client.write_all(line.as_bytes()).await.expect("pipeline");
            } else {
                drop(client);
            }
            tokio::time::timeout(std::time::Duration::from_secs(2), handler)
                .await
                .expect("disconnect/pipeline cancels dispatch")
                .expect("handler");
            assert_eq!(pending.capacity.available_permits(), 1);
        }
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

    fn make_provider_credential_tx()
    -> Arc<tokio::sync::mpsc::UnboundedSender<BatchCredentialRequest>> {
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
        let provider_credential_tx = make_provider_credential_tx();

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
            &provider_credential_tx,
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

    /// Dispatch a request through the canned test ports — the scaffolding
    /// every dispatch test repeats: the canned inference port, fresh
    /// list-models and provider-credential channels, no embedding port, no
    /// worktree spawn port (the worktree tests use `dispatch_worktree`).
    /// `tool_port` toggles the tool-dispatch leg.
    async fn dispatch_canned(
        request: InferenceRequest,
        tool_port: Option<&Arc<dyn ToolPort>>,
    ) -> InferenceOutcome {
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let list_models_tx = make_list_models_tx();
        let provider_credential_tx = make_provider_credential_tx();
        dispatch(
            &port,
            None,
            tool_port,
            &list_models_tx,
            None,
            &provider_credential_tx,
            request,
        )
        .await
    }

    /// A granted CreateWorktreeThread request — the fixture both worktree
    /// early-return tests build: a fresh server name, a WORKTREE_SPAWN
    /// grant, an empty allowlist, fixed prompt and title. Returns the server
    /// name so the caller can revoke the grant when the test ends.
    fn granted_worktree_request() -> (String, InferenceRequest) {
        let server = format!("spawn-test-{}", uuid::Uuid::new_v4());
        let grant = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::WORKTREE_SPAWN.into()],
        )
        .expect("grant");
        let request = InferenceRequest {
            id: 1,
            method: InferenceMethod::CreateWorktreeThread,
            params: InferenceParams {
                tool_grant: Some(grant),
                tool_allowlist: Some(vec![]),
                worktree_prompt: Some("do a thing".to_string()),
                worktree_title: Some("Test Task".to_string()),
                ..Default::default()
            },
        };
        (server, request)
    }

    #[tokio::test]
    async fn dispatch_tool_invoke_rejects_missing_allowlist() {
        // A missing allowlist is a protocol violation — fail closed,
        // never an implicit grant-all.
        let tool_port: Arc<dyn ToolPort> = Arc::new(CannedToolPort);

        let request = make_tool_invoke_request("kanban", "kanban_task_create", None);

        let outcome = dispatch_canned(request, Some(&tool_port)).await;

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
        let tool_port: Arc<dyn ToolPort> = Arc::new(CannedToolPort);

        let request = make_tool_invoke_request("kanban", "kanban_task_create", Some(vec![]));

        let outcome = dispatch_canned(request, Some(&tool_port)).await;

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
        let tool_port: Arc<dyn ToolPort> = Arc::new(CannedToolPort);

        let mut request = make_tool_invoke_request(
            "kanban",
            "kanban_task_create",
            Some(vec!["kanban/kanban_task_create".to_string()]),
        );

        request.params.tool_grant = crate::delegation_grants::grant_for_server(
            "allow-test",
            &["kanban/kanban_task_create".into()],
        );

        let outcome = dispatch_canned(request, Some(&tool_port)).await;

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
        let request = make_tool_invoke_request(
            "kanban",
            "kanban_task_create",
            Some(vec!["kanban/kanban_task_create".to_string()]),
        );

        let outcome = dispatch_canned(request, None).await; // no tool port

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
            4,
        );
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let outcome = dispatch(
            &port,
            Some(&embedding),
            None,
            &make_list_models_tx(),
            None,
            &make_provider_credential_tx(),
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
        let request = InferenceRequest {
            id: 1,
            method: InferenceMethod::Embed,
            params: InferenceParams {
                embed_model: Some("test/model".to_string()),
                embed_texts: Some(vec!["hello".to_string()]),
                ..Default::default()
            },
        };

        let outcome = dispatch_canned(request, None).await; // no embedding port

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
        let request = InferenceRequest {
            id: 42,
            method: InferenceMethod::Generate,
            params: InferenceParams {
                prompt: Some("hello".to_string()),
                parameters: LLMParameters::default(),
                ..Default::default()
            },
        };

        let outcome = dispatch_canned(request, None).await;

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

        let outcome = dispatch_canned(request, None).await;

        match outcome {
            InferenceOutcome::Result { result } => {
                assert_eq!(result.text, "canned response");
            }
            other => panic!("expected result outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_generate_vision_returns_canned_result() {
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

        let outcome = dispatch_canned(request, None).await;

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
        let tool_port: Arc<dyn ToolPort> = Arc::new(CannedToolPort);

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

        let outcome = dispatch_canned(request, Some(&tool_port)).await;

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
        let tool_port: Arc<dyn ToolPort> = Arc::new(CannedToolPort);

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

        let outcome = dispatch_canned(request, Some(&tool_port)).await;

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
        let provider_credential_tx = make_provider_credential_tx();

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
            &provider_credential_tx,
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
        let (server, request) = granted_worktree_request();

        let outcome = dispatch_canned(request, None).await; // no worktree spawn port

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
        crate::delegation_grants::revoke_delegation_grant(&server);
    }

    #[tokio::test]
    async fn dispatch_create_worktree_thread_errors_when_channel_dropped() {
        // `CreateWorktreeThread` early-return: the spawn port is present but
        // the receiver was dropped (server shutting down). Dispatch must
        // return a Connection error — not reach the defensive arm.
        let (tx, rx) = tokio::sync::mpsc::channel::<WorktreeSpawnRequest>(WORKTREE_QUEUE_CAPACITY);
        drop(rx);
        let worktree_spawn_tx = Arc::new(tx);

        let (server, request) = granted_worktree_request();

        let outcome = dispatch_worktree(&worktree_spawn_tx, request).await;

        match outcome {
            InferenceOutcome::Error { error } => {
                assert_eq!(error.code, "WorktreeSpawn");
                assert!(
                    error.message.contains("not admitted"),
                    "expected worktree-spawn-task-dropped error, got: {}",
                    error.message
                );
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
        crate::delegation_grants::revoke_delegation_grant(&server);
    }

    fn worktree_request(grant: Option<String>, tools: Option<Vec<String>>) -> InferenceRequest {
        InferenceRequest {
            id: 1,
            method: InferenceMethod::CreateWorktreeThread,
            params: InferenceParams {
                tool_grant: grant,
                tool_allowlist: tools,
                worktree_prompt: Some("fixture".into()),
                ..Default::default()
            },
        }
    }

    async fn dispatch_worktree(
        tx: &Arc<tokio::sync::mpsc::Sender<WorktreeSpawnRequest>>,
        request: InferenceRequest,
    ) -> InferenceOutcome {
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        dispatch(
            &port,
            None,
            None,
            &make_list_models_tx(),
            Some(tx),
            &make_provider_credential_tx(),
            request,
        )
        .await
    }

    /// expect: [P1] missing, insufficient, revoked grants and absent narrowing enqueue no effects.
    #[tokio::test]
    async fn worktree_authority_denies_before_enqueue() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(WORKTREE_QUEUE_CAPACITY);
        let tx = Arc::new(tx);
        let server = format!("spawn-denied-{}", uuid::Uuid::new_v4());
        let insufficient =
            crate::delegation_grants::grant_for_server(&server, &["a/read".into()]).expect("grant");
        let revoked = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::WORKTREE_SPAWN.into()],
        )
        .expect("grant");
        crate::delegation_grants::revoke_delegation_grant(&server);
        let valid = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::WORKTREE_SPAWN.into()],
        )
        .expect("grant");
        for request in [
            worktree_request(None, Some(vec![])),
            worktree_request(Some(insufficient), Some(vec!["a/read".into()])),
            worktree_request(Some(revoked), Some(vec![])),
            worktree_request(Some(valid), None),
        ] {
            assert!(matches!(dispatch_worktree(&tx, request).await,
                InferenceOutcome::Error { error } if error.code == "Auth"));
            assert!(rx.try_recv().is_err());
        }
        crate::delegation_grants::revoke_delegation_grant(&server);
    }

    /// expect: [P1] the real queue consumer starts one valid spawn with only the intersection.
    #[tokio::test]
    async fn worktree_authority_is_intersected_and_revocable_at_dequeue() {
        for revoke in [false, true] {
            let server = format!("spawn-valid-{}", uuid::Uuid::new_v4());
            let grant = crate::delegation_grants::grant_for_server(
                &server,
                &[
                    crate::delegation_grants::WORKTREE_SPAWN.into(),
                    "a/read".into(),
                ],
            )
            .expect("grant");
            let (tx, mut rx) =
                tokio::sync::mpsc::channel::<WorktreeSpawnRequest>(WORKTREE_QUEUE_CAPACITY);
            let tx = Arc::new(tx);
            let send = tokio::spawn(async move {
                dispatch_worktree(
                    &tx,
                    worktree_request(Some(grant), Some(vec!["a/read".into(), "b/write".into()])),
                )
                .await
            });
            let request = rx.recv().await.expect("queued");
            if revoke {
                crate::delegation_grants::revoke_delegation_grant(&server);
            }
            let effects = std::sync::atomic::AtomicUsize::new(0);
            request
                .execute(|_, _, _, _, tools| {
                    effects.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    assert_eq!(tools, vec!["a/read"]);
                    std::future::ready(Ok(WorktreeThreadInfo {
                        message: "spawned".into(),
                    }))
                })
                .await;
            let outcome = send.await.expect("dispatch joined");
            assert_eq!(
                effects.load(std::sync::atomic::Ordering::SeqCst),
                usize::from(!revoke)
            );
            assert_eq!(
                matches!(outcome, InferenceOutcome::WorktreeThread { .. }),
                !revoke
            );
            crate::delegation_grants::revoke_delegation_grant(&server);
        }
    }

    /// expect: [P1] caller B disconnecting while A occupies the consumer never starts B.
    #[tokio::test]
    async fn worktree_disconnected_queued_request_never_starts() {
        let server_name = format!("spawn-cancel-{}", uuid::Uuid::new_v4());
        let grant = crate::delegation_grants::grant_for_server(
            &server_name,
            &[crate::delegation_grants::WORKTREE_SPAWN.into()],
        )
        .expect("grant");
        let (tx, mut rx) = tokio::sync::mpsc::channel::<WorktreeSpawnRequest>(1);
        let tx = Arc::new(tx);
        let (release, wait) = oneshot::channel::<()>();
        let a_tx = tx.clone();
        let a_grant = grant.clone();
        let a = tokio::spawn(async move {
            dispatch_worktree(&a_tx, worktree_request(Some(a_grant), Some(vec![]))).await
        });
        let first = rx.recv().await.expect("A queued");
        let (started, start) = oneshot::channel();
        let consumer = tokio::spawn(async move {
            first
                .execute(|_, _, _, _, _| async move {
                    started.send(()).expect("start receiver");
                    wait.await.expect("release A");
                    Ok(WorktreeThreadInfo {
                        message: "A".into(),
                    })
                })
                .await;
            rx.recv()
                .await
                .expect("B queued")
                .execute(|_, _, _, _, _| async {
                    panic!("cancelled B must never start");
                })
                .await;
        });
        start.await.expect("A started");
        let (mut client, socket) = tokio::net::UnixStream::pair().expect("socket pair");
        let port: Arc<dyn InferencePort> = Arc::new(CannedInferencePort);
        let (skill_tx, _skill_rx) = tokio::sync::mpsc::unbounded_channel();
        let backend_slot = Arc::new(IpcBackendSlot::new(
            IpcBackend {
                inference: port,
                embedding: None,
                tools: None,
            },
            skill_tx,
        ));
        let handler = tokio::spawn(handle_connection(
            socket,
            backend_slot,
            make_list_models_tx(),
            Some(tx.clone()),
            make_provider_credential_tx(),
        ));
        let bytes =
            serde_json::to_vec(&worktree_request(Some(grant), Some(vec![]))).expect("request JSON");
        client.write_all(&bytes).await.expect("request");
        client.write_all(b"\n").await.expect("newline");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while tx.capacity() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("B reached bounded queue");
        drop(client);
        handler.await.expect("disconnect handled");
        release.send(()).expect("release consumer");
        consumer.await.expect("consumer joined");
        assert!(matches!(
            a.await.expect("A joined"),
            InferenceOutcome::WorktreeThread { .. }
        ));
        crate::delegation_grants::revoke_delegation_grant(&server_name);
    }

    /// expect: [P1] a full queue rejects rather than allocating more pending work.
    #[tokio::test]
    async fn worktree_queue_rejects_over_capacity() {
        let server = format!("spawn-full-{}", uuid::Uuid::new_v4());
        let grant = crate::delegation_grants::grant_for_server(
            &server,
            &[crate::delegation_grants::WORKTREE_SPAWN.into()],
        )
        .expect("grant");
        let (tx, mut rx) = tokio::sync::mpsc::channel::<WorktreeSpawnRequest>(1);
        let tx = Arc::new(tx);
        let first_tx = tx.clone();
        let first_grant = grant.clone();
        let first = tokio::spawn(async move {
            dispatch_worktree(&first_tx, worktree_request(Some(first_grant), Some(vec![]))).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while tx.capacity() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("queue filled");
        assert!(
            matches!(dispatch_worktree(&tx, worktree_request(Some(grant), Some(vec![]))).await,
            InferenceOutcome::Error { error } if error.message.contains("not admitted"))
        );
        first.abort();
        assert!(first.await.expect_err("cancelled").is_cancelled());
        rx.recv()
            .await
            .expect("queued first")
            .execute(|_, _, _, _, _| async {
                panic!("cancelled request must not run");
            })
            .await;
        crate::delegation_grants::revoke_delegation_grant(&server);
    }

    // ── Socket path / directory tests ──────────────────────────────────

    #[test]
    fn generate_socket_path_is_stable_within_a_process() {
        let path_a = generate_socket_path().unwrap();
        let path_b = generate_socket_path().unwrap();
        assert_eq!(
            path_a, path_b,
            "the socket path must be stable for the process lifetime — a per-instance path \
             turns the no-op→real re-wire into an env change that restarts every MCP child (P7h)"
        );
        assert!(
            path_a
                .to_string_lossy()
                .contains(&format!("kask-inference-{}", std::process::id())),
            "the path is pid-keyed so concurrent editor processes never collide"
        );
    }

    // zed-kask: P7h — the model-resolved re-wire swaps the serving backend in
    // place; these pins hold the swap mechanics.

    #[test]
    fn backend_slot_swap_replaces_serving_ports_in_place() {
        let (skill_tx, _skill_rx) = tokio::sync::mpsc::unbounded_channel();
        let initial: Arc<dyn InferencePort> = Arc::new(crate::NoModelInferencePort);
        let slot = IpcBackendSlot::new(
            IpcBackend {
                inference: initial.clone(),
                embedding: None,
                tools: None,
            },
            skill_tx,
        );

        let before = slot.current();
        assert!(
            Arc::ptr_eq(&before.inference, &initial),
            "current() must serve the backend the slot was built with"
        );
        assert!(before.tools.is_none());

        let replacement: Arc<dyn InferencePort> = Arc::new(crate::NoModelInferencePort);
        slot.swap(replacement.clone(), None, Some(Arc::new(CannedToolPort)));

        let after = slot.current();
        assert!(
            Arc::ptr_eq(&after.inference, &replacement),
            "swap must replace the inference port in place"
        );
        assert!(
            after.tools.is_some(),
            "swap must install the tool port (wrapped in HostSkillToolPort against the slot's skill channel)"
        );
    }

    #[test]
    fn swap_inference_ipc_ports_without_a_server_returns_false_and_keeps_the_socket_path() {
        // No `InferenceIpcServer::start` runs in this test binary, so the
        // global slot is empty: the swap must report false (the caller falls
        // back to a fresh start) and must not touch the socket-path global —
        // the re-wire never changes `HKASK_INFERENCE_SOCKET` (P7h).
        crate::set_inference_socket_path("/tmp/kask-test-swap-socket-path.sock");
        let swapped = swap_inference_ipc_ports(Arc::new(crate::NoModelInferencePort), None, None);
        assert!(!swapped, "no server is running in this test process");
        assert_eq!(
            crate::get_inference_socket_path().as_deref(),
            Some("/tmp/kask-test-swap-socket-path.sock")
        );
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
    // labeled "Connection": `embed_error_outcome` (the dispatch Embed
    // arm's classifier) maps each `EmbeddingGenerationError` variant to a
    // distinct error code, and the `Api` status travels structurally so
    // the client reconstructs the exact variant instead of re-parsing the
    // message string.

    #[test]
    fn embed_error_outcome_classifies_variants_and_preserves_api_status() {
        let InferenceOutcome::Error { error } = embed_error_outcome(
            hkask_types::EmbeddingGenerationError::Json("test".to_string()),
        ) else {
            panic!("an embedding failure must map to the Error outcome");
        };
        assert_eq!(error.code, "Json");
        assert_eq!(error.message, "test");
        assert_eq!(error.status, None);

        let InferenceOutcome::Error { error } = embed_error_outcome(
            hkask_types::EmbeddingGenerationError::Api(429, "rate limited".to_string()),
        ) else {
            panic!("an embedding failure must map to the Error outcome");
        };
        assert_eq!(error.code, "Api");
        assert_eq!(error.message, "rate limited");
        assert_eq!(error.status, Some(429));
    }
}
