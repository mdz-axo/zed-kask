//! Canonical registry of built-in kask MCP servers and child-process env construction.
//!
//! Single source of truth for the server ID → binary name → description mapping
//! and the one canonical env-construction path (`build_mcp_server_env`).

mod catalog;
mod env;
#[cfg(test)]
mod tests;

pub use catalog::{
    BUILT_IN_MCP_SERVERS, BuiltinMcpServer, builtin_mcp_server_ids, builtin_mcp_server_pairs,
    remove_shadowing_context_server_entries, shadowed_context_server_entry_ids,
};
#[cfg(test)]
pub(crate) use env::filter_config_env_for_server;
pub use env::{
    CredentialSweep, build_mcp_server_env, build_mcp_server_env_with_sweep,
    filter_credentials_for_server,
};
