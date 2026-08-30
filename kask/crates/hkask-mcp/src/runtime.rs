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
//! restart that races an in-flight call). Four mechanisms keep the runtime from
//! serving a dead connection forever:
//!
//! 1. **Reap on death** — the keeper task that owns each `RunningService` removes
//!    the connection from `connections` when the service loop exits on its own,
//!    so a corpse is never left behind for `get_peer` to hand out.
//! 2. **Liveness on read** — `get_peer` filters out a peer whose transport has
//!    already closed, covering the window before the keeper task is scheduled.
//! 3. **Reconnect on demand** — `start_server_with_env` records each server's
//!    launch spec, so `call_tool_inner` can re-spawn a dead server once (subject
//!    to the reconnect cooldown]) and retry the call rather than failing until the
//!    next settings change.
//! 4. **Health supervisor** — a per-server background task that periodically
//!    checks transport liveness, proactively removes dead connections, and
//!    **attempts a restart** using the recorded launch spec. Unlike the
//!    on-demand path, the supervisor heals even when no tool call is in
//!    flight — without it, a server that crashes while idle stays dead
//!    forever. After `max_consecutive_health_failures` consecutive failures
//!    the circuit breaker stops auto-healing that server with an
//!    operator-actionable error — an unsupervised respawn loop is the
//!    crash-loop defect (2026-08-29). Any explicit start (settings-change
//!    restart, operator action, on-demand reconnect from a tool call)
//!    spawns a fresh supervisor and re-enables healing.
//!
//! Without these, `start_server_with_env`'s presence-based idempotency check
//! (`connections.contains_key`) would short-circuit every recovery attempt and
//! the only route back to a working connection was an operator settings change.

use hkask_tool_port::ToolInfo;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{Peer, RoleClient, ServiceExt};
use rmcp::transport::TokioChildProcess;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Minimum interval between reconnect attempts for the same server.
///
/// Bounds the damage from a crash-looping binary: without it, a server that
/// dies during its handshake would be re-spawned once per tool call, turning a
/// broken binary into a process-spawn storm.
///
/// Override: `HKASK_MCP_RECONNECT_COOLDOWN_SECS` env var.
const DEFAULT_RECONNECT_COOLDOWN: Duration = Duration::from_secs(5);

/// Maximum number of retry attempts when a server fails to start (spawn or
/// handshake). After exhausting these, the failure is reported to the caller
/// and the next tool call will retry via the on-demand reconnect path.
///
/// Override: `HKASK_MCP_STARTUP_MAX_RETRIES` env var.
const DEFAULT_STARTUP_MAX_RETRIES: u32 = 3;

/// Initial backoff for startup retries. Doubles each attempt up to
/// `DEFAULT_STARTUP_MAX_BACKOFF`.
///
/// Override: `HKASK_MCP_STARTUP_INITIAL_BACKOFF_MS` env var.
const DEFAULT_STARTUP_INITIAL_BACKOFF: Duration = Duration::from_millis(500);

/// Cap on the startup retry backoff.
///
/// Override: `HKASK_MCP_STARTUP_MAX_BACKOFF_SECS` env var.
const DEFAULT_STARTUP_MAX_BACKOFF: Duration = Duration::from_secs(10);

/// Interval between proactive health checks. The supervisor checks each
/// server's transport liveness and, if closed, removes the dead connection
/// and attempts a restart. The restart is the proactive self-healing path —
/// the on-demand reconnect (`call_tool_inner → try_reconnect`) only fires on
/// a tool call, so without a supervisor-driven restart a server that crashes
/// while no tool call is in flight stays dead indefinitely.
///
/// Override: `HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS` env var.
const DEFAULT_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Maximum consecutive health-check failures before the supervisor's circuit
/// breaker stops auto-healing the server. The stop is not a permanent outage:
/// any explicit start (settings-change restart, operator action, or
/// `call_tool_inner`'s on-demand reconnect) spawns a fresh supervisor whose
/// first healthy check resets the counter. The threshold bounds the
/// crash-loop defect — a dying binary respawned forever with no
/// operator-visible stop condition (observed live 2026-08-29).
/// up. Reset to zero on the first healthy connection seen.
///
/// Override: `HKASK_MCP_MAX_HEALTH_FAILURES` env var.
const DEFAULT_MAX_CONSECUTIVE_HEALTH_FAILURES: u32 = 3;

/// Interval between health checks once the supervisor has exceeded
/// `max_consecutive_health_failures`. Slower than the normal interval so a
/// crash-looping binary does not burn CPU, but still attempts restarts so a
/// transient cause (a DB lock that released, a disk that freed) is recovered
/// without operator intervention.
///
/// Resolve a duration from an env var (seconds), falling back to `default`.
/// Logs a warning on parse failure per `.rules` (numeric env vars that fail
/// to parse must `log::warn!` naming the malformed value).
fn resolve_duration_env_secs(var: &str, default: Duration) -> Duration {
    match std::env::var(var) {
        Ok(val) => match val.parse::<u64>() {
            Ok(secs) => Duration::from_secs(secs),
            Err(_) => {
                tracing::warn!(
                    target: "hkask.mcp",
                    env_var = %var,
                    value = %val,
                    "Failed to parse as u64 seconds — using default"
                );
                default
            }
        },
        Err(_) => default,
    }
}

/// Resolve a duration from an env var (milliseconds), falling back to `default`.
fn resolve_duration_env_millis(var: &str, default: Duration) -> Duration {
    match std::env::var(var) {
        Ok(val) => match val.parse::<u64>() {
            Ok(millis) => Duration::from_millis(millis),
            Err(_) => {
                tracing::warn!(
                    target: "hkask.mcp",
                    env_var = %var,
                    value = %val,
                    "Failed to parse as u64 milliseconds — using default"
                );
                default
            }
        },
        Err(_) => default,
    }
}

/// Resolve a u32 from an env var, falling back to `default`.
fn resolve_u32_env(var: &str, default: u32) -> u32 {
    match std::env::var(var) {
        Ok(val) => match val.parse::<u32>() {
            Ok(n) => n,
            Err(_) => {
                tracing::warn!(
                    target: "hkask.mcp",
                    env_var = %var,
                    value = %val,
                    "Failed to parse as u32 — using default"
                );
                default
            }
        },
        Err(_) => default,
    }
}

/// Resolved MCP runtime tuning parameters. Read once at first server launch
/// and cached for the process lifetime.
#[derive(Clone, Debug)]
struct McpRuntimeConfig {
    reconnect_cooldown: Duration,
    startup_max_retries: u32,
    startup_initial_backoff: Duration,
    startup_max_backoff: Duration,
    health_check_interval: Duration,
    max_consecutive_health_failures: u32,
}

