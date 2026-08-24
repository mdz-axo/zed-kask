//! MCP server scaffolding — shared helpers for hKask MCP server binaries.
//
//! WebID resolution order: `HKASK_WEBID` → anonymous.
//! No ambient authority — all identity and credentials flow through `ServerContext`.
//
//! ```rust,ignore
//! use hkask_mcp::server::{run_stdio_server, CredentialRequirement, ServerContext};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     run_stdio_server(
//!         "hkask-mcp-web",
//!         env!("CARGO_PKG_VERSION"),
//!         |ctx: ServerContext| {
//!             Ok(WebServer::new(ctx.webid))
//!         },
//!         vec![],
//!     ).await
//! }
//! ```

mod context;
mod credentials;
mod error;
mod http_helpers;
mod tool_span;
mod transport;
mod validation;

// ── Re-exports ─────────────────────────────────────────────────────────────

pub use crate::security::{validate_tool_url_permissive, validate_tool_url_with_dns};
pub use context::{CapabilityTier, CredentialRequirement, ServerContext};
pub use credentials::{parse_env_warn, resolve_credential, resolve_db_passphrase};
pub use error::{McpError, McpToolError};
pub use http_helpers::classify_http_error;
pub use tool_span::{ToolContext, execute_tool, execute_tool_semantic};
pub use transport::run_stdio_server;
pub use validation::{
    MAX_READ_BYTES, contain_for_read, contain_for_write, read_capped, resolve_max_read_bytes,
};
pub use validation::{
    map_infra_error, map_io_error, map_join_error, map_memory_store_error, validate_identifier,
    validate_path,
};
