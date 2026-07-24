#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask MCP Server — MCP server utilities, daemon transport, and startup verification.
//!
//! Provides the lightweight layer that all hKask MCP servers depend on:
//! - Server scaffolding (McpToolError, ServerContext, CredentialRequirement, run_stdio_server)
//! - Daemon transport (DaemonClient, DaemonListener)
//! - Startup verification (P4 Gate 1/2/3)
//! - URL validation, identifier validation, HTTP helpers
//! - Macros: validate_field!, impl_tool_context!, mcp_server!

pub const BUILTIN_SERVERS: &[(&str, &str)] = &[
    ("memory", "hkask-mcp-memory"),
    ("condenser", "hkask-mcp-condenser"),
    ("research", "hkask-mcp-research"),
    ("companies", "hkask-mcp-companies"),
    ("communication", "hkask-mcp-communication"),
    ("curator", "hkask-mcp-curator"),
    ("media", "hkask-mcp-media"),
    ("docproc", "hkask-mcp-docproc"),
    ("training", "hkask-mcp-training"),
    ("replica", "hkask-mcp-replica"),
    ("kanban", "hkask-mcp-kata-kanban"),
    ("skill", "hkask-mcp-skill"),
    ("filesystem", "hkask-mcp-filesystem"),
    ("codegraph", "hkask-mcp-codegraph"),
    ("scenarios", "hkask-mcp-scenarios"),
    ("regulation", "hkask-mcp-regulation"),
];

pub mod daemon; // Unix socket transport for MCP binary ↔ hKask daemon

pub(crate) mod security;
pub mod server;
pub mod startup; // P4 Gate 1/2/3 startup verification for MCP server binaries

// ── Canonical MCP server registry ─────────────────────────────────────────
// Single source of truth for all (server_id, binary_name) mappings.
// Every consumer that starts MCP servers MUST use this list.
//
// Subsets are permitted only for intentionally-sandboxed environments
// (e.g., API server may exclude filesystem for security), but must
// reference this constant as the upper bound.

pub use daemon::{DaemonClient, DaemonHandler, DaemonListener, DaemonRequest, DaemonResponse};

pub use server::{
    CapabilityTier, CredentialRequirement, ExperienceCallback, McpError, ServerContext,
    ToolContext, api_get, api_put, execute_tool, load_dotenv, record_via_daemon,
    resolve_credential, run_stdio_server, run_stdio_server_with_preloaded, tool_internal_error,
    validate_identifier, validate_path, validate_tool_url, validate_tool_url_permissive,
};
pub use startup::{StartupGateResult, verify_startup_gates};

/// Run an MCP server with stdio transport.
///
/// This is the canonical entry point for all hKask MCP servers.
/// Each server's `main.rs` should call this directly.
#[must_use = "result must be used"]
pub async fn run_server<S, F>(
    name: &str,
    version: &str,
    factory: F,
    credentials: Vec<CredentialRequirement>,
) -> Result<(), McpError>
where
    S: rmcp::ServiceExt<rmcp::RoleServer>,
    S: rmcp::Service<rmcp::RoleServer>,
    F: FnOnce(ServerContext) -> Result<S, McpError>,
{
    run_stdio_server(name, version, factory, credentials).await
}

/// Run an MCP server with preloaded .env credentials.
#[must_use = "result must be used"]
pub async fn run_server_with_preloaded<S, F>(
    name: &str,
    version: &str,
    factory: F,
    credentials: Vec<CredentialRequirement>,
    preloaded: std::collections::HashMap<String, String>,
) -> Result<(), McpError>
where
    S: rmcp::ServiceExt<rmcp::RoleServer>,
    S: rmcp::Service<rmcp::RoleServer>,
    F: FnOnce(ServerContext) -> Result<S, McpError>,
{
    run_stdio_server_with_preloaded(name, version, factory, credentials, preloaded).await
}

/// Result of the standard MCP server daemon bootstrap flow.
///
/// All 16 MCP server binaries use this. The userpod identity
/// and optional daemon client are passed to the server's `run()`.
#[must_use = "bootstrap result must be passed to the server's run() function"]
pub struct MCPBootstrap {
    pub userpod: String,
    pub daemon_client: Option<DaemonClient>,
}