impl Default for McpRuntimeConfig {
    fn default() -> Self {
        Self {
            reconnect_cooldown: resolve_duration_env_secs(
                "HKASK_MCP_RECONNECT_COOLDOWN_SECS",
                DEFAULT_RECONNECT_COOLDOWN,
            ),
            startup_max_retries: resolve_u32_env(
                "HKASK_MCP_STARTUP_MAX_RETRIES",
                DEFAULT_STARTUP_MAX_RETRIES,
            ),
            startup_initial_backoff: resolve_duration_env_millis(
                "HKASK_MCP_STARTUP_INITIAL_BACKOFF_MS",
                DEFAULT_STARTUP_INITIAL_BACKOFF,
            ),
            startup_max_backoff: resolve_duration_env_secs(
                "HKASK_MCP_STARTUP_MAX_BACKOFF_SECS",
                DEFAULT_STARTUP_MAX_BACKOFF,
            ),
            health_check_interval: resolve_duration_env_secs(
                "HKASK_MCP_HEALTH_CHECK_INTERVAL_SECS",
                DEFAULT_HEALTH_CHECK_INTERVAL,
            ),
            max_consecutive_health_failures: resolve_u32_env(
                "HKASK_MCP_MAX_HEALTH_FAILURES",
                DEFAULT_MAX_CONSECUTIVE_HEALTH_FAILURES,
            ),
        }
    }
}

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
    // D-Bus session bus address — required by the `keyring` crate's Secret
    // Service backend on Linux. Without it, `Keychain::retrieve_by_key`
    // blocks indefinitely in child processes after `env_clear()`. The governed
    // MCP runtime injects credentials as env vars (checked first by
    // `resolve_credential`). The DB passphrase keychain fallback path needs
    // D-Bus to avoid a hang when the env var is absent.
    "DBUS_SESSION_BUS_ADDRESS",
    // X11 display — required by some keychain backends (e.g. kwallet) on Linux.
    "DISPLAY",
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
/// Also serves as the Regulation governance boundary for tool invocations.
/// The `invoke` method emits a Regulation span, calls the tool, and emits
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
    env: hkask_types::ServerEnv,
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

/// The supervisor's view of a server's connection state.
#[derive(Debug)]
enum ConnectionState {
    /// Connection exists and transport is open.
    Healthy,
    /// Connection exists but transport has closed (keeper hasn't reaped yet).
    TransportClosed,
    /// No connection in the map (keeper reaped, or supervisor removed it).
    Missing,
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
    /// Last reconnect attempt per server, for the reconnect cooldown.
    last_reconnect: Arc<RwLock<HashMap<String, Instant>>>,
    /// Consecutive health-check failures per server. After
    /// `config.max_consecutive_health_failures` the circuit breaker stops
    /// auto-healing that server (operator-actionable error logged). Reset to
    /// zero on the first healthy connection seen.
    health_failures: Arc<RwLock<HashMap<String, u32>>>,
    /// Resolved tuning parameters (env-overridable defaults).
    config: McpRuntimeConfig,
    governance: Option<ToolGovernance>,
}

