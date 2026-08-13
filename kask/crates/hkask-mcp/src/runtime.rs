//! MCP runtime for hKask
//!
//! Manages MCP server connections, tool discovery, and lifecycle.
//! Servers are spawned as child processes via `start_server_with_env()`, which
//! performs the MCP handshake, discovers tools dynamically, and stores
//! live `Peer<RoleClient>` connections. `shutdown_all()` terminates
//! all managed processes.
//!
//! ## Connection healing
//!
//! A child process can die without anyone asking it to (crash, OOM, a parent
//! restart that races an in-flight call). Three mechanisms keep the runtime from
//! serving a dead connection forever:
//!
//! 1. **Reap on death** — the keeper task that owns each `RunningService` removes
//!    the connection from `connections` when the service loop exits on its own,
//!    so a corpse is never left behind for `get_peer` to hand out.
//! 2. **Liveness on read** — `get_peer` filters out a peer whose transport has
//!    already closed, covering the window before the keeper task is scheduled.
//! 3. **Reconnect on demand** — `start_server_with_env` records each server's
//!    launch spec, so `call_tool_inner` can re-spawn a dead server once (subject
//!    to [`RECONNECT_COOLDOWN`]) and retry the call rather than failing until the
//!    next settings change.
//!
//! Without these, `start_server_with_env`'s presence-based idempotency check
//! (`connections.contains_key`) would short-circuit every recovery attempt and
//! the only route back to a working connection was an operator settings change.

use hkask_capability::ToolInfo;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{Peer, RoleClient, ServiceExt};
use rmcp::transport::TokioChildProcess;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;

/// Minimum interval between reconnect attempts for the same server.
///
/// Bounds the damage from a crash-looping binary: without it, a server that
/// dies during its handshake would be re-spawned once per tool call, turning a
/// broken binary into a process-spawn storm.
const RECONNECT_COOLDOWN: Duration = Duration::from_secs(5);

/// A simple flat-cost energy estimator.
///
/// All tool invocations cost the same flat amount. This is the default
/// MCP tool definition
#[derive(Debug, Clone)]
pub struct McpTool {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Input schema (JSON Schema)
    pub input_schema: Value,
    /// MCP server that provides this tool
    pub server_id: String,
}

/// MCP server registration
#[derive(Debug, Clone)]
pub struct McpServer {
    /// Server ID
    pub id: String,
    /// Server name
    pub name: String,
    /// Tools provided by this server
    pub tools: Vec<McpTool>,
}

/// Non-secret process plumbing forwarded to every MCP child process after
/// `env_clear()` (RR-0060).
///
/// The child's environment is otherwise built solely from its own filtered
/// per-server allowlist, so nothing here may be a credential. Each entry is
/// justified:
///
/// - `PATH`, `HOME` — subprocess resolution and data-dir derivation. `HOME` is
///   read directly by the training server; several servers derive paths from it.
/// - `XDG_DATA_HOME`, `XDG_RUNTIME_DIR` — read by kask crates for data/socket
///   paths on Linux.
/// - `TMPDIR` — temp-file placement.
/// - `RUST_LOG`, `RUST_BACKTRACE` — diagnostics. `hkask-mcp-server`'s transport
///   builds an `EnvFilter` from the environment, so without `RUST_LOG` a child
///   silently loses operator-configured log levels.
/// - `LANG`, `LC_ALL`, `TZ` — locale and timezone correctness for date parsing
///   and formatting.
/// - `SSL_CERT_FILE`, `SSL_CERT_DIR` — TLS trust roots for servers making
///   outbound HTTPS calls; omitting these breaks certificate verification on
///   distributions that rely on them.
///
/// Deliberately absent: every `*_API_KEY`, `HF_TOKEN`, `HKASK_SMTP_PASSWORD`,
/// `HKASK_DB_PASSPHRASE`, and every other secret. A server that needs one of
/// those must declare it in its credential allowlist
/// (`kask_bridge::mcp_servers`), which is the point of the boundary.
pub(crate) const PASSTHROUGH_ENV_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "XDG_DATA_HOME",
    "XDG_RUNTIME_DIR",
    "TMPDIR",
    "RUST_LOG",
    "RUST_BACKTRACE",
    "LANG",
    "LC_ALL",
    "TZ",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// Error type for MCP server startup.
#[derive(Debug, Error)]
#[allow(clippy::enum_variant_names)]
pub enum ServerStartError {
    #[error("Failed to spawn MCP server process: {0}")]
    SpawnFailed(String),
    #[error("Failed to connect to MCP server (handshake): {0}")]
    ConnectFailed(String),
    #[error("Failed to discover tools from server: {0}")]
    DiscoveryFailed(String),
}

/// Resolve the binary path for an MCP server.
///
/// 1. Check `HKASK_MCP_{SERVER_ID_UPPER}_BIN` environment variable.
///    Example: `HKASK_MCP_FILESYSTEM_BIN` for server_id="filesystem".
/// 2. Fall back to the provided command name (PATH-based resolution).
///
/// This is the implementation of the contract documented in
/// `crates/hkask-cli/src/repl/builtin_servers.rs`.
fn resolve_mcp_binary(server_id: &str, command: &str) -> String {
    let env_var = format!("HKASK_MCP_{}_BIN", server_id.to_uppercase());
    if let Ok(explicit_path) = std::env::var(&env_var)
        && !explicit_path.is_empty()
    {
        tracing::info!(
            target: "hkask.mcp",
            server_id = %server_id,
            env_var = %env_var,
            binary = %explicit_path,
            "MCP binary resolved via env var"
        );
        return explicit_path;
    }
    command.to_string()
}

/// MCP runtime manager
///
/// Also serves as the OCAP/gas/Regulation governance boundary for tool invocations.
/// The `invoke` method verifies the delegation token, reserves gas via the
/// CyberneticsLoop, emits a Regulation span, calls the tool, settles gas, and emits
/// the outcome span. This collapses the former `GovernedTool` wrapper —
/// one tool, one path.
#[derive(Clone)]
struct ToolGovernance {
    cybernetics: Arc<RwLock<hkask_regulation::CyberneticsLoop>>,
    event_sink: Arc<std::sync::RwLock<Arc<dyn hkask_types::RegulationSink>>>,
}

/// How to (re-)spawn a server, recorded at launch so a dead connection can be
/// rebuilt without the composition root re-supplying the binary and env.
#[derive(Clone, Debug)]
struct LaunchSpec {
    command: String,
    env: HashMap<String, String>,
}