/// Standard MCP server bootstrap: .env → daemon verification → fallback.
///
/// Every hKask MCP server binary follows this pattern:
/// 1. Load `.env`
/// 2. Verify P4 startup gates (auth, role, tools) against the daemon
/// 3. If daemon is unavailable, warn and fall back to direct/standalone mode
///
/// After calling this, pass `userpod` and `daemon_client` to the
/// server's `run()` function.
///
/// # Arguments
/// - `server_name` — short name used in `verify_startup_gates` (e.g. "communication")
/// - `target` — tracing target for log messages (e.g. "hkask.mcp.communication")
/// - `host_env_var` — environment variable for the userpod identity
///   (defaults to `"HKASK_MCP_HOST"` for most servers)
///
/// expect: "Every MCP action has an authenticated host identity."
/// \[P12\] Motivating: every action has an authenticated author.
/// pre: `host_env_var` names a non-empty host identity environment variable.
/// post: returns an error before daemon verification when the host identity is absent.
/// \[P1\] Constraining: User Sovereignty — anonymous agency is never synthesized.
#[must_use = "MCPBootstrap must be passed to the server's run() function"]
pub async fn bootstrap_mcp_server(
    server_name: &str,
    target: &str,
    host_env_var: &str,
) -> Result<MCPBootstrap, McpError> {
    let userpod = std::env::var(host_env_var)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| McpError::MissingHostIdentity {
            env_var: host_env_var.to_string(),
        })?;

    let client = DaemonClient::new();
    let daemon_client = match verify_startup_gates(&client, &userpod, server_name, &[]).await {
        Ok(result) => {
            tracing::info!(
                target,
                userpod = %userpod,
                "P4 gates verified{}",
                if result.denied_tools.is_empty() {
                    String::new()
                } else {
                    format!(
                        " — {} tool(s) denied: {:?}",
                        result.denied_tools.len(),
                        result.denied_tools
                    )
                }
            );
            Some(DaemonClient::new())
        }
        Err(e) => {
            tracing::warn!(
                target,
                userpod = %userpod,
                error = %e,
                "Daemon unavailable — falling back to direct mode"
            );
            tracing::info!(
                target: "reg.mcp.health",
                status = "degraded",
                server = server_name,
                error = %e,
                "MCP server degraded — daemon unavailable"
            );
            None
        }
    };

    Ok(MCPBootstrap {
        userpod,
        daemon_client,
    })
}

/// Macro to validate an identifier field and return early on error.
///
/// Eliminates the repeated 3-line pattern:
/// ```ignore
/// if let Err(e) = validate_identifier("field", &value, 256) {
///     return span.error(e.kind, e.to_json_string());
/// }
/// ```
///
/// Usage:
/// ```ignore
/// validate_field!(span, "session_id", &session_id, 256);
/// ```
#[macro_export]
macro_rules! validate_field {
    ($span:expr, $name:expr, $value:expr, $max_len:expr) => {
        if let Err(e) = $crate::validate_identifier($name, $value, $max_len) {
            return $span.error(e.kind, e.to_json_string());
        }
    };
}

/// Generate a `ToolContext` impl for an MCP server struct.
///
/// Assumes the struct has `webid: WebID`, `userpod: String`,
/// and `daemon: Option<DaemonClient>` fields — the standard
/// pattern for all 14 hKask MCP servers.
///
/// Usage:
/// ```ignore
/// impl_tool_context!(CommunicationServer);
/// ```
#[macro_export]
macro_rules! impl_tool_context {
    ($type:ty) => {
        impl $crate::server::ToolContext for $type {
            fn webid(&self) -> &hkask_types::WebID {
                &self.webid
            }
            fn record_tool_outcome(&self, tool: &str, outcome: &str) {
                $crate::record_via_daemon(&self.daemon, &self.userpod, tool, outcome);
            }
        }
    };
}

/// Define an MCP server struct with standard fields + constructor.
///
/// Generates the struct with mandatory `webid`, `userpod`, `daemon`
/// fields plus any domain-specific fields, a `new()` constructor, and
/// a `ToolContext` impl via `impl_tool_context!`.
///
/// # Example
/// ```ignore
/// mcp_server!(struct SkillServer {
///     inference_port: Arc<dyn InferencePort>,
///     skills: HashMap<String, SkillDef>,
/// });
/// ```
///
/// Expands to a struct with `webid, userpod, daemon, inference_port, skills`.
#[macro_export]
macro_rules! mcp_server {
    // Variant with custom fields
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $ty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            /// Agent identity for capability tokens and ownership.
            pub webid: hkask_types::WebID,
            /// UserPod identity serving this MCP server.
            pub userpod: String,
            /// Daemon client for event recording.
            pub daemon: Option<$crate::DaemonClient>,
            $(
                $(#[$field_meta])*
                $field_vis $field : $ty
            ),*
        }

        impl $name {
            #[allow(clippy::too_many_arguments)]
            pub fn new(
                webid: hkask_types::WebID,
                userpod: String,
                daemon: Option<$crate::DaemonClient>,
                $($field : $ty),*
            ) -> Self {
                Self { webid, userpod, daemon, $($field),* }
            }
        }

        $crate::impl_tool_context!($name);
    };

    // Variant with no custom fields
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident;
    ) => {
        $(#[$meta])*
        $vis struct $name {
            /// Agent identity for capability tokens and ownership.
            pub webid: hkask_types::WebID,
            /// UserPod identity serving this MCP server.
            pub userpod: String,
            /// Daemon client for event recording.
            pub daemon: Option<$crate::DaemonClient>,
        }

        impl $name {
            pub fn new(
                webid: hkask_types::WebID,
                userpod: String,
                daemon: Option<$crate::DaemonClient>,
            ) -> Self {
                Self { webid, userpod, daemon }
            }
        }

        $crate::impl_tool_context!($name);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bootstrap_rejects_missing_host_identity() {
        let err = match bootstrap_mcp_server(
            "test-server",
            "hkask.mcp.test",
            "HKASK_TEST_MISSING_BOOTSTRAP_HOST",
        )
        .await
        {
            Ok(_) => panic!("missing host identity must prevent bootstrap"),
            Err(err) => err,
        };

        assert!(matches!(
            err,
            McpError::MissingHostIdentity { env_var }
                if env_var == "HKASK_TEST_MISSING_BOOTSTRAP_HOST"
        ));
    }
}
