#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask MCP Server — MCP server utilities and startup verification.
//!
//! Provides the lightweight layer that all hKask MCP servers depend on:
//! - Server scaffolding (McpToolError, ServerContext, CredentialRequirement, run_stdio_server)
//! - Startup identity verification (P12 host identity)
//! - URL validation, identifier validation, HTTP helpers
//! - Macros: validate_field!, impl_tool_context!, mcp_server!

pub const BUILTIN_SERVERS: &[(&str, &str)] = &[
    ("condenser", "hkask-mcp-condenser"),
    ("research", "hkask-mcp-research"),
    ("companies", "hkask-mcp-companies"),
    ("curator", "hkask-mcp-curator"),
    ("media", "hkask-mcp-media"),
    ("corpus", "hkask-mcp-corpus"),
    ("training", "hkask-mcp-training"),
    ("kanban", "hkask-mcp-kata-kanban"),
    ("codegraph", "hkask-mcp-codegraph"),
    ("scenarios", "hkask-mcp-scenarios"),
];

pub(crate) mod security;
pub mod server;

// ── Canonical MCP server registry ─────────────────────────────────────────
// Single source of truth for all (server_id, binary_name) mappings.
// Every consumer that starts MCP servers MUST use this list.
//
// Subsets are permitted only for intentionally-sandboxed environments
// (e.g., API server may exclude filesystem for security), but must
// reference this constant as the upper bound.

pub use server::{
    CapabilityTier, CredentialRequirement, ExperienceCallback, McpError, ServerContext,
    ToolContext, api_get, api_put, execute_tool, load_dotenv, resolve_credential, run_stdio_server,
    run_stdio_server_with_preloaded, tool_internal_error, validate_identifier, validate_path,
    validate_tool_url, validate_tool_url_permissive,
};

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

/// Result of the standard MCP server bootstrap flow.
///
/// All MCP server binaries use this. The userpod identity is passed to the
/// server's `run()` function.
#[must_use = "bootstrap result must be passed to the server's run() function"]
pub struct MCPBootstrap {
    pub userpod: String,
}

/// Standard MCP server bootstrap: resolve host identity from env.
///
/// Every hKask MCP server binary follows this pattern:
/// 1. Load `.env`
/// 2. Resolve the userpod identity from the host env var
///
/// After calling this, pass `userpod` to the server's `run()` function.
///
/// # Arguments
/// - `server_name` — short name for logging (e.g. "corpus")
/// - `target` — tracing target for log messages (e.g. "hkask.mcp.corpus")
/// - `host_env_var` — environment variable for the userpod identity
///   (defaults to `"HKASK_MCP_HOST"` for most servers)
///
/// expect: "Every MCP action has an authenticated host identity."
/// \[P12\] Motivating: every action has an authenticated author.
/// pre: `host_env_var` names a non-empty host identity environment variable.
/// post: returns an error when the host identity is absent.
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

    tracing::info!(
        target,
        server = server_name,
        userpod = %userpod,
        "MCP server bootstrapped",
    );

    Ok(MCPBootstrap { userpod })
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
/// Assumes the struct has `webid: WebID` and `userpod: String` fields —
/// the standard pattern for all hKask MCP servers.
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
                tracing::debug!(
                    target: "reg.memory",
                    userpod = %self.userpod,
                    tool = %tool,
                    outcome = %outcome,
                    "Tool outcome recorded (no daemon — in-process only)",
                );
            }
        }
    };
}

/// Define an MCP server struct with standard fields + constructor.
///
/// Generates the struct with mandatory `webid`, `userpod`
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
/// Expands to a struct with `webid, userpod, inference_port, skills`.
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
                $($field : $ty),*
            ) -> Self {
                Self { webid, userpod, $($field),* }
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
        }

        impl $name {
            pub fn new(
                webid: hkask_types::WebID,
                userpod: String,
            ) -> Self {
                Self { webid, userpod }
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