/// Monotonic generation counter for connection identity.
///
/// A keeper task must only reap the connection *it* installed. Without a
/// generation stamp, a keeper whose service loop exits just after a reconnect
/// replaced its entry would remove the healthy replacement — turning recovery
/// into a self-inflicted outage. The counter is process-global; only equality
/// matters.
static CONNECTION_GENERATION: AtomicU64 = AtomicU64::new(0);

/// A live connection plus the generation that installed it.
#[derive(Clone)]
struct Connection {
    peer: Peer<RoleClient>,
    generation: u64,
}

#[derive(Clone)]
pub struct McpRuntime {
    /// Registered MCP servers (metadata)
    servers: Arc<RwLock<HashMap<String, McpServer>>>,
    /// Tool registry (tool_name -> server_id)
    tool_registry: Arc<RwLock<HashMap<String, String>>>,
    /// Live connections to MCP server processes, keyed by server ID
    connections: Arc<RwLock<HashMap<String, Connection>>>,
    /// Cancellation tokens for managed server processes
    cancellation_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
    /// How each server was launched, so a dead connection can be rebuilt.
    launch_specs: Arc<RwLock<HashMap<String, LaunchSpec>>>,
    /// Last reconnect attempt per server, for [`RECONNECT_COOLDOWN`].
    last_reconnect: Arc<RwLock<HashMap<String, Instant>>>,
    governance: Option<ToolGovernance>,
}

