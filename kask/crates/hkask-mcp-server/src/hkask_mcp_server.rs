#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! hKask MCP Server — MCP server utilities and startup verification.
//!
//! Provides the lightweight layer that all hKask MCP servers depend on:
//! - Server scaffolding (McpToolError, ServerContext, CredentialRequirement, run_stdio_server)
//! - URL validation, identifier validation, HTTP helpers
//! - Macros: validate_field!, impl_tool_context!, mcp_server!

pub(crate) mod security;
pub mod server;

// NOTE: The canonical MCP server registry lives in
// `kask_bridge::mcp_servers::BUILT_IN_MCP_SERVERS` (id + binary + description).
// Do NOT re-introduce a parallel list here — it drifts (the previous
// `BUILTIN_SERVERS` used id `"kanban"` while the canonical list uses
// `"kata-kanban"`, and the two contradicted each other silently).

pub use server::{
    CapabilityTier, CredentialRequirement, McpError, ServerContext, ToolContext, execute_tool,
    load_dotenv, parse_env_warn, resolve_credential, resolve_db_passphrase, run_stdio_server,
    run_stdio_server_with_preloaded, validate_identifier, validate_path,
    validate_tool_url_permissive, validate_tool_url_with_dns,
};
pub use server::{
    MAX_READ_BYTES, contain_for_read, contain_for_write, map_infra_error, map_io_error,
    map_join_error, map_memory_store_error, read_capped,
};

// Re-exported from `hkask-types::tool_schema` so consumers can use
// `hkask_mcp_server::AnyJsonValue` / `find_boolean_schema_positions` without
// pulling `rmcp`/`reqwest`/`hkask-keystore`/`hkask-storage`/
// `tracing-subscriber` (which this crate drags in). The dedicated
// `tool_schema` module file was inlined here — the `tool_schema::` path had
// no external users.
pub use hkask_types::tool_schema::{AnyJsonValue, find_boolean_schema_positions};

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
/// Assumes the struct has a `webid: WebID` field — the standard pattern
/// for all hKask MCP servers.
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
        }
    };
}

/// Define an MCP server struct with standard fields + constructor.
///
/// Generates the struct with a mandatory `webid` field plus any
/// domain-specific fields, a `new()` constructor, and a `ToolContext` impl
/// via `impl_tool_context!`.
///
/// # Example
/// ```ignore
/// mcp_server!(struct SkillServer {
///     inference_port: Arc<dyn InferencePort>,
///     skills: HashMap<String, SkillDef>,
/// });
/// ```
///
/// Expands to a struct with `webid, inference_port, skills`.
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
            $(
                $(#[$field_meta])*
                $field_vis $field : $ty
            ),*
        }

        impl $name {
            pub fn new(
                webid: hkask_types::WebID,
                $($field : $ty),*
            ) -> Self {
                Self { webid, $($field),* }
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
        }

        impl $name {
            pub fn new(webid: hkask_types::WebID) -> Self {
                Self { webid }
            }
        }

        $crate::impl_tool_context!($name);
    };
}
