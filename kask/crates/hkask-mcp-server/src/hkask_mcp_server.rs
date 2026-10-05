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
    parse_env_warn, resolve_credential, resolve_db_passphrase, run_stdio_server,
    validate_identifier, validate_path, validate_tool_url_permissive, validate_tool_url_with_dns,
};
pub use server::{
    MAX_READ_BYTES, contain_for_read, contain_for_write, map_infra_error, map_io_error,
    map_join_error, map_memory_store_error, read_capped, write_contained,
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

/// Macro to validate an identifier field and return early on error.
///
/// Eliminates the repeated 3-line pattern:
/// ```ignore
/// if let Err(e) = validate_identifier("field", &value, 256) {
///     return Err(span.error(e));
/// }
/// ```
///
/// Usage (inside a tool returning `Result<String, McpToolError>`):
/// ```ignore
/// validate_field!(span, "session_id", &session_id, 256);
/// ```
#[macro_export]
macro_rules! validate_field {
    ($span:expr, $name:expr, $value:expr, $max_len:expr) => {
        if let Err(e) = $crate::validate_identifier($name, $value, $max_len) {
            return Err($span.error(e));
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

/// Pin a server's registered tool surface with one macro call: the exact
/// tool count AND the build.rs-generated `TOOL_NAMES` set against the live
/// router. The count test catches additions/removals; the name test catches
/// same-count renames (which the count alone cannot) — together they make
/// silent registration drift fail a test instead of degrading to
/// tool-not-found at dispatch.
///
/// The invoking test module must bring `TOOL_NAMES` into scope: either a
/// crate-root `include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"))`
/// re-exported through the module's `use super::*`, or a module-local
/// include directly inside the test module. Unqualified `TOOL_NAMES` below
/// resolves at the call site either way.
///
/// `$test_name` keeps each server's historical count-test name (docs cite
/// them); the name test is fixed-shape everywhere.
///
/// # Example
/// ```ignore
/// #[cfg(test)]
/// mod tool_surface_tests {
///     use super::*;
///     include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));
///     hkask_mcp_server::tool_surface_pin!(
///         TrainingServer::combined_router(),
///         "combined_router",
///         9,
///         tool_surface_is_exactly_9_registered_tools,
///     );
/// }
/// ```
#[macro_export]
macro_rules! tool_surface_pin {
    ($router:expr, $router_name:literal, $expected:literal, $test_name:ident $(,)?) => {
        #[test]
        fn $test_name() {
            let n = $router.list_all().len();
            assert_eq!(
                n, $expected,
                "{} registered tool surface changed; got {}",
                $router_name, n
            );
        }

        #[test]
        fn tool_names_match_live_router() {
            let mut live: Vec<String> = $router
                .list_all()
                .iter()
                .map(|tool| tool.name.to_string())
                .collect();
            live.sort();
            let mut generated: Vec<&str> = TOOL_NAMES.to_vec();
            generated.sort();
            assert_eq!(
                generated,
                live.iter().map(String::as_str).collect::<Vec<_>>(),
                concat!(
                    "TOOL_NAMES (build.rs-generated) must match the live ",
                    $router_name,
                    " surface"
                )
            );
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