impl McpRuntime {
    /// Create a new MCP runtime with no governance configured.
    /// Tool invocations will bypass OCAP/gas/Regulation — use `with_governance`
    /// to wire the cybernetic membrane.
    #[must_use]
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            tool_registry: Arc::new(RwLock::new(HashMap::new())),
            connections: Arc::new(RwLock::new(HashMap::new())),
            cancellation_tokens: Arc::new(RwLock::new(HashMap::new())),
            launch_specs: Arc::new(RwLock::new(HashMap::new())),
            last_reconnect: Arc::new(RwLock::new(HashMap::new())),
            governance: None,
        }
    }

    /// Wire the cybernetic governance membrane (OCAP + call-cap + Regulation spans).
    /// All subsequent `invoke` calls will verify the token, charge one call against
    /// the agent's per-tick cap, and emit spans. Must be called before the first
    /// invocation.
    #[must_use]
    pub fn with_governance(
        mut self,
        cybernetics: Arc<RwLock<hkask_regulation::CyberneticsLoop>>,
        event_sink: Arc<dyn hkask_types::RegulationSink>,
    ) -> Self {
        self.governance = Some(ToolGovernance {
            cybernetics,
            event_sink: Arc::new(std::sync::RwLock::new(event_sink)),
        });
        self
    }

    /// Replace the governance event sink after construction.
    ///
    /// Used by the composition root to upgrade from `NoopEventSink` to a
    /// durable `RegulationArchive` once the curator DB passphrase resolves
    /// (post-login deferred task). No-op when governance is not configured.
    pub fn set_event_sink(&self, sink: Arc<dyn hkask_types::RegulationSink>) {
        if let Some(governance) = &self.governance
            && let Ok(mut guard) = governance.event_sink.write()
        {
            *guard = sink;
        }
    }

    /// Register an MCP server (metadata only, no live connection).
    pub async fn register_server(&self, server: McpServer) {
        let mut servers = self.servers.write().await;
        let mut tool_registry = self.tool_registry.write().await;

        info!(
            target: "hkask.mcp",
            server_id = %server.id,
            server_name = %server.name,
            tools = server.tools.len(),
            "Registering MCP server"
        );

        // Register tools
        for tool in &server.tools {
            tool_registry.insert(tool.name.clone(), server.id.clone());
        }

        servers.insert(server.id.clone(), server);
    }

    /// Start an MCP server process and connect via rmcp stdio transport with
    /// extra environment variables for the child process.
    ///
    /// Idempotent per *live* connection: an already-connected server whose
    /// transport is still open is a no-op. A server whose transport has closed is
    /// reconnected — the check is liveness-based, not presence-based, because a
    /// presence-based check would short-circuit every recovery attempt against
    /// exactly the stale entry that needs replacing.
    #[must_use = "result must be used"]
    pub async fn start_server_with_env(
        &self,
        server_id: &str,
        command: &str,
        extra_env: std::collections::HashMap<String, String>,
    ) -> Result<(), ServerStartError> {
        // Acquire write lock first to prevent TOCTOU races.
        let mut connections = self.connections.write().await;
        match connections.get(server_id) {
            Some(existing) if !existing.peer.is_transport_closed() => {
                info!(
                    target: "hkask.mcp",
                    server_id = %server_id,
                    "Server already connected"
                );
                return Ok(());
            }
            Some(_) => {
                // A dead entry: drop it so the fresh connection below replaces
                // it even if the handshake takes a while.
                tracing::warn!(
                    target: "hkask.mcp",
                    server_id = %server_id,
                    "Replacing a closed connection"
                );
                connections.remove(server_id);
            }
            None => {}
        }

        // Record the launch spec before spawning so a later reconnect can rebuild
        // this server even if this attempt fails partway through.
        self.launch_specs.write().await.insert(
            server_id.to_string(),
            LaunchSpec {
                command: command.to_string(),
                env: extra_env.clone(),
            },
        );

        // Resolve the binary path: check HKASK_MCP_{ID}_BIN first, then fall back
        // to PATH-based resolution. The env var allows pointing at a specific build
        // (e.g., target/debug/hkask-mcp-codegraph) without polluting PATH.
        //
        // P12 authenticated-host-mandate: the binary path is not a secret — it's a
        // deployment-time configuration, not an ambient authority.
        let binary = resolve_mcp_binary(server_id, command);

        let mut cmd = Command::new(&binary);
        // Start from an empty environment, not an inherited one (RR-0060).
        //
        // `Command` inherits the parent env by default, and the parent loads every
        // provider API key into its own environment (`dotenvy::from_path` in
        // main.rs) and sets HKASK_SMTP_PASSWORD. Inheriting meant every MCP child
        // received every secret regardless of its per-server allowlist — a server
        // allowlisted `Some(&[])` still got the SMTP password and all the API keys,
        // silently nullifying the credential scoping that `filter_credentials_for_server`
        // exists to provide.
        //
        // `extra_env` is the caller's already-filtered per-server set, so after the
        // clear the child sees exactly that, plus the non-secret process plumbing
        // enumerated in `PASSTHROUGH_ENV_VARS` (a child with no PATH or HOME cannot
        // resolve subprocesses or its own data directory).
        cmd.env_clear();
        for key in PASSTHROUGH_ENV_VARS {
            if let Some(value) = std::env::var_os(key) {
                cmd.env(key, value);
            }
        }
        for (key, value) in &extra_env {
            cmd.env(key, value);
        }
        let transport = TokioChildProcess::new(cmd)
            .map_err(|e| ServerStartError::SpawnFailed(e.to_string()))?;

        let running = ().into_dyn().serve(transport).await.map_err(|e| {
            ServerStartError::ConnectFailed(format!("Handshake with '{}' failed: {}", server_id, e))
        })?;

        let peer = running.peer().clone();
        let cancel = CancellationToken::new();
        let generation = CONNECTION_GENERATION.fetch_add(1, Ordering::Relaxed);

        // Keep the RunningService alive in a background task.
        // When `cancel` fires, the service loop exits and the child
        // process is cleaned up by rmcp's DropGuard.
        //
        // When the service loop exits on its *own* (the child died, the transport
        // closed), reap the connection so `get_peer` stops handing out a corpse.
        // The generation stamp ensures a late-exiting keeper cannot remove a
        // healthy replacement installed by a reconnect.
        let bg_cancel = cancel.clone();
        let reap_connections = self.connections.clone();
        let reap_tokens = self.cancellation_tokens.clone();
        let reap_id = server_id.to_string();
        tokio::spawn(async move {
            let reaped = tokio::select! {
                quit = running.waiting() => {
                    match quit {
                        Ok(reason) => tracing::warn!(
                            target: "hkask.mcp",
                            server_id = %reap_id,
                            reason = ?reason,
                            "MCP server connection ended - reaping so the next call reconnects"
                        ),
                        Err(e) => tracing::warn!(
                            target: "hkask.mcp",
                            server_id = %reap_id,
                            error = %e,
                            "MCP server keeper task failed - reaping so the next call reconnects"
                        ),
                    }
                    true
                }
                _ = bg_cancel.cancelled() => {
                    // Deliberate stop (`stop_server` / `shutdown_all`), which
                    // already removed the entry. Nothing to reap.
                    false
                }
            };
            if !reaped {
                return;
            }
            let mut connections = reap_connections.write().await;
            if connections
                .get(&reap_id)
                .is_some_and(|current| current.generation == generation)
            {
                connections.remove(&reap_id);
                drop(connections);
                reap_tokens.write().await.remove(&reap_id);
            }
        });

        // Discover tools from the live server
        let tools = peer.list_all_tools().await.map_err(|e| {
            ServerStartError::DiscoveryFailed(format!(
                "list_all_tools from '{}' failed: {}",
                server_id, e
            ))
        })?;

        // Insert into the already-held write lock
        connections.insert(server_id.to_string(), Connection { peer, generation });
        // Drop the write lock before acquiring the cancellation_tokens lock
        drop(connections);

        self.cancellation_tokens
            .write()
            .await
            .insert(server_id.to_string(), cancel);

        // Register the server and its discovered tools
        let server = McpServer {
            id: server_id.to_string(),
            name: server_id.to_string(),
            tools: tools
                .into_iter()
                .map(|t| McpTool {
                    name: t.name.to_string(),
                    description: t.description.map(|d| d.to_string()).unwrap_or_default(),
                    input_schema: Value::Object((*t.input_schema).clone()),
                    server_id: server_id.to_string(),
                })
                .collect(),
        };

        info!(
            target: "hkask.mcp",
            server_id = %server_id,
            tools = server.tools.len(),
            "MCP server started and tools discovered"
        );

        self.register_server(server).await;

        Ok(())
    }

    /// Get a live Peer connection for a server (if connected).
    ///
    /// A peer whose transport has already closed is treated as absent. The keeper
    /// task reaps dead entries, but it is scheduled asynchronously — this check
    /// closes the window where a caller would otherwise dispatch onto a corpse and
    /// get `Transport closed` back.
    pub(crate) async fn get_peer(&self, server_id: &str) -> Option<Peer<RoleClient>> {
        let connection = self.connections.read().await.get(server_id).cloned()?;
        if connection.peer.is_transport_closed() {
            return None;
        }
        Some(connection.peer)
    }

    /// Whether a server currently has a live connection.
    ///
    /// Liveness-based, matching `get_peer`: a peer whose transport has closed
    /// reports `false` even before the keeper task reaps it.
    ///
    /// **Feature-gated to `test-fixture` deliberately.** Reap-on-death is only
    /// observable by reading connection state directly — every production path
    /// (`invoke`) *heals* on a missing peer, so it cannot distinguish "reaped"
    /// from "never reaped but reconnected". `tests/reconnect_integration.rs`
    /// needs that distinction to pin the reap independently.
    ///
    /// Not exposed unconditionally because nothing in production consumes it, and
    /// an always-present "for health checks" accessor with no health surface is
    /// the dead-advertised-invariant pattern this crate already deleted once
    /// (`list_servers`/`connection_count`/`connections`). Wire a real health
    /// consumer before promoting this to unconditional `pub` — see
    /// `tasks/kask-core-audit.md` §2a.
    #[cfg(feature = "test-fixture")]
    pub async fn is_connected(&self, server_id: &str) -> bool {
        self.get_peer(server_id).await.is_some()
    }

    /// Re-spawn a server whose connection died, subject to
    /// [`RECONNECT_COOLDOWN`].
    ///
    /// Returns `true` when a live connection exists afterwards. Requires a launch
    /// spec recorded by a prior `start_server_with_env` — a server that was only
    /// ever `register_server`'d (metadata, no process) cannot be reconnected, and
    /// reports `false` rather than pretending to recover.
    async fn try_reconnect(&self, server_id: &str) -> bool {
        let Some(spec) = self.launch_specs.read().await.get(server_id).cloned() else {
            return false;
        };

        // Cooldown check and stamp under one write lock so concurrent callers
        // cannot both pass the gate and spawn duplicate processes.
        {
            let mut last = self.last_reconnect.write().await;
            if let Some(previous) = last.get(server_id)
                && previous.elapsed() < RECONNECT_COOLDOWN
            {
                tracing::debug!(
                    target: "hkask.mcp",
                    server_id = %server_id,
                    "Reconnect suppressed by cooldown"
                );
                return false;
            }
            last.insert(server_id.to_string(), Instant::now());
        }

        info!(
            target: "hkask.mcp",
            server_id = %server_id,
            "Reconnecting to MCP server after transport loss"
        );
        match self
            .start_server_with_env(server_id, &spec.command, spec.env)
            .await
        {
            Ok(()) => {
                info!(
                    target: "hkask.mcp",
                    server_id = %server_id,
                    "MCP server reconnected"
                );
                true
            }
            Err(e) => {
                tracing::warn!(
                    target: "hkask.mcp",
                    server_id = %server_id,
                    error = %e,
                    "MCP server reconnect failed"
                );
                false
            }
        }
    }

    /// Shut down all managed server processes.
    ///
    /// Clears the launch specs too: a deliberate shutdown must not leave a
    /// reconnect path that would resurrect servers the caller just stopped.
    pub async fn shutdown_all(&self) {
        // Drop the connections before cancelling so a keeper task racing the
        // cancellation finds nothing of its own generation to reap.
        self.connections.write().await.clear();
        let mut tokens = self.cancellation_tokens.write().await;
        for (_, cancel) in tokens.drain() {
            cancel.cancel();
        }
        drop(tokens);
        self.launch_specs.write().await.clear();
        self.last_reconnect.write().await.clear();
    }

    /// Stop a single managed server process and drop its tool registry.
    ///
    /// Used by the settings-change restart path: governed `McpRuntime`
    /// instances are started once at login, so a settings change that alters
    /// a server's env (e.g. `kask.swarm.mode` → `HKASK_SWARM_MODE`) requires
    /// stopping the old process and re-running `start_server_with_env` with
    /// the new env (`start_server_with_env` alone is idempotent per
    /// connection and would no-op on an already-connected server).
    ///
    /// Idempotent: stopping an unknown or already-stopped server is a no-op.
    ///
    /// Drops the server's launch spec so the reconnect path does not resurrect a
    /// server that was deliberately stopped. The restart path re-records the spec
    /// when it calls `start_server_with_env` again.
    pub async fn stop_server(&self, server_id: &str) {
        // Remove the connection first: the keeper task's cancellation arm does not
        // reap, so removing here (before cancelling) keeps the two paths from
        // racing over the same entry.
        self.connections.write().await.remove(server_id);
        if let Some(cancel) = self.cancellation_tokens.write().await.remove(server_id) {
            cancel.cancel();
        }
        self.launch_specs.write().await.remove(server_id);
        self.last_reconnect.write().await.remove(server_id);
        // Drop the server's tools from the registry so stale names do not
        // resolve to a dead connection.
        let mut servers = self.servers.write().await;
        if let Some(server) = servers.remove(server_id) {
            let tools = server.tools;
            let tool_count = tools.len();
            let mut tool_registry = self.tool_registry.write().await;
            for tool in tools {
                tool_registry.remove(&tool.name);
            }
            info!(
                target: "hkask.mcp",
                server_id = %server_id,
                tools = tool_count,
                "MCP server stopped"
            );
        }
    }

    /// Discover tools from all registered servers
    #[must_use]
    pub async fn discover_tools(&self) -> Vec<String> {
        let tool_registry = self.tool_registry.read().await;
        tool_registry.keys().cloned().collect()
    }

    /// Get tool information with metadata
    #[must_use]
    pub async fn get_tool_info(&self, tool_name: &str) -> Option<ToolInfo> {
        let tool_registry = self.tool_registry.read().await;
        let server_id = tool_registry.get(tool_name)?;

        let servers = self.servers.read().await;
        let server = servers.get(server_id)?;

        server
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .map(|t| ToolInfo {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
                server_id: server_id.clone(),
            })
    }

    /// Check if a tool exists
    pub(crate) async fn tool_exists(&self, tool_name: &str) -> bool {
        let tool_registry = self.tool_registry.read().await;
        tool_registry.contains_key(tool_name)
    }
}

