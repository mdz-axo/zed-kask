//! MCP runtime for hKask
//!
//! Manages MCP server connections, tool discovery, and lifecycle.
//! Servers are spawned as child processes via `start_server_with_env()`, which
//! performs the MCP handshake, discovers tools dynamically, and stores
//! live `Peer<RoleClient>` connections. `shutdown_all()` terminates
//! all managed processes.

use hkask_capability::ToolInfo;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{Peer, RoleClient, ServiceExt};
use rmcp::transport::TokioChildProcess;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;

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

#[derive(Clone)]
pub struct McpRuntime {
    /// Registered MCP servers (metadata)
    servers: Arc<RwLock<HashMap<String, McpServer>>>,
    /// Tool registry (tool_name -> server_id)
    tool_registry: Arc<RwLock<HashMap<String, String>>>,
    /// Live connections to MCP server processes, keyed by server ID
    connections: Arc<RwLock<HashMap<String, Peer<RoleClient>>>>,
    /// Cancellation tokens for managed server processes
    cancellation_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
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
    #[must_use = "result must be used"]
    pub async fn start_server_with_env(
        &self,
        server_id: &str,
        command: &str,
        extra_env: std::collections::HashMap<String, String>,
    ) -> Result<(), ServerStartError> {
        // Acquire write lock first to prevent TOCTOU races.
        let mut connections = self.connections.write().await;
        if connections.contains_key(server_id) {
            info!(
                target: "hkask.mcp",
                server_id = %server_id,
                "Server already connected"
            );
            return Ok(());
        }

        // Resolve the binary path: check HKASK_MCP_{ID}_BIN first, then fall back
        // to PATH-based resolution. The env var allows pointing at a specific build
        // (e.g., target/debug/hkask-mcp-codegraph) without polluting PATH.
        //
        // P12 authenticated-host-mandate: the binary path is not a secret — it's a
        // deployment-time configuration, not an ambient authority.
        let binary = resolve_mcp_binary(server_id, command);

        let mut cmd = Command::new(&binary);
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

        // Keep the RunningService alive in a background task.
        // When `cancel` fires, the service loop exits and the child
        // process is cleaned up by rmcp's DropGuard.
        let bg_cancel = cancel.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = running.waiting() => {}
                _ = bg_cancel.cancelled() => {}
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
        connections.insert(server_id.to_string(), peer);
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
    pub(crate) async fn get_peer(&self, server_id: &str) -> Option<Peer<RoleClient>> {
        self.connections.read().await.get(server_id).cloned()
    }

    /// Call a tool on a connected server directly via the peer.
    ///
    /// Private transport primitive used by the governed `ToolPort` path.
    #[must_use = "result must be used"]
    async fn call_tool(
        &self,
        server_id: &str,
        tool: &str,
        arguments: serde_json::Map<String, Value>,
    ) -> Result<rmcp::model::CallToolResult, rmcp::service::ServiceError> {
        let peer = self
            .get_peer(server_id)
            .await
            .ok_or_else(|| rmcp::service::ServiceError::TransportClosed)?;

        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        peer.call_tool(params).await
    }

    /// Shut down all managed server processes.
    pub async fn shutdown_all(&self) {
        let mut tokens = self.cancellation_tokens.write().await;
        for (_, cancel) in tokens.drain() {
            cancel.cancel();
        }
        drop(tokens);
        self.connections.write().await.clear();
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
    pub async fn stop_server(&self, server_id: &str) {
        if let Some(cancel) = self.cancellation_tokens.write().await.remove(server_id) {
            cancel.cancel();
        }
        self.connections.write().await.remove(server_id);
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
                taint: hkask_capability::tool_taint::ToolTaint::Pure,
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
    async fn call_tool_inner(
        &self,
        server: &str,
        tool: &str,
        args: Value,
    ) -> Result<Value, hkask_capability::ToolPortError> {
        if self.get_peer(server).await.is_some() {
            // `args` is owned and unused after this point — move the map out
            // instead of cloning (non-object args collapse to an empty map,
            // matching the prior `as_object().cloned().unwrap_or_default()`).
            let arguments = match args {
                Value::Object(map) => map,
                _ => serde_json::Map::new(),
            };
            let result = self
                .call_tool(server, tool, arguments)
                .await
                .map_err(|e| hkask_capability::ToolPortError::InvocationFailed(e.to_string()))?;
            if result.is_error.unwrap_or(false) {
                return Err(hkask_capability::ToolPortError::InvocationFailed(
                    extract_text_content(&result),
                ));
            }
            return Ok(parse_call_result(&result));
        }
        if !self.tool_exists(tool).await {
            return Err(hkask_capability::ToolPortError::NotFound(
                hkask_types::NotFound {
                    entity_type: "tool".to_string(),
                    id: format!("Tool '{}' not found in MCP runtime", tool),
                },
            ));
        }
        Err(hkask_capability::ToolPortError::InvocationFailed(format!(
            "Server '{}' registered but not connected — call start_server_with_env() first",
            server
        )))
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
                    description:
