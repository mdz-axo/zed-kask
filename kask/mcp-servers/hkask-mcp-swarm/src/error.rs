//! Local swarm error types.
//!
//! Extracted from the swarm server root (continues the per-concern extraction).
//! [`map_local_swarm_error`] projects each local failure onto the MCP tool
//! error surface with the appropriate kind.

use hkask_mcp_server::server::McpToolError;

/// Errors from the local swarm runtime layer — filesystem I/O, SQLite ops,
/// input validation, path sanitization. Carrying these as `String` (the prior
/// `Result<_, String>` signatures) erased the error kind, forcing the
/// tool-method boundary to blanket-map everything to
/// `McpToolError::internal` — the `.rules` "MCP tool error classification"
/// trap. `LocalSwarmError` keeps the kind so
/// [`map_local_swarm_error`] can project each variant onto the right MCP
/// wire-level kind (`NotFound` → `NotFound`, invalid input → `InvalidArgument`,
/// …) instead of collapsing the whole surface to `Internal`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LocalSwarmError {
    /// Filesystem I/O failure (`create_dir_all`, `read_dir`, `read`, `write`,
    /// `canonicalize`, `remove_dir_all`). Maps to `Internal` at the tool
    /// boundary — the per-`ErrorKind` `NotFound`/`PermissionDenied`
    /// classification for *raw* fs calls in the tool layer is handled by
    /// `hkask_mcp_server::map_io_error` (H2), not by this variant.
    #[error("I/O error: {0}")]
    Io(String),
    /// Generic database/storage failure (pool build, schema init, semantic
    /// memory open/query). Infrastructure-class → `Internal`.
    #[error("database error: {0}")]
    Database(String),
    /// Caller-supplied input failed validation (empty name, bad slug charset,
    /// JSON parse failure, non-positive amount, wrong socket family). Maps to
    /// `InvalidArgument` — caller-fixable.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// A referenced entity (agent, swarm) was not found. Maps to `NotFound`.
    #[error("not found: {0}")]
    NotFound(String),
    /// Path-safety / sanitize failure (`sanitize_agent_id` rejected the id, or
    /// a resolved path escaped its containing directory). Caller-fixable →
    /// `InvalidArgument`.
    #[error("sanitize error: {0}")]
    Sanitize(String),
    /// A dependency is unavailable (inference port, bound socket).
    /// Transient or capability-class → `Unavailable`.
    #[error("unavailable: {0}")]
    Unavailable(String),
}

/// Row-level storage errors (`hkask_types::DbError` from the SQL driver)
/// compose into the local error type as database failures — lets the gate
/// ledger's row loops propagate `row.get_int(..)?` / `row.get_str(..)?`
/// through `Result<_, LocalSwarmError>` the same way the collect-based
/// readers in `thread_store` do over `DbError` directly.
impl From<hkask_types::DbError> for LocalSwarmError {
    fn from(e: hkask_types::DbError) -> Self {
        LocalSwarmError::Database(e.to_string())
    }
}

/// Classify a [`LocalSwarmError`] into the MCP wire-level [`McpToolError`]
/// kind, per variant (not a blanket `Internal`) — the `.rules` "MCP tool error
/// classification" trap. `Io`/`Database` are infrastructure failures
/// (`Internal`); `InvalidInput`/`Sanitize` are caller-fixable
/// (`InvalidArgument`); `NotFound` → `NotFound`; `Unavailable` →
/// `Unavailable`.
pub fn map_local_swarm_error(e: LocalSwarmError) -> McpToolError {
    match e {
        LocalSwarmError::Io(m) | LocalSwarmError::Database(m) => McpToolError::internal(m),
        LocalSwarmError::InvalidInput(m) | LocalSwarmError::Sanitize(m) => {
            McpToolError::invalid_argument(m)
        }
        LocalSwarmError::NotFound(m) => McpToolError::not_found(m),
        LocalSwarmError::Unavailable(m) => McpToolError::unavailable(m),
    }
}