impl Default for McpRuntime {
    fn default() -> Self {
        Self::new()
    }
}

// ── ToolPort implementation ──────────────────────────────────────────────
//
// McpRuntime implements ToolPort directly. When governance is configured (via
// `with_governance`), `invoke` meters the call against the agent's runaway-loop
// ceiling, dispatches, and emits the outcome span. When governance is not
// configured it dispatches directly. One tool, one path — no wrapper layers.
//
// There is deliberately NO per-call capability check here. The prior gate
// compared a `DelegationToken`'s declared `(resource, resource_id, action)`
// against the invoked tool, but all three production mint sites built
// `resource_id` from the same tool name they passed to `invoke` — the
// comparison was a value against itself and could not deny. Authority lives at
// the boundaries that hold a list the caller cannot choose: the per-request
// `tool_allowlist` on the inference IPC dispatch, each swarm card's `mcp_tools`
// allowlist, and the per-server MCP env/credential allowlists.

impl hkask_capability::ToolPort for McpRuntime {
    fn invoke<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
        args: Value,
        agent: hkask_types::WebID,
    ) -> hkask_capability::ToolFuture<'a, Result<Value, hkask_capability::ToolPortError>> {
        Box::pin(async move {
            // Metering + span emit. Skipped when governance is not configured
            // (tests, lightweight embedders).
            if let Some(governance) = &self.governance {
                let cyber = &governance.cybernetics;
                // Clone the current sink out of the lock — the composition
                // root can swap it post-construction (deferred DB upgrade).
                let sink = governance
                    .event_sink
                    .read()
                    .map(|guard| guard.clone())
                    .unwrap_or_else(|_| std::sync::Arc::new(hkask_regulation::NoopEventSink));

                // Runaway-loop breaker. The ceiling exists to stop an agent that
                // has entered a non-terminating tool loop and to meter usage for
                // later optimization — it is not a permission check, so an agent
                // the composition root never registered is auto-registered at the
                // default ceiling rather than denied. Denying would fail the call
                // for a wiring omission (which is exactly what happened: the
                // `kask-panel` and `manifest-executor` personas were never
                // seeded, so every IPC and cascade tool call died here).
                let cyber_lock = cyber.read().await;
                match cyber_lock.charge_call_metered(&agent).await {
                    hkask_regulation::CallMeterOutcome::Charged => {}
                    hkask_regulation::CallMeterOutcome::AutoRegistered => {
                        tracing::info!(
                            target: "reg.mcp.cap",
                            agent = ?agent,
                            tool = %tool,
                            ceiling = hkask_regulation::DEFAULT_RUNAWAY_CALL_CEILING,
                            "no call ceiling registered for agent - auto-registered at the default runaway ceiling"
                        );
                    }
                    hkask_regulation::CallMeterOutcome::CeilingReached { ceiling } => {
                        // The only pre-dispatch refusal. A loop that has burned
                        // its whole per-tick ceiling is almost certainly not
                        // making progress; the cap resets next regulation tick.
                        tracing::warn!(
                            target: "reg.mcp.cap",
                            agent = ?agent,
                            tool = %tool,
                            ceiling,
                            "runaway-loop breaker tripped - agent exhausted its per-tick call ceiling"
                        );
                        return Err(hkask_capability::ToolPortError::EnergyBudgetExceeded(
                            format!(
                                "runaway-loop breaker: {agent:?} reached its per-tick ceiling of \
                                 {ceiling} calls (tool {tool}); resets next regulation tick"
                            ),
                        ));
                    }
                }
                drop(cyber_lock);

                // Call the tool.
                let result = self.call_tool_inner(server, tool, args).await;

                // Regulation: emit the call-settled span (best-effort, non-blocking).
                let status = if result.is_ok() { "success" } else { "failure" };
                use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanKind};
                let record = RegulationRecord::new(
                    agent,
                    Span::from_kind(SpanKind::GasSettled),
                    CyclePhase::Act,
                    serde_json::json!({ "server": server, "tool": tool, "calls": 1, "status": status }),
                    0,
                );
                if let Err(e) = sink.persist(&record) {
                    tracing::warn!(target: "reg.mcp", error = %e, "Failed to persist reg.mcp call-settled span");
                }

                result
            } else {
                // No governance configured: dispatch unmetered. Metering is an
                // accounting and loop-breaking concern, not an authorization
                // one, so its absence is not a reason to refuse the call — the
                // prior fail-closed refusal here made `McpRuntime::new()`
                // unusable for any embedder that did not also wire regulation.
                // Production wires `with_governance` (see main.rs).
                self.call_tool_inner(server, tool, args).await
            }
        })
    }

    fn discover_tools<'a>(&'a self) -> hkask_capability::ToolFuture<'a, Vec<String>> {
        Box::pin(async move { McpRuntime::discover_tools(self).await })
    }

    fn get_tool_info<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> hkask_capability::ToolFuture<'a, Option<hkask_capability::ToolInfo>> {
        Box::pin(async move { McpRuntime::get_tool_info(self, tool_name).await })
    }
}

