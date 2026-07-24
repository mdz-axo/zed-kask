//! Start built-in MCP servers at REPL boot.
//!
//! Each server is spawned as a child process via `McpRuntime::start_server()`,
//! which performs the MCP handshake and discovers tools dynamically.
//! Servers that fail to start (binary not found, missing credentials) are
//! logged and skipped — their tools simply won't be available.

use hkask_mcp::runtime::McpRuntime;

/// Canonical (server_id, binary_name) registry — single source of truth.
///
/// Each entry maps `(server_id, binary_name)`. The binary must be on PATH
/// or specified via the `HKASK_MCP_{ID}_BIN` environment variable.
///
/// Servers are NOT auto-started at REPL boot — they require explicit user
/// consent via the `/mcp` command or the post-sign-on prompt (P2: Affirmative Consent).
///
/// Imported from `hkask_mcp_server::BUILTIN_SERVERS` — the canonical source.
pub use hkask_mcp_server::BUILTIN_SERVERS;

/// Start all built-in MCP servers and discover their tools.
///
/// Returns the number of servers that started successfully.
/// Servers that fail to start are logged and skipped.
pub async fn start_builtin_servers(runtime: &McpRuntime) -> usize {
    let mut started = 0;

    for (server_id, command) in BUILTIN_SERVERS {
        match runtime.start_server(server_id, command).await {
            Ok(()) => {
                started += 1;
            }
            Err(e) => {
                tracing::warn!(
                    target: "hkask.repl",
                    server_id = %server_id,
                    error = %e,
                    "Failed to start MCP server — tools will be unavailable"
                );
            }
        }
    }

    started
}

/// Start a single MCP server by its server_id.
///
/// Returns `true` if the server started successfully, `false` if it failed
/// (binary not found, missing credentials, handshake error).
pub async fn start_single_server(runtime: &McpRuntime, server_id: &str) -> bool {
    let command = match BUILTIN_SERVERS.iter().find(|(id, _)| *id == server_id) {
        Some((_, cmd)) => *cmd,
        None => {
            tracing::warn!(
                target: "hkask.repl",
                server_id = %server_id,
                "Unknown MCP server ID"
            );
            return false;
        }
    };

    match runtime.start_server(server_id, command).await {
        Ok(()) => {
            tracing::info!(
                target: "hkask.repl",
                server_id = %server_id,
                "MCP server started"
            );
            true
        }
        Err(e) => {
            tracing::warn!(
                target: "hkask.repl",
                server_id = %server_id,
                error = %e,
                "Failed to start MCP server"
            );
            false
        }
    }
}