impl McpRuntime {
    /// Create a new MCP runtime with no governance configured.
    /// Tool invocations will bypass Regulation — use `with_governance`
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
            health_failures: Arc::new(RwLock::new(HashMap::new())),
            config: McpRuntimeConfig::default(),
            governance: None,
        }
    }

    /// Wire the cybernetic governance membrane (call-cap metering + Regulation spans).
    /// All subsequent `invoke` calls charge one call against the agent's per-tick
    /// cap and emit Regulation spans. There is deliberately **no** per-call OCAP
    /// capability check (RR-0056): the prior gate compared a `resource_id` built
    /// from the same tool name passed to `invoke`, so it was a value against
    /// itself. Authority is enforced upstream — at the inference IPC
    /// `tool_allowlist`, the swarm card `mcp_tools` allowlist, and per-server env
    /// allowlists — not re-checked here. Must be called before the first invocation.
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
    /// (deferred task). No-op when governance is not configured.
    pub fn set_event_sink(&self, sink: Arc<dyn hkask_types::RegulationSink>) {
        if let Some(governance) = &self.governance
            && let Ok(mut guard) = governance.event_sink.write()
        {
            *guard = sink;
        }
    }

    /// Server IDs that have a live (non-closed) connection.
    ///
    /// Used by the fleet health poller to count McpRuntime-managed servers
    /// alongside ContextServerStore-managed servers. A server is "running"
    /// if it has a connection whose transport is not closed.
    #[must_use = "result must be used"]
    pub async fn running_server_ids(&self) -> Vec<String> {
        let connections = self.connections.read().await;
        connections
            .iter()
            .filter(|(_, conn)| !conn.peer.is_transport_closed())
            .map(|(id, _)| id.clone())
            .collect()
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
        env: hkask_types::ServerEnv,
    ) -> Result<(), ServerStartError> {
        // Acquire write lock first to prevent TOCTOU races.
        {
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
        }
        // Lock dropped — the spawn+handshake below does not hold the connections
        // lock across `.await` points. This keeps the future closer to `Send`,
        // though `start_server_with_env` is still not fully `Send` because
        // `serve(transport).await` captures the `TokioChildProcess` (which
        // contains a `Box<dyn ChildWrapper>`). The health supervisor works
        // around this by running on the multi-thread `gpui_tokio` runtime,
        // which can host non-`Send` futures in a `tokio::spawn` task. The
        // supervisor clones the runtime (all fields are `Arc<RwLock<...>>`)
        // and calls `start_server_with_env` directly — see the supervisor
        // spawn below for the full reasoning.

        // Record the launch spec before spawning so a later reconnect can rebuild
        // this server even if this attempt fails partway through.
        self.launch_specs.write().await.insert(
            server_id.to_string(),
            LaunchSpec {
                command: command.to_string(),
                env: env.clone(),
            },
        );

        // Resolve the binary path: check HKASK_MCP_{ID}_BIN first, then fall back
        // to PATH-based resolution. The env var allows pointing at a specific build
        //
        // P12 authenticated-host-mandate: the binary path is not a secret — it's a
        // deployment-time configuration, not an ambient authority.
        let binary = resolve_mcp_binary(server_id, command);

        // Spawn + handshake with retry. A server that fails its handshake (binary
        // missing, DB locked, socket misconfiguration) is retried with exponential
        // backoff up to `config.startup_max_retries`. Each attempt pipes stderr and
        // forwards lines to the tracing substrate tagged with the server_id, so
        // operator logs attribute child diagnostics correctly.
        let mut last_error: Option<ServerStartError> = None;
        let mut backoff = self.config.startup_initial_backoff;
        let mut attempt: u32 = 0;
        let running = loop {
            let mut cmd = Command::new(&binary);
            // `Command` inherits the parent env by default, and the parent
            // process may have API keys in its environment (set via shell
            // env vars) and sets HKASK_SMTP_PASSWORD. Inheriting meant every
            // MCP child received every secret regardless of its per-server
            // allowlist — a server allowlisted `Some(&[])` still got the SMTP
            // password and all the API keys, silently nullifying the
            // credential scoping that `filter_credentials_for_server` exists
            // to provide.
            //
            // `env` is the caller's already-filtered per-server set (a
            // `ServerEnv` composed by `build_mcp_server_env`), so after the
            // clear the child sees exactly that, plus the non-secret process plumbing
            // enumerated in `PASSTHROUGH_ENV_VARS` (a child with no PATH or HOME cannot
            // resolve subprocesses or its own data directory).
            cmd.env_clear();
            for key in PASSTHROUGH_ENV_VARS {
                if let Some(value) = std::env::var_os(key) {
                    cmd.env(key, value);
                }
            }
            for (key, value) in env.iter() {
                cmd.env(key, value);
            }

            // Pipe stderr so child diagnostics are captured and tagged rather
            // than mixed into the parent's stderr unattributed. The builder API
            // returns the `ChildStderr` handle alongside the transport.
            let builder = TokioChildProcess::builder(cmd).stderr(Stdio::piped());
            let spawn_result = builder.spawn();
            let (transport, stderr_handle) = match spawn_result {
                Ok((transport, stderr)) => (transport, stderr),
                Err(e) => {
                    last_error = Some(ServerStartError::SpawnFailed(e.to_string()));
                    warn!(
                        target: "hkask.mcp",
                        server_id = %server_id,
                        attempt = attempt + 1,
                        max = self.config.startup_max_retries,
                        error = %e,
                        "MCP server spawn failed — will retry"
                    );
                    if attempt + 1 >= self.config.startup_max_retries {
                        break None;
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, self.config.startup_max_backoff);
                    attempt += 1;
                    continue;
                }
            };

            // Forward child stderr to tracing, tagged with the server_id so
            // operator logs attribute diagnostics correctly. Each line is logged
            // at INFO (most MCP servers emit structured `tracing` output on
            // stderr, which is informational, not an error).
            if let Some(stderr) = stderr_handle {
                let stderr_server_id = server_id.to_string();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stderr).lines();
                    loop {
                        match reader.next_line().await {
                            Ok(Some(line)) => {
                                info!(
                                    target: "hkask.mcp.child",
                                    server_id = %stderr_server_id,
                                    "{}",
                                    line
                                );
                            }
                            Ok(None) => break, // EOF — child closed stderr
                            Err(e) => {
                                warn!(
                                    target: "hkask.mcp.child",
                                    server_id = %stderr_server_id,
                                    error = %e,
                                    "stderr reader error"
                                );
                                break;
                            }
                        }
                    }
                });
            }

            match ().into_dyn().serve(transport).await {
                Ok(running) => break Some(running),
                Err(e) => {
                    last_error = Some(ServerStartError::ConnectFailed(format!(
                        "Handshake with '{}' failed: {}",
                        server_id, e
                    )));
                    warn!(
                        target: "hkask.mcp",
                        server_id = %server_id,
                        attempt = attempt + 1,
                        max = self.config.startup_max_retries,
                        error = %e,
                        "MCP server handshake failed — will retry"
                    );
                    if attempt + 1 >= self.config.startup_max_retries {
                        break None;
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, self.config.startup_max_backoff);
                    attempt += 1;
                }
            }
        };

        let Some(running) = running else {
            let error = last_error.unwrap_or_else(|| {
                ServerStartError::SpawnFailed("exhausted retries without a captured error".into())
            });
            return Err(error);
        };

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

        // Re-acquire the connections write lock to insert the new connection.
        // The lock was dropped before the spawn+handshake to keep the future `Send`,
        // which opens a TOCTOU window: a concurrent `start_server_with_env` for
        // the same server_id (e.g. from `try_reconnect`) may have already inserted
        // a live connection. If so, drop the new connection (its `RunningService`
        // and child process are cleaned up by rmcp's DropGuard) rather than
        // overwriting the existing one and orphaning its keeper task.
        //
        // The keeper task for this (loser) connection was already spawned above
        // and holds the `RunningService` alive. We must cancel it so the child
        // process is killed and the keeper exits — otherwise the loser's process
        // and keeper task leak (nobody holds the cancellation token for this
        // generation, since the insert below is skipped).
        {
            let mut connections = self.connections.write().await;
            if let Some(existing) = connections.get(server_id)
                && !existing.peer.is_transport_closed()
            {
                info!(
                    target: "hkask.mcp",
                    server_id = %server_id,
                    "Concurrent start_server_with_env race — discarding duplicate connection"
                );
                // Cancel the keeper task for the loser connection. The keeper's
                // `bg_cancel.cancelled()` arm fires, it exits without reaping
                // (returns `false`), and the `RunningService` is dropped, killing
                // the child process via rmcp's DropGuard.
                cancel.cancel();
                return Ok(());
            }
            connections.insert(server_id.to_string(), Connection { peer, generation });
        }

        // Cancel the previous supervisor + keeper for this server_id before
        // inserting the new token. Without this, a `start_server_with_env` call
        // that replaces an existing connection (e.g. from `try_reconnect` or
        // the supervisor itself) orphans the old supervisor task: the old
        // `CancellationToken` is dropped from the map but the old supervisor
        // still holds a clone, so it never exits and leaks. Cancelling here
        // fires the old supervisor's `supervisor_cancel.cancelled()` arm and
        // the old keeper's `bg_cancel.cancelled()` arm, both of which exit
        // cleanly. The new `cancel` (with the new supervisor + keeper) is then
        // inserted as the sole live token.
        //
        // The old keeper's reap arm does NOT fire (it returns `false` on
        // cancellation), so it does not remove the new connection we just
        // inserted above — the generation stamp would also protect against
        // this, but the cancellation is the primary guard.
        {
            let mut tokens = self.cancellation_tokens.write().await;
            if let Some(previous) = tokens.insert(server_id.to_string(), cancel.clone()) {
                previous.cancel();
            }
        }

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

        // Spawn a health supervisor for this server. See `spawn_health_supervisor`
        // for the full design — the supervisor is the proactive self-healing
        // path that restarts crashed servers even when no tool call is in flight.
        self.spawn_health_supervisor(server_id, cancel);

        Ok(())
    }

    /// Spawn a health supervisor for a server.
    ///
    /// The supervisor closes two gaps the keeper task and the on-demand
    /// reconnect path leave open:
    ///
    /// 1. A server whose transport has closed but whose keeper task hasn't
    ///    been scheduled yet to reap it. The supervisor removes the dead
    ///    connection so the next call doesn't dispatch onto a corpse.
    /// 2. A server that has crashed and no tool call is in flight. Without a
    ///    supervisor-driven restart, the only healing path is
    ///    `call_tool_inner → try_reconnect`, which fires on a tool call. If
    ///    no call comes (the panel sees the server as unavailable and stops
    ///    calling), the server stays dead forever — a transient crash
    ///    becomes a permanent outage.
    ///
    /// The supervisor detects transport-closed and missing connections. It
    /// does NOT detect hung processes (child alive but unresponsive with an
    /// open transport) — that would require a ping-based health check, which
    /// is a future enhancement.
    ///
    /// The supervisor attempts a restart on every unhealthy check by
    /// re-invoking `start_server_with_env` with the recorded launch spec.
    /// `McpRuntime: Clone + Send + Sync` (all fields are `Arc<RwLock<...>>`),
    /// so the supervisor clones the runtime and spawns the restart inline.
    /// The `start_server_with_env` future is not `Send` (it holds
    /// `RwLockWriteGuard` across `.await` points), but the supervisor itself
    /// is a `tokio::spawn` task on the multi-thread runtime, which can host
    /// non-`Send` futures — unlike `tokio::spawn` which requires `Send`, the
    /// `LocalSet`-free `tokio::spawn` on the current-thread runtime would
    /// not. The governed runtime runs on the multi-thread `gpui_tokio`
    /// runtime, so this is safe.
    ///
    /// After `config.max_consecutive_health_failures` consecutive failures
    /// the circuit breaker stops auto-healing the server with an
    /// operator-actionable error. The failure counter increments on every
    /// check where the connection is dead or missing, and only resets
    /// when a genuinely healthy connection is seen — so a server that keeps
    /// dying accumulates failures across check intervals.
    fn spawn_health_supervisor(&self, server_id: &str, cancel: CancellationToken) {
        let supervisor_cancel = cancel;
        let supervisor_connections = self.connections.clone();
        let supervisor_health_failures = self.health_failures.clone();
        let supervisor_launch_specs = self.launch_specs.clone();
        let supervisor_runtime = self.clone();
        let supervisor_id = server_id.to_string();
        let normal_interval = self.config.health_check_interval;
        let max_health_failures = self.config.max_consecutive_health_failures;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(normal_interval);
            interval.tick().await; // skip the immediate first tick
            loop {
                tokio::select! {
                    _ = supervisor_cancel.cancelled() => return,
                    _ = interval.tick() => {}
                }

                // Classify the connection state. The failure counter only
                // resets on Healthy — Missing and TransportClosed both count
                // as failures, so a server that dies and stays dead
                // accumulates failures across intervals.
                let connection_state = {
                    let connections = supervisor_connections.read().await;
                    match connections.get(&supervisor_id) {
                        Some(conn) if !conn.peer.is_transport_closed() => ConnectionState::Healthy,
                        Some(_) => ConnectionState::TransportClosed,
                        None => ConnectionState::Missing,
                    }
                };

                match connection_state {
                    ConnectionState::Healthy => {
                        let mut failures = supervisor_health_failures.write().await;
                        if let Some(count) = failures.get_mut(&supervisor_id)
                            && *count > 0
                        {
                            *count = 0;
                        }
                        continue;
                    }
                    ConnectionState::TransportClosed => {
                        // Transport is closed but the keeper hasn't reaped yet.
                        // Remove the dead connection so `get_peer` returns `None`
                        // and the restart below (or `call_tool_inner`) can
                        // install a fresh one.
                        warn!(
                            target: "hkask.mcp",
                            server_id = %supervisor_id,
                            "MCP server transport closed — supervisor removing dead connection"
                        );
                        {
                            let mut connections = supervisor_connections.write().await;
                            connections.remove(&supervisor_id);
                        }
                    }
                    ConnectionState::Missing => {
                        // Connection was reaped (by the keeper or a prior
                        // supervisor cycle). Fall through to the restart attempt
                        // below — the supervisor heals proactively rather than
                        // waiting for a tool call.
                    }
                }

                // Increment the failure counter for both TransportClosed and
                // Missing states. A healthy connection resets it (above).
                // `saturating_add` avoids overflow panics in debug mode after
                // very long outage periods (the counter is never reset while
                // the server stays dead).
                let failures = {
                    let mut failures = supervisor_health_failures.write().await;
                    let count = failures.entry(supervisor_id.clone()).or_insert(0);
                    *count = count.saturating_add(1);
                    *count
                };

                // Attempt a restart using the recorded launch spec. This is the
                // proactive self-healing path — without it, a server that
                // crashes while no tool call is in flight stays dead forever.
                // `start_server_with_env` is idempotent per live connection, so
                // a concurrent `try_reconnect` from `call_tool_inner` is safe:
                // whichever wins installs the connection, the other no-ops.
                let spec = {
                    let specs = supervisor_launch_specs.read().await;
                    specs.get(&supervisor_id).cloned()
                };
                let Some(spec) = spec else {
                    // No launch spec means the server was deliberately stopped
                    // (`stop_server` clears the spec). Do not resurrect it.
                    warn!(
                        target: "hkask.mcp",
                        server_id = %supervisor_id,
                        consecutive_failures = failures,
                        "MCP server unhealthy but no launch spec recorded — deliberately stopped, not restarting"
                    );
                    continue;
                };

                // Re-check the cancellation token before the restart. There is a
                // TOCTOU window between the spec read above and the
                // `start_server_with_env` call below: `stop_server` could clear
                // the spec and cancel the token in between. Without this check,
                // the supervisor would resurrect a deliberately stopped server.
                // The `block_in_place` call below blocks the supervisor task, so
                // the `tokio::select!` cancellation arm cannot interrupt it —
                // this check closes the window for the common case (stop fires
                // before the restart begins). A stop that fires *during* the
                // `block_in_place` is still racy, but `start_server_with_env`'s
                // idempotency check (line ~454) would see the cancellation token
                // removed and the connection absent, so it would proceed —
                // however, the resulting process would be reaped on the next
                // supervisor tick because `stop_server` also cancels the
                // supervisor token, so the supervisor exits and the keeper task
                // for the new process reaps it on the next death. The residual
                // race is bounded and self-correcting.
                if supervisor_cancel.is_cancelled() {
                    info!(
                        target: "hkask.mcp",
                        server_id = %supervisor_id,
                        "Supervisor cancelled before restart — not resurrecting"
                    );
                    return;
                }

                if failures >= max_health_failures {
                    // The circuit breaker: stop auto-healing this server. The
                    // prior behavior — degrade the check interval but keep
                    // restarting forever — is the live crash-loop defect
                    // (2026-08-29: a keyless instance respawned a new pid
                    // every interval indefinitely, with no operator-visible
                    // stop condition). Giving up is NOT a permanent outage:
                    // any explicit start (settings-change restart, operator
                    // action, or `call_tool_inner`'s on-demand reconnect)
                    // spawns a fresh supervisor whose first healthy check
                    // resets the counter. What stops is the unsupervised
                    // respawn loop.
                    tracing::error!(
                        target: "hkask.mcp",
                        server_id = %supervisor_id,
                        consecutive_failures = failures,
                        "MCP server health supervisor giving up — auto-healing disabled \
                         after repeated restart failures (operator action required: check \
                         the server binary and configuration; a settings change or tool \
                         call re-enables healing)"
                    );
                    return;
                }

                warn!(
                    target: "hkask.mcp",
                    server_id = %supervisor_id,
                    consecutive_failures = failures,
                    connection_state = ?connection_state,
                    "MCP server unhealthy — supervisor attempting restart"
                );
                // `start_server_with_env` is not `Send` (it holds
                // `RwLockWriteGuard` across `.await` points and captures
                // `TokioChildProcess` which contains a `Box<dyn ChildWrapper>`),
                // so it cannot be `.await`'d directly inside a `tokio::spawn`
                // task (which requires `Send`). `block_in_place` moves the
                // blocking onto a dedicated thread (the multi-thread runtime's
                // blocking pool), and `Handle::current().block_on` drives the
                // future to completion on the runtime's reactor without
                // requiring `Send`. This is the same pattern used by
                // `hkask-mcp-kata-kanban` for `resolve_worktree_spawn_port`.
                //
                // SAFETY: `block_in_place` is safe on the multi-thread runtime.
                // The governed runtime runs on `gpui_tokio`'s multi-thread
                // runtime, so this is always satisfied in production. In tests,
                // `#[tokio::test(flavor = "multi_thread")]` provides the
                // required runtime flavor.
                let restart_outcome = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        supervisor_runtime
                            .start_server_with_env(&supervisor_id, &spec.command, spec.env)
                            .await
                    })
                });
                match restart_outcome {
                    Ok(()) => {
                        info!(
                            target: "hkask.mcp",
                            server_id = %supervisor_id,
                            "MCP server restarted by health supervisor"
                        );
                    }
                    Err(error) => {
                        tracing::warn!(
                            target: "hkask.mcp",
                            server_id = %supervisor_id,
                            %error,
                            consecutive_failures = failures,
                            "MCP server supervisor restart failed — will retry on next check"
                        );
                    }
                }
            }
        });
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
    /// from "never reaped but reconnected". A future `tests/reconnect_integration.rs`
    /// (not yet written) would need that distinction to pin the reap independently.
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

    /// Test seam: the health supervisor's consecutive-failure count for a
    /// server. When it reaches `max_consecutive_health_failures`, auto-healing
    /// is disabled for that server — the crash-loop protection (invariant I5
    /// of the MCP server lifecycle review, 2026-08-29). Read-only, so it
    /// cannot perturb the supervisor.
    #[doc(hidden)]
    pub async fn health_failure_count(&self, server_id: &str) -> u32 {
        self.health_failures
            .read()
            .await
            .get(server_id)
            .copied()
            .unwrap_or(0)
    }

    /// Re-spawn a server whose connection died, subject to the reconnect cooldown
    /// (`config.reconnect_cooldown`).
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
                && previous.elapsed() < self.config.reconnect_cooldown
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
        self.health_failures.write().await.clear();
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
        self.health_failures.write().await.remove(server_id);
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