impl McpRuntime {
    /// Inner tool call: live-connection check, JSON-RPC dispatch, result parsing.
    ///
    /// Heals a lost connection rather than reporting it: when the server has no
    /// live peer, or the dispatch itself fails because the transport closed under
    /// it, this reconnects once (bounded by [`RECONNECT_COOLDOWN`]) and retries.
    /// Exactly one retry — a second transport failure is reported, because a
    /// server that dies immediately after a successful handshake is broken, not
    /// transiently unavailable, and retrying would spin.
    async fn call_tool_inner(
        &self,
        server: &str,
        tool: &str,
        args: Value,
    ) -> Result<Value, hkask_capability::ToolPortError> {
        // `args` is owned and unused after this point — move the map out instead
        // of cloning (non-object args collapse to an empty map, matching the prior
        // `as_object().cloned().unwrap_or_default()`).
        let arguments = match args {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };

        if self.get_peer(server).await.is_none() {
            // No live peer. Reconnect before deciding this is a failure — the
            // connection may have died since the last call.
            if !self.try_reconnect(server).await {
                return Err(self.unavailable_error(server, tool).await);
            }
        }

        // Retry only a dispatch that never reached a live peer. Once a request
        // has been handed to one, a transport loss is `Interrupted` — see
        // `DispatchError::Interrupted` for why that must not be retried here.
        match self.dispatch(server, tool, arguments.clone()).await {
            Err(DispatchError::NotDelivered(detail)) => {
                tracing::warn!(
                    target: "hkask.mcp",
                    server_id = %server,
                    tool = %tool,
                    detail = %detail,
                    "Tool dispatch found no live transport - reconnecting and retrying once"
                );
                if !self.try_reconnect(server).await {
                    return Err(hkask_capability::ToolPortError::Unavailable(format!(
                        "Server '{server}' transport closed and could not be reconnected: {detail}"
                    )));
                }
                self.dispatch(server, tool, arguments)
                    .await
                    .map_err(DispatchError::into_port_error)
            }
            other => other.map_err(DispatchError::into_port_error),
        }
    }

    /// One dispatch attempt against the currently-registered peer.
    ///
    /// Classifies the failure by whether the request could have been *delivered*,
    /// which is what determines retry safety:
    ///
    /// - [`DispatchError::NotDelivered`] — there was no live peer to send to, so
    ///   the tool provably did not run.
    /// - [`DispatchError::Interrupted`] — a live peer accepted the call and the
    ///   connection then failed. `rmcp` reports both a failed send and a dropped
    ///   response channel as `ServiceError::TransportClosed`, so this cannot be
    ///   narrowed to non-delivery; the effect may have been applied.
    /// - [`DispatchError::Failed`] — the tool ran and failed.
    async fn dispatch(
        &self,
        server: &str,
        tool: &str,
        arguments: serde_json::Map<String, Value>,
    ) -> Result<Value, DispatchError> {
        let Some(peer) = self.get_peer(server).await else {
            return Err(DispatchError::NotDelivered(format!(
                "server '{server}' has no live connection"
            )));
        };
        if peer.is_transport_closed() {
            return Err(DispatchError::NotDelivered(format!(
                "server '{server}' transport closed before the request was sent"
            )));
        }

        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        let result = match peer.call_tool(params).await {
            Ok(result) => result,
            // The peer was live when we handed off, so we cannot distinguish
            // "the send was rejected" from "the server died after receiving it."
            // Report the outcome as unknown rather than assuming either.
            Err(
                error @ (rmcp::service::ServiceError::TransportClosed
                | rmcp::service::ServiceError::TransportSend(_)),
            ) => {
                return Err(DispatchError::Interrupted(error.to_string()));
            }
            Err(e) => return Err(DispatchError::Failed(e.to_string())),
        };
        if result.is_error.unwrap_or(false) {
            return Err(DispatchError::Failed(extract_text_content(&result)));
        }
        Ok(parse_call_result(&result))
    }