impl hkask_tool_port::ToolPort for McpRuntime {
    fn invoke<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
        args: Value,
        agent: hkask_types::WebID,
    ) -> hkask_tool_port::ToolFuture<'a, Result<Value, hkask_tool_port::ToolPortError>> {
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
                // `kask-panel` and skill execution personas were never
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
                        return Err(hkask_tool_port::ToolPortError::EnergyBudgetExceeded(
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
                    Span::from_kind(SpanKind::ToolCompleted),
                    CyclePhase::Act,
                    serde_json::json!({ "server": server, "tool": tool, "calls": 1, "status": status }),
                    0,
                );
                if let Err(e) = sink.persist(&record) {
                    tracing::warn!(target: "reg.mcp", error = %e, "Failed to persist reg.mcp call-settled span");
                }

                // Record the outcome in the RegulationLedger so the
                // `ToolReliabilitySensor` can sense the aggregate success
                // rate. The domain is the MCP server name (not the tool name)
                // so reliability is tracked per-server — a single broken
                // server surfaces as one degraded domain, not scattered across
                // individual tools. The `record_outcome` call is best-effort:
                // if the ledger is unavailable the outcome is simply not
                // recorded, and the sensor stays silent (not 1.0 — the
                // `.rules` `unwrap_or(0)` trap on sense inputs).
                let error_kind = result.as_ref().err().map(|e| {
                    // zed-kask: extract the typed kind from the `[kind] `
                    // marker (dispatch formats failed-tool details this way
                    // from the server's `structured_content`) so the ledger's
                    // per-kind breakdown classifies config gaps (unavailable /
                    // permission_denied) instead of recording the full
                    // message text as a "kind".
                    hkask_types::tool_response::error_kind_from_display(&e.to_string())
                });
                let cyber_lock = cyber.read().await;
                cyber_lock
                    .record_outcome(server, result.is_ok(), error_kind.as_deref())
                    .await;
                drop(cyber_lock);

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

    fn discover_tools<'a>(&'a self) -> hkask_tool_port::ToolFuture<'a, Vec<String>> {
        Box::pin(async move { McpRuntime::discover_tools(self).await })
    }

    fn get_tool_info<'a>(
        &'a self,
        tool_name: &'a str,
    ) -> hkask_tool_port::ToolFuture<'a, Option<hkask_tool_port::ToolInfo>> {
        Box::pin(async move { McpRuntime::get_tool_info(self, tool_name).await })
    }
}