    /// The error for a server with no live connection and no working reconnect,
    /// distinguishing an unknown tool from an unavailable server.
    async fn unavailable_error(&self, server: &str, tool: &str) -> hkask_capability::ToolPortError {
        if !self.tool_exists(tool).await {
            return hkask_capability::ToolPortError::NotFound(hkask_types::NotFound {
                entity_type: "tool".to_string(),
                id: format!("Tool '{}' not found in MCP runtime", tool),
            });
        }
        let known_launch = self.launch_specs.read().await.contains_key(server);
        if known_launch {
            hkask_capability::ToolPortError::Unavailable(format!(
                "Server '{server}' is not connected and could not be restarted — check that the \
                 hkask-mcp-{server} binary runs (set HKASK_MCP_{}_BIN to override the path)",
                server.to_uppercase()
            ))
        } else {
            hkask_capability::ToolPortError::Unavailable(format!(
                "Server '{server}' registered but never started — call start_server_with_env() first"
            ))
        }
    }
}

/// Outcome of a single dispatch attempt, split by whether the request could
/// have been delivered — which is what determines retry safety.
enum DispatchError {
    /// No live peer accepted the request, so the tool provably did not run.
    /// Reconnecting and retrying is safe.
    NotDelivered(String),
    /// A live peer accepted the request and the connection then failed. The
    /// effect may or may not have been applied, so this must not be retried
    /// automatically.
    Interrupted(String),
    /// The call reached the server and failed there. A retry would only repeat it.
    Failed(String),
}

impl DispatchError {
    fn into_port_error(self) -> hkask_capability::ToolPortError {
        match self {
            DispatchError::NotDelivered(detail) => {
                hkask_capability::ToolPortError::Unavailable(format!("not delivered: {detail}"))
            }
            DispatchError::Interrupted(detail) => {
                hkask_capability::ToolPortError::Interrupted(detail)
            }
            DispatchError::Failed(detail) => {
                hkask_capability::ToolPortError::InvocationFailed(detail)
            }
        }
    }
}

/// Extract concatenated text from a CallToolResult's content items.
fn extract_text_content(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| match c {
            rmcp::model::ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse a CallToolResult into a JSON Value.
///
/// For a single text content item, tries to parse as JSON first
/// (structured tool responses often return JSON strings).
/// Falls back to a plain JSON string if parsing fails.
/// For multiple items, wraps them in a JSON array.
fn parse_call_result(result: &rmcp::model::CallToolResult) -> Value {
    if result.content.is_empty() {
        return Value::Null;
    }

    if result.content.len() == 1
        && let rmcp::model::ContentBlock::Text(text_content) = &result.content[0]
    {
        if let Ok(v) = serde_json::from_str::<Value>(&text_content.text) {
            return v;
        }
        return Value::String(text_content.text.clone());
    }

    let items: Vec<Value> = result
        .content
        .iter()
        .map(|c| match c {
            rmcp::model::ContentBlock::Text(t) => serde_json::from_str::<Value>(&t.text)
                .unwrap_or_else(|_| Value::String(t.text.clone())),
            rmcp::model::ContentBlock::Image(i) => serde_json::json!({
                "type": "image",
                "data": i.data,
                "mimeType": i.mime_type,
            }),
            _ => Value::Null,
        })
        .collect();
    Value::Array(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_capability::{ToolPort, ToolPortError};
    use hkask_regulation::{CyberneticsLoop, NoopEventSink, RegulationLedger};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn cybernetics() -> Arc<RwLock<CyberneticsLoop>> {
        let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(10)));
        Arc::new(RwLock::new(CyberneticsLoop::new(ledger)))
    }

    async fn register_test_tool(runtime: &McpRuntime, server_id: &str, tool_name: &str) {
        runtime
            .register_server(McpServer {
                id: server_id.to_string(),
                name: server_id.to_string(),
                tools: vec![McpTool {
                    name: tool_name.to_string(),
                    description: "test tool".to_string(),
                    input_schema: serde_json::json!({"type": "object"}),
                    server_id: server_id.to_string(),
                }],
            })
            .await;
    }

    /// RR-0057: an agent the composition root never seeded must be
    /// auto-registered and allowed through, not refused. Regression for the
    /// persona mismatch where `main.rs` seeded only `swarm-panel` while the IPC
    /// dispatch used `kask-panel` and the cascade used `manifest-executor`,
    /// so the old fail-closed cap denied every call on both paths.
    #[tokio::test]
    async fn unregistered_agent_is_auto_registered_not_denied() {
        let runtime = McpRuntime::new().with_governance(cybernetics(), Arc::new(NoopEventSink));
        register_test_tool(&runtime, "test-server", "test_tool").await;

        let unseeded = hkask_types::WebID::from_persona(b"never-registered-persona");
        let result = runtime
            .invoke("test-server", "test_tool", serde_json::json!({}), unseeded)
            .await;

        assert!(
            !matches!(result, Err(ToolPortError::EnergyBudgetExceeded(_))),
            "an unseeded agent must be auto-registered and allowed through, not refused: {result:?}"
        );
    }

    /// RR-0057: the one pre-dispatch refusal — a runaway loop that burns its
    /// whole per-tick ceiling.
    #[tokio::test]
    async fn exhausted_ceiling_trips_the_runaway_breaker() {
        let cyber = cybernetics();
        let agent = hkask_types::WebID::from_persona(b"ceiling-test-agent");
        cyber.read().await.register_call_cap(agent, 1).await;

        let runtime = McpRuntime::new().with_governance(cyber, Arc::new(NoopEventSink));
        register_test_tool(&runtime, "test-server", "test_tool").await;

        let first = runtime
            .invoke("test-server", "test_tool", serde_json::json!({}), agent)
            .await;
        assert!(
            !matches!(first, Err(ToolPortError::EnergyBudgetExceeded(_))),
            "the first call fits within a ceiling of 1: {first:?}"
        );

        let second = runtime
            .invoke("test-server", "test_tool", serde_json::json!({}), agent)
            .await;
        assert!(
            matches!(second, Err(ToolPortError::EnergyBudgetExceeded(_))),
            "exhausting the per-tick ceiling must trip the breaker: {second:?}"
        );
    }

    /// RR-0056: metering is accounting, not authorization — its absence must not
    /// refuse the call.
    #[tokio::test]
    async fn no_governance_dispatches_unmetered() {
        let runtime = McpRuntime::new();
        register_test_tool(&runtime, "test-server", "test_tool").await;

        let result = runtime
            .invoke(
                "test-server",
                "test_tool",
                serde_json::json!({}),
                hkask_types::WebID::from_persona(b"any-agent"),
            )
            .await;

        assert!(
            !matches!(result, Err(ToolPortError::EnergyBudgetExceeded(_))),
            "an unmetered runtime must dispatch rather than refuse: {result:?}"
        );
    }

    // ── Connection healing ─────────────────────────────────────────────
    //
    // These cover the paths that made a routine MCP server restart look like a
    // permanent panel outage: a dead peer left in `connections`, and a
    // presence-based idempotency check that refused to replace it.

    /// A server with no live connection reports `Unavailable`, not
    /// `InvocationFailed`.
    ///
    /// The distinction is what lets a panel retry: `Unavailable` means the call
    /// never reached the tool, so re-issuing it cannot double a side effect.
    #[tokio::test]
    async fn unreachable_server_reports_unavailable_not_failed() {
        let runtime = McpRuntime::new();
        register_test_tool(&runtime, "test-server", "test_tool").await;

        let result = runtime
            .invoke(
                "test-server",
                "test_tool",
                serde_json::json!({}),
                hkask_types::WebID::from_persona(b"any-agent"),
            )
            .await;

        match result {
            Err(ToolPortError::Unavailable(_)) => {}
            other => panic!(
                "a registered-but-unconnected server must report Unavailable so callers \
                 know a retry is safe, got: {other:?}"
            ),
        }
    }

    /// `Unavailable` is the only retryable classification.
    ///
    /// Pins the predicate panels branch on. If `InvocationFailed` ever became
    /// retryable, panels would re-issue state-changing tools whose failure was
    /// semantic.
    #[test]
    fn only_unavailable_is_retryable() {
        assert!(ToolPortError::Unavailable("transport closed".into()).is_retryable());
        assert!(!ToolPortError::InvocationFailed("tool said no".into()).is_retryable());
        assert!(!ToolPortError::EnergyBudgetExceeded("cap".into()).is_retryable());
        assert!(
            !ToolPortError::NotFound(hkask_types::NotFound {
                entity_type: "tool".into(),
                id: "nope".into(),
            })
            .is_retryable()
        );
    }

    /// A request that was delivered before the connection dropped must NOT be
    /// retryable.
    ///
    /// `rmcp` reports both a failed send and a dropped response channel as
    /// `ServiceError::TransportClosed` (`service.rs:921` vs `:555,566`), so once
    /// a request reaches a live peer, a transport loss is not proof of
    /// non-delivery. Auto-retrying would duplicate side effects — two
    /// `kanban_task_create`s, or a `swarm_hire` charging credits twice.
    #[test]
    fn interrupted_is_never_auto_retried() {
        let interrupted = ToolPortError::Interrupted("connection reset".into());
        assert!(
            !interrupted.is_retryable(),
            "an interrupted call has an unknown outcome; retrying it risks applying \
             a state-changing effect twice"
        );
    }

    /// The two transport classifications are distinct, and the unknown-outcome
    /// one says so in the message an operator sees.
    #[test]
    fn interrupted_and_unavailable_are_distinguishable() {
        let unavailable = ToolPortError::Unavailable("no live connection".into()).to_string();
        let interrupted = ToolPortError::Interrupted("connection reset".into()).to_string();
        assert_ne!(
            unavailable, interrupted,
            "an operator must be able to tell 'never ran' from 'outcome unknown'"
        );
        assert!(
            interrupted.contains("unknown"),
            "the interrupted message must state that the outcome is unknown, got: {interrupted}"
        );
    }

    /// An unknown tool is `NotFound`, not `Unavailable` — retrying cannot
    /// conjure a tool that was never registered.
    #[tokio::test]
    async fn unknown_tool_is_not_found_not_unavailable() {
        let runtime = McpRuntime::new();
        register_test_tool(&runtime, "test-server", "test_tool").await;

        let result = runtime
            .invoke(
                "test-server",
                "no_such_tool",
                serde_json::json!({}),
                hkask_types::WebID::from_persona(b"any-agent"),
            )
            .await;

        match result {
            Err(ToolPortError::NotFound(_)) => {}
            other => panic!("an unregistered tool name must report NotFound, got: {other:?}"),
        }
    }

    /// A server that cannot be spawned leaves a launch spec behind, so the
    /// reconnect path keeps trying on later calls rather than requiring an
    /// operator settings change.
    ///
    /// This is the regression for the sticky-dead-connection bug: recovery used to
    /// depend on `sync_kask_mcp_runtime_servers` firing, which only happens on a
    /// settings change.
    #[tokio::test]
    async fn failed_start_still_records_a_launch_spec_for_later_reconnect() {
        let runtime = McpRuntime::new();
        let outcome = runtime
            .start_server_with_env(
                "ghost",
                "hkask-mcp-definitely-not-a-real-binary",
                HashMap::new(),
            )
            .await;
        assert!(outcome.is_err(), "a nonexistent binary must fail to start");

        assert!(
            runtime.launch_specs.read().await.contains_key("ghost"),
            "the launch spec must survive a failed start so the reconnect path can retry; \
             without it, recovery requires an operator settings change"
        );
    }

    /// `stop_server` drops the launch spec, so the reconnect path does not
    /// resurrect a server that was deliberately stopped.
    #[tokio::test]
    async fn stop_server_clears_the_reconnect_path() {
        let runtime = McpRuntime::new();
        let _ = runtime
            .start_server_with_env("ghost", "hkask-mcp-not-real", HashMap::new())
            .await;
        assert!(runtime.launch_specs.read().await.contains_key("ghost"));

        runtime.stop_server("ghost").await;

        assert!(
            !runtime.launch_specs.read().await.contains_key("ghost"),
            "a deliberate stop must not leave a reconnect path that resurrects the server"
        );
    }

    /// `shutdown_all` clears every reconnect path for the same reason.
    #[tokio::test]
    async fn shutdown_all_clears_every_reconnect_path() {
        let runtime = McpRuntime::new();
        let _ = runtime
            .start_server_with_env("ghost-a", "hkask-mcp-not-real", HashMap::new())
            .await;
        let _ = runtime
            .start_server_with_env("ghost-b", "hkask-mcp-not-real", HashMap::new())
            .await;

        runtime.shutdown_all().await;

        assert!(
            runtime.launch_specs.read().await.is_empty(),
            "shutdown_all must not leave reconnect paths that resurrect stopped servers"
        );
    }

    /// The reconnect cooldown bounds spawn attempts against a crash-looping
    /// binary. Without it, every tool call would spawn another process.
    #[tokio::test]
    async fn reconnect_is_rate_limited_by_the_cooldown() {
        let runtime = McpRuntime::new();
        // Record a launch spec without a live connection, so `try_reconnect` has
        // something to attempt.
        let _ = runtime
            .start_server_with_env("ghost", "hkask-mcp-not-real", HashMap::new())
            .await;

        // The failed start above already stamped an attempt, so the immediate
        // next attempt must be suppressed.
        assert!(
            !runtime.try_reconnect("ghost").await,
            "a reconnect within the cooldown window must be suppressed so a broken \
             binary cannot become a process-spawn storm"
        );
    }

    /// A server that was only `register_server`'d (metadata, never a process)
    /// cannot be reconnected, and reports that rather than pretending to recover.
    #[tokio::test]
    async fn metadata_only_server_cannot_be_reconnected() {
        let runtime = McpRuntime::new();
        register_test_tool(&runtime, "metadata-only", "test_tool").await;

        assert!(
            !runtime.try_reconnect("metadata-only").await,
            "a server with no recorded launch spec has no process to restart"
        );
    }
}

/// RR-0060: the spawned-child environment boundary.
///
/// These tests assert on a REAL child process's environment rather than on
/// `filter_credentials_for_server`. That distinction is the whole point: the
/// three pre-existing "blast radius" tests in `kask_bridge::mcp_servers` all
/// exercised the filter function in isolation and passed for months while every
/// child inherited every secret, because nothing checked the actual boundary.
///
/// `start_server_with_env` needs a live MCP handshake, so these tests exercise
/// the same clear + passthrough + extra_env construction against
/// `/usr/bin/env`, which prints the environment it was given.
///
/// The simulated parent environment is passed in explicitly rather than written
/// with `std::env::set_var`: this crate is `#![forbid(unsafe_code)]` with no test
/// exemption, and mutating process env is `unsafe` in this edition. Passing it in
/// is also a better test — it cannot race another test in the same binary.
#[cfg(all(test, unix))]
mod env_isolation_tests {
    use super::PASSTHROUGH_ENV_VARS;

    /// Mirror of the environment construction in `start_server_with_env`, with
    /// the parent environment injected instead of read from the process. Kept
    /// adjacent to the real code so a divergence shows up as a failing test.
    // `tokio::process::Command` rather than `std::process::Command`: clippy.toml
    // bans the std spawn methods because they block the calling thread for an
    // unbounded time. The production path uses rmcp's TokioChildProcess for the
    // same reason.
    async fn child_env(parent: &[(&str, &str)], extra_env: &[(&str, &str)]) -> String {
        let mut cmd = tokio::process::Command::new("/usr/bin/env");
        cmd.env_clear();
        for key in PASSTHROUGH_ENV_VARS {
            if let Some((_, value)) = parent.iter().find(|(k, _)| k == key) {
                cmd.env(key, value);
            }
        }
        for (key, value) in extra_env {
            cmd.env(key, value);
        }
        let output = cmd
            .output()
            .await
            .expect("/usr/bin/env should be spawnable on unix");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// A parent environment shaped like the real one: `dotenvy::from_path` has
    /// loaded the provider keys (main.rs) and login has set the SMTP password.
    fn parent_with_secrets() -> Vec<(&'static str, &'static str)> {
        vec![
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/home/tester"),
            ("RUST_LOG", "hkask=debug"),
            ("DEEPINFRA_API_KEY", "parent-deepinfra-secret"),
            ("OPENROUTER_API_KEY", "parent-openrouter-secret"),
            ("ATLASCLOUD_API_KEY", "parent-atlascloud-secret"),
            ("HKASK_SMTP_PASSWORD", "parent-smtp-secret"),
            ("HKASK_DB_PASSPHRASE", "parent-db-secret"),
        ]
    }

    /// The regression: a secret present in the PARENT environment but absent
    /// from the server's allowlist must NOT reach the child.
    #[tokio::test]
    async fn spawned_child_env_excludes_non_allowlisted_secrets() {
        let parent = parent_with_secrets();
        // A server allowlisted `Some(&[])` / `Some(&[])` — e.g. `portfolio`.
        let env = child_env(&parent, &[]).await;

        for (key, value) in &parent {
            let looks_secret =
                key.contains("API_KEY") || key.contains("PASSWORD") || key.contains("PASSPHRASE");
            if looks_secret {
                assert!(
                    !env.contains(value),
                    "child inherited parent secret {key} despite an empty \
                     allowlist (RR-0060). Child env was:\n{env}"
                );
            }
        }
    }

    /// The complement: a credential the server IS allowlisted for must arrive.
    /// Without this, "no secrets leak" could be satisfied by leaking nothing at
    /// all, which would break every server that needs a real credential.
    #[tokio::test]
    async fn spawned_child_env_includes_allowlisted_credentials() {
        let env = child_env(
            &parent_with_secrets(),
            &[("HKASK_DB_PASSPHRASE", "filtered-in-passphrase")],
        )
        .await;
        assert!(
            env.contains("filtered-in-passphrase"),
            "an allowlisted credential must reach the child. Child env was:\n{env}"
        );
    }

    /// Clearing the environment must not strip the non-secret plumbing a child
    /// needs to function (subprocess resolution, data dirs, TLS roots, logging).
    #[tokio::test]
    async fn spawned_child_env_retains_non_secret_plumbing() {
        let env = child_env(&parent_with_secrets(), &[]).await;
        assert!(
            env.contains("hkask=debug"),
            "RUST_LOG must pass through so operator log levels survive; \
             hkask-mcp-server builds its EnvFilter from the environment. Got:\n{env}"
        );
        assert!(
            env.lines().any(|line| line.starts_with("PATH=")),
            "PATH must pass through for subprocess resolution. Got:\n{env}"
        );
        assert!(
            env.lines().any(|line| line.starts_with("HOME=")),
            "HOME must pass through — the training server reads it directly and \
             several servers derive data paths from it. Got:\n{env}"
        );
    }

    /// No entry in the passthrough list may be a credential. This is the guard
    /// against someone "fixing" a missing-credential bug by widening the
    /// passthrough list instead of the server's allowlist.
    #[test]
    fn passthrough_list_contains_no_credentials() {
        for key in PASSTHROUGH_ENV_VARS {
            let upper = key.to_uppercase();
            assert!(
                !(upper.contains("KEY")
                    || upper.contains("TOKEN")
                    || upper.contains("SECRET")
                    || upper.contains("PASSWORD")
                    || upper.contains("PASSPHRASE")),
                "{key} looks like a credential and must not be in \
                 PASSTHROUGH_ENV_VARS — add it to the server's credential \
                 allowlist in kask_bridge::mcp_servers instead (RR-0060)"
            );
        }
    }
}