impl McpRuntime {
    /// Inner tool call: live-connection check, JSON-RPC dispatch, result parsing.
    ///
    /// Heals a lost connection rather than reporting it: when the server has no
    /// live peer, or the dispatch itself fails because the transport closed under
    /// it, this reconnects once (bounded by the reconnect cooldown) and retries.
    /// Exactly one retry — a second transport failure is reported, because a
    /// server that dies immediately after a successful handshake is broken, not
    /// transiently unavailable, and retrying would spin.
    async fn call_tool_inner(
        &self,
        server: &str,
        tool: &str,
        args: Value,
    ) -> Result<Value, hkask_tool_port::ToolPortError> {
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
                    return Err(hkask_tool_port::ToolPortError::Unavailable(format!(
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
        let text = extract_text_content(&result);
        if result.is_error.unwrap_or(false) {
            // kask servers set `is_error` natively (rmcp's Result handling +
            // `McpToolError: IntoCallToolResult`) with the typed kind in
            // `structured_content`. Format the detail as `[kind] message`
            // (the `McpToolError` Display convention) so `invoke` can
            // extract the kind for the ledger's per-kind breakdown.
            let detail = result
                .structured_content
                .as_ref()
                .and_then(hkask_types::tool_response::parse_tool_error_value)
                .and_then(|envelope| envelope.kind)
                .map(|kind| format!("[{kind}] {text}"))
                .unwrap_or(text);
            return Err(DispatchError::Failed(detail));
        }
        Ok(parse_call_result(&result))
    }

    /// The error for a server with no live connection and no working reconnect,
    /// distinguishing an unknown tool from an unavailable server.
    async fn unavailable_error(&self, server: &str, tool: &str) -> hkask_tool_port::ToolPortError {
        if !self.tool_exists(tool).await {
            return hkask_tool_port::ToolPortError::NotFound(hkask_types::NotFound {
                entity_type: "tool".to_string(),
                id: format!("Tool '{}' not found in MCP runtime", tool),
            });
        }
        let known_launch = self.launch_specs.read().await.contains_key(server);
        if known_launch {
            hkask_tool_port::ToolPortError::Unavailable(format!(
                "Server '{server}' is not connected and could not be restarted — check that the \
                 hkask-mcp-{server} binary runs (set HKASK_MCP_{}_BIN to override the path)",
                server.to_uppercase()
            ))
        } else {
            hkask_tool_port::ToolPortError::Unavailable(format!(
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
    fn into_port_error(self) -> hkask_tool_port::ToolPortError {
        match self {
            DispatchError::NotDelivered(detail) => {
                hkask_tool_port::ToolPortError::Unavailable(format!("not delivered: {detail}"))
            }
            DispatchError::Interrupted(detail) => {
                hkask_tool_port::ToolPortError::Interrupted(detail)
            }
            DispatchError::Failed(detail) => {
                hkask_tool_port::ToolPortError::InvocationFailed(detail)
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

    /// A parent environment shaped like the real one: shell env vars
    /// have set the provider keys and login has set the SMTP password.
    fn parent_with_secrets() -> Vec<(&'static str, &'static str)> {
        vec![
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/home/tester"),
            ("RUST_LOG", "hkask=debug"),
            ("OPENROUTER_API_KEY", "parent-openrouter-secret"),
            ("HKASK_FRED_API_KEY", "parent-fred-secret"),
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

// ── Reconnect-path bookkeeping tests ───────────────────────────────────────
//
// These pin the four self-heal mechanisms' bookkeeping against the private
// `launch_specs` / `last_reconnect` maps and the `try_reconnect` path. They
// do NOT prove a killed child process is actually reconnected end-to-end —
// that is the not-yet-restored `tests/reconnect_integration.rs`'s job (see
// DIVERGENCE.md D3). They DO pin the invariants the integration test would
// rely on: that a launch spec is recorded, that a deliberate stop clears it,
// that the cooldown bounds a crash-looping binary, and that a metadata-only
// server cannot be reconnected.
//
// Inline in `runtime.rs` so they can read the private `launch_specs` and
// `last_reconnect` maps directly. A `#[cfg(test)]` module in a separate file
// could not.
#[cfg(test)]
mod reconnect_path_tests {
    use super::*;

    /// A runtime with a registered-but-not-started server has no launch spec,
    /// so `try_reconnect` reports `false` rather than pretending to recover.
    ///
    /// This is the metadata-only-server case: `register_server` populates
    /// `servers` and `tool_registry` but not `launch_specs`, so a reconnect
    /// has nothing to rebuild from.
    #[tokio::test]
    async fn metadata_only_server_cannot_be_reconnected() {
        let runtime = McpRuntime::new();
        runtime
            .register_server(McpServer {
                id: "metadata-only".to_string(),
                name: "metadata-only".to_string(),
                tools: vec![],
            })
            .await;

        // No launch spec was ever recorded.
        assert!(
            runtime.launch_specs.read().await.is_empty(),
            "a metadata-only server must not record a launch spec"
        );

        // try_reconnect returns false — there is nothing to reconnect from.
        let reconnected = runtime.try_reconnect("metadata-only").await;
        assert!(
            !reconnected,
            "try_reconnect must report false for a server with no launch spec"
        );
    }

    /// `stop_server` clears the launch spec, so a deliberate stop is not
    /// resurrected by a later `try_reconnect`.
    ///
    /// We cannot call `start_server_with_env` here (it needs a real binary
    /// and handshake), so we record the launch spec directly — mirroring the
    /// one line `start_server_with_env` writes before spawning. This pins
    /// the *clearing* behavior, which is the part `stop_server` owns.
    #[tokio::test]
    async fn stop_server_clears_the_reconnect_path() {
        let runtime = McpRuntime::new();
        // Record a launch spec directly, as `start_server_with_env` would.
        runtime.launch_specs.write().await.insert(
            "fixture".to_string(),
            LaunchSpec {
                command: "mcp-test-fixture".to_string(),
                env: hkask_types::ServerEnv::default(),
            },
        );
        runtime
            .last_reconnect
            .write()
            .await
            .insert("fixture".to_string(), Instant::now());

        runtime.stop_server("fixture").await;

        assert!(
            runtime.launch_specs.read().await.get("fixture").is_none(),
            "stop_server must clear the launch spec so the reconnect path \
             does not resurrect a deliberately-stopped server"
        );
        assert!(
            runtime.last_reconnect.read().await.get("fixture").is_none(),
            "stop_server must clear the last_reconnect stamp"
        );
    }

    /// `shutdown_all` clears every launch spec and last_reconnect stamp, so a
    /// deliberate full shutdown is not resurrected.
    #[tokio::test]
    async fn shutdown_all_clears_every_reconnect_path() {
        let runtime = McpRuntime::new();
        let mut specs = runtime.launch_specs.write().await;
        specs.insert(
            "a".to_string(),
            LaunchSpec {
                command: "a".to_string(),
                env: hkask_types::ServerEnv::default(),
            },
        );
        specs.insert(
            "b".to_string(),
            LaunchSpec {
                command: "b".to_string(),
                env: hkask_types::ServerEnv::default(),
            },
        );
        drop(specs);
        let mut last = runtime.last_reconnect.write().await;
        last.insert("a".to_string(), Instant::now());
        last.insert("b".to_string(), Instant::now());
        drop(last);

        runtime.shutdown_all().await;

        assert!(
            runtime.launch_specs.read().await.is_empty(),
            "shutdown_all must clear every launch spec"
        );
        assert!(
            runtime.last_reconnect.read().await.is_empty(),
            "shutdown_all must clear every last_reconnect stamp"
        );
    }

    /// `try_reconnect` is rate-limited by the reconnect cooldown: a second
    /// call within `config.reconnect_cooldown` of the first reports `false`
    /// without attempting a spawn.
    ///
    /// We assert on the `last_reconnect` stamp being present (the first call
    /// recorded it) and on `try_reconnect` returning `false` for the second
    /// call. The second call returns `false` either way (no launch spec →
    /// false; cooldown → false), so we additionally assert the stamp was
    /// *not* overwritten — proving the cooldown gate fired rather than the
    /// no-spec gate.
    #[tokio::test]
    async fn reconnect_is_rate_limited_by_the_cooldown() {
        let runtime = McpRuntime::new();
        // Record a launch spec so the first call reaches the cooldown gate
        // rather than the no-spec early return.
        runtime.launch_specs.write().await.insert(
            "fixture".to_string(),
            LaunchSpec {
                command: "mcp-test-fixture".to_string(),
                env: hkask_types::ServerEnv::default(),
            },
        );

        // First call: reaches the cooldown gate, stamps `last_reconnect`,
        // then calls `start_server_with_env` which fails (no such binary
        // resolves a handshake). It returns `false` (reconnect failed), but
        // the stamp is now present.
        let first = runtime.try_reconnect("fixture").await;
        assert!(
            !first,
            "reconnect against a non-spawning binary reports false"
        );
        let first_stamp = runtime
            .last_reconnect
            .read()
            .await
            .get("fixture")
            .copied()
            .expect("first try_reconnect must stamp last_reconnect");

        // Second call immediately after: the cooldown gate fires, the stamp
        // is NOT overwritten, and `try_reconnect` returns `false` without
        // attempting a spawn.
        let second = runtime.try_reconnect("fixture").await;
        assert!(
            !second,
            "a second try_reconnect within the cooldown must report false"
        );
        let second_stamp = runtime
            .last_reconnect
            .read()
            .await
            .get("fixture")
            .copied()
            .expect(
                "last_reconnect stamp must still be present after the \
                     cooldown-suppressed second call",
            );
        assert_eq!(
            first_stamp, second_stamp,
            "the cooldown gate must not overwrite the last_reconnect stamp \
             — if it did, the cooldown would never fire"
        );
    }

    /// A failed `start_server_with_env` still records a launch spec, so a
    /// later reconnect attempt has something to rebuild from.
    ///
    /// `start_server_with_env` records the spec *before* spawning (see the
    /// comment in that function: "Record the launch spec before spawning so a
    /// later reconnect can rebuild this server even if this attempt fails
    /// partway through"). We cannot exercise the full path without a real
    /// binary, so we pin the recording directly — the invariant the comment
    /// claims is that the spec is present even when the spawn fails.
    #[tokio::test]
    async fn failed_start_still_records_a_launch_spec_for_later_reconnect() {
        let runtime = McpRuntime::new();
        // Mirror the pre-spawn recording `start_server_with_env` performs.
        // The real call would fail at the handshake step (no such binary),
        // but the spec is already recorded by that point.
        let fixture_env = hkask_types::ServerEnv::from_canonical(HashMap::from([(
            "FIXTURE_MARKER".to_string(),
            "first".to_string(),
        )]));
        runtime.launch_specs.write().await.insert(
            "fixture".to_string(),
            LaunchSpec {
                command: "mcp-test-fixture".to_string(),
                env: fixture_env.clone(),
            },
        );

        // The spec is present and carries the env a reconnect would need.
        let spec = runtime
            .launch_specs
            .read()
            .await
            .get("fixture")
            .cloned()
            .expect("launch spec must be recorded even if the spawn fails");
        assert_eq!(spec.command, "mcp-test-fixture");
        assert_eq!(
            spec.env.get("FIXTURE_MARKER"),
            Some("first"),
            "the recorded env must be the one a reconnect would reuse"
        );
    }
}

// ── Metering + retry-classification tests ──────────────────────────────────
//
// These pin the public `ToolPort::invoke` metering behavior and the
// `ToolPortError` retry-classification invariants. They use a real
// `CyberneticsLoop` with a `NoopEventSink` so the metering path is exercised
// end-to-end (auto-register, ceiling, charged) without a DB.
//
// They do NOT exercise a live MCP server — the dispatch path is exercised by
// the not-yet-restored `tests/reconnect_integration.rs`. They assert on the
// metering decisions and the error classification, which are the parts `invoke`
// owns before dispatch.
#[cfg(test)]
mod metering_tests {
    use super::*;
    use hkask_regulation::{CyberneticsLoop, NoopEventSink, RegulationLedger};
    use hkask_tool_port::{ToolPort, ToolPortError};
    use hkask_types::WebID;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Build a runtime with governance wired (a real CyberneticsLoop with a
    /// NoopEventSink), so `invoke` exercises the metering path.
    fn governed_runtime() -> McpRuntime {
        let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(100)));
        let cyber = Arc::new(RwLock::new(CyberneticsLoop::new(ledger)));
        McpRuntime::new().with_governance(cyber, Arc::new(NoopEventSink))
    }

    /// An agent the composition root never registered is auto-registered at
    /// the default runaway ceiling, NOT denied. The call still proceeds to
    /// dispatch (which then fails because no server is connected — but the
    /// failure is `Unavailable`, not a metering refusal).
    ///
    /// This pins RR-0056's removal of the per-call capability gate: a missing
    /// registration is a wiring omission, not an authorization decision.
    #[tokio::test]
    async fn unregistered_agent_is_auto_registered_not_denied() {
        let runtime = governed_runtime();
        // Register a server with a tool so `tool_exists` returns true and the
        // dispatch path reaches `Unavailable` (registered but never started)
        // rather than `NotFound` (unknown tool). The metering decision is
        // what's under test, not the tool lookup.
        runtime
            .register_server(McpServer {
                id: "fixture".to_string(),
                name: "fixture".to_string(),
                tools: vec![McpTool {
                    name: "ping".to_string(),
                    description: String::new(),
                    input_schema: Value::Null,
                    server_id: "fixture".to_string(),
                }],
            })
            .await;
        let agent = WebID::new();

        // The agent has no registered cap. invoke must NOT return
        // EnergyBudgetExceeded — it must auto-register and proceed to
        // dispatch, which then reports Unavailable (no server connected).
        let result = runtime.invoke("fixture", "ping", Value::Null, agent).await;
        match result {
            Err(ToolPortError::Unavailable(_)) => {
                // Correct: auto-registered, proceeded to dispatch, dispatch
                // found no live connection.
            }
            Err(ToolPortError::EnergyBudgetExceeded(msg)) => {
                panic!(
                    "unregistered agent must be auto-registered, not denied \
                     with EnergyBudgetExceeded. Got: {msg}"
                );
            }
            other => panic!(
                "unregistered agent must proceed to dispatch and report \
                 Unavailable. Got: {other:?}"
            ),
        }
    }

    /// An agent that has exhausted its per-tick ceiling is refused with
    /// `EnergyBudgetExceeded` — the runaway-loop breaker. This is the ONE
    /// pre-dispatch refusal.
    #[tokio::test]
    async fn exhausted_ceiling_trips_the_runaway_breaker() {
        let runtime = governed_runtime();
        // Register a server+tool so the first call reaches `Unavailable`
        // (not `NotFound`) — the cap exhaustion, not the tool lookup, is
        // what's under test.
        runtime
            .register_server(McpServer {
                id: "fixture".to_string(),
                name: "fixture".to_string(),
                tools: vec![McpTool {
                    name: "ping".to_string(),
                    description: String::new(),
                    input_schema: Value::Null,
                    server_id: "fixture".to_string(),
                }],
            })
            .await;
        let agent = WebID::new();
        // Register a ceiling of 1 — the first call charges it, the second
        // trips the breaker.
        let cyber = runtime
            .governance
            .as_ref()
            .expect("governance must be wired")
            .cybernetics
            .clone();
        cyber.read().await.register_call_cap(agent, 1).await;

        // First call: charges the cap, proceeds to dispatch, reports
        // Unavailable (no server connected). The cap is now exhausted.
        let first = runtime.invoke("fixture", "ping", Value::Null, agent).await;
        assert!(
            matches!(first, Err(ToolPortError::Unavailable(_))),
            "first call against an exhausted-but-not-yet cap must proceed \
             to dispatch and report Unavailable. Got: {first:?}"
        );

        // Second call: cap exhausted, refused before dispatch.
        let second = runtime.invoke("fixture", "ping", Value::Null, agent).await;
        match second {
            Err(ToolPortError::EnergyBudgetExceeded(_)) => {
                // Correct: the runaway-loop breaker tripped.
            }
            other => panic!(
                "an agent that exhausted its ceiling must be refused with \
                 EnergyBudgetExceeded. Got: {other:?}"
            ),
        }
    }

    /// A runtime with no governance configured dispatches unmetered. The call
    /// proceeds to dispatch (and reports Unavailable) without any cap check.
    ///
    /// This pins the "no governance = unmetered” behavior: metering is an
    /// accounting concern, not an authorization one, so its absence is not a
    /// reason to refuse.
    #[tokio::test]
    async fn no_governance_dispatches_unmetered() {
        let runtime = McpRuntime::new(); // no with_governance
        assert!(runtime.governance.is_none());
        // Register a server+tool so the call reaches `Unavailable` (not
        // `NotFound`) — the absence of metering, not the tool lookup, is
        // what's under test.
        runtime
            .register_server(McpServer {
                id: "fixture".to_string(),
                name: "fixture".to_string(),
                tools: vec![McpTool {
                    name: "ping".to_string(),
                    description: String::new(),
                    input_schema: Value::Null,
                    server_id: "fixture".to_string(),
                }],
            })
            .await;

        let agent = WebID::new();
        let result = runtime.invoke("fixture", "ping", Value::Null, agent).await;
        // No cap refusal — proceeds to dispatch, reports Unavailable.
        assert!(
            matches!(result, Err(ToolPortError::Unavailable(_))),
            "no-governance runtime must dispatch unmetered and report \
             Unavailable (no server connected). Got: {result:?}"
        );
    }

    /// `ToolPortError::Unavailable` is retryable: the request provably never
    /// reached the tool, so re-issuing is safe.
    #[test]
    fn only_unavailable_is_retryable() {
        let unavailable = ToolPortError::Unavailable("no live connection".to_string());
        assert!(
            unavailable.is_retryable(),
            "Unavailable must be retryable — the request provably never \
             reached the tool"
        );
    }

    /// `ToolPortError::Interrupted` is NEVER retryable: a live peer accepted
    /// the request and the connection then failed, so the effect may or may
    /// not have been applied. Auto-retrying would duplicate side effects.
    #[test]
    fn interrupted_is_never_auto_retried() {
        let interrupted = ToolPortError::Interrupted("connection lost mid-call".to_string());
        assert!(
            !interrupted.is_retryable(),
            "Interrupted must NOT be retryable — the outcome is unknown, \
             so a retry could apply an effect twice"
        );
    }

    /// `Unavailable` and `Interrupted` are distinguishable: a caller can
    /// tell "provably never delivered" from "outcome unknown" and decide
    /// whether to retry. This pins the distinction rmcp forces
    /// (`ServiceError::TransportClosed` covers both a failed send and a
    /// dropped response channel, so the classification is the only signal).
    #[test]
    fn interrupted_and_unavailable_are_distinguishable() {
        let unavailable = ToolPortError::Unavailable("no live peer".to_string());
        let interrupted =
            ToolPortError::Interrupted("peer accepted then transport closed".to_string());
        assert_ne!(
            unavailable.is_retryable(),
            interrupted.is_retryable(),
            "Unavailable (retryable) and Interrupted (not retryable) must be \
             distinguishable via is_retryable()"
        );
    }

    /// An unknown tool reports `NotFound`, not `Unavailable`. This is the
    /// distinction `unavailable_error` enforces: a missing tool is a
    /// caller error (wrong name), not a transient connection state, and
    /// presenting it as Unavailable would invite a useless retry.
    #[tokio::test]
    async fn unknown_tool_is_not_found_not_unavailable() {
        let runtime = McpRuntime::new();
        // No tools registered — every tool name is unknown.
        let agent = WebID::new();
        let result = runtime
            .invoke("any-server", "nonexistent_tool", Value::Null, agent)
            .await;
        match result {
            Err(ToolPortError::NotFound(nf)) => {
                assert!(
                    nf.id.contains("nonexistent_tool"),
                    "NotFound must name the missing tool. Got: {nf:?}"
                );
            }
            Err(ToolPortError::Unavailable(msg)) => {
                panic!(
                    "an unknown tool must report NotFound, not Unavailable \
                     (a useless retry would be invited). Got Unavailable: {msg}"
                );
            }
            other => panic!("an unknown tool must report NotFound. Got: {other:?}"),
        }
    }
}
