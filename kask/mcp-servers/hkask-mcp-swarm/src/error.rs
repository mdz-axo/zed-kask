//! ABW swarm client error type.
//!
//! Extracted from the swarm server root (continues the per-concern extraction).
//! `SwarmError` maps ABW HTTP errors and body-embedded domain errors (never
//! leaks reqwest types); `into_tool_error` projects it onto the MCP tool error
//! surface with the appropriate kind.

use hkask_mcp_server::server::McpToolError;

/// Errors from the ABW swarm client. Maps ABW HTTP errors AND body-embedded
/// domain errors; never leaks reqwest types.
#[derive(Debug, thiserror::Error)]
pub enum SwarmError {
    /// 401 / missing or invalid API key.
    #[error("ABW authentication failed: {0}. Set HKASK_ABW_API_KEY (Pro tier required).")]
    Auth(String),
    /// 402 — credits exhausted (algedonic).
    #[error("ABW payment required: {0}")]
    PaymentRequired(String),
    /// 500 "not funded" — the agent's owner has not configured an LLM key on
    /// their ABW profile. Execution funding is owner-side, not caller-side.
    #[error("ABW agent '{agent}' is not funded: {message}")]
    AgentNotFunded { agent: String, message: String },
    /// HTTP 200 envelope containing an upstream LLM/provider error string.
    /// Algedonic-adjacent: surface verbatim, do not retry blindly.
    #[error("ABW upstream model error ({provider}): {message}")]
    UpstreamModelError { provider: String, message: String },
    /// 429.
    #[error("ABW rate limited: {0}")]
    RateLimited(String),
    /// Xaman Ek session creation failed.
    #[error("ABW curator unavailable: {0}")]
    CuratorUnavailable(String),
    /// Serde parse failure on a known endpoint — possible API drift (S4).
    #[error("ABW API version mismatch: {0}")]
    ApiVersionMismatch(String),
    /// A spend tool was invoked without a valid consent token. The gate is
    /// the enforcement point — this is a hard refusal, not a warning.
    #[error(
        "ABW spend refused: {0}. Obtain operator consent via the swarm panel (Hire… → Confirm) and retry with the issued consent token."
    )]
    ConsentDenied(String),
    /// Network/transport failure.
    #[error("ABW request failed: {0}")]
    Unavailable(String),
}

impl SwarmError {
    /// Convert into the MCP tool error surface with the appropriate kind.
    pub fn into_tool_error(self) -> McpToolError {
        match self {
            Self::Auth(m) => McpToolError::permission_denied(m),
            Self::PaymentRequired(m) => McpToolError::permission_denied(m),
            Self::AgentNotFunded { .. } => McpToolError::unavailable(self.to_string()),
            Self::UpstreamModelError { .. } => McpToolError::unavailable(self.to_string()),
            Self::RateLimited(m) => McpToolError::rate_limited(m),
            Self::CuratorUnavailable(m) => McpToolError::unavailable(m),
            Self::ApiVersionMismatch(m) => McpToolError::internal(m),
            Self::ConsentDenied(m) => McpToolError::permission_denied(m),
            Self::Unavailable(m) => McpToolError::unavailable(m),
        }
    }
}

/// Errors from the local swarm runtime layer — the local-runtime counterpart
/// of [`SwarmError`] (which maps ABW HTTP errors).
///
/// `SwarmError` is ABW-specific (auth, payment, upstream model errors); the
/// local-runtime layer (local ledger, local agent/swarm registries, local
/// knowledge tools, consent store, A2A HTTP gateway) has a different failure
/// surface — filesystem I/O, SQLite/ledger ops, input validation, path
/// sanitization. Carrying these as `String` (the prior `Result<_, String>`
/// signatures) erased the error kind, forcing the tool-method boundary to
/// blanket-map everything to `McpToolError::internal` — the `.rules`
/// "MCP tool error classification" trap. `LocalSwarmError` keeps the kind so
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
    /// Ledger operation failure (commit, query, `ensure_account`, balance,
    /// debit). Infrastructure-class → `Internal`. Distinguished from
    /// `Database` because the ledger is the local spend surface.
    #[error("ledger error: {0}")]
    Ledger(String),
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
    /// A dependency is unavailable (inference port, bound socket, guard
    /// rejection of generated output). Transient or capability-class →
    /// `Unavailable`.
    #[error("unavailable: {0}")]
    Unavailable(String),
}

/// Compose an ABW [`SwarmError`] into the local error type. ABW upstream/auth
/// failures become [`LocalSwarmError::Unavailable`] (the operation cannot
/// proceed); a consent denial is a caller-fixable input problem
/// ([`LocalSwarmError::InvalidInput`]). Lets a call site that mixes local and
/// ABW work propagate both through a single `Result<_, LocalSwarmError>` via
/// `?`.
impl From<SwarmError> for LocalSwarmError {
    fn from(e: SwarmError) -> Self {
        match e {
            SwarmError::ConsentDenied(m) => LocalSwarmError::InvalidInput(m),
            other => LocalSwarmError::Unavailable(other.to_string()),
        }
    }
}

/// Classify a [`LocalSwarmError`] into the MCP wire-level [`McpToolError`]
/// kind, per variant (not a blanket `Internal`) — the `.rules` "MCP tool error
/// classification" trap. `Io`/`Database`/`Ledger` are infrastructure failures
/// (`Internal`); `InvalidInput`/`Sanitize` are caller-fixable
/// (`InvalidArgument`); `NotFound` → `NotFound`; `Unavailable` →
/// `Unavailable`.
pub fn map_local_swarm_error(e: LocalSwarmError) -> McpToolError {
    match e {
        LocalSwarmError::Io(m) | LocalSwarmError::Database(m) | LocalSwarmError::Ledger(m) => {
            McpToolError::internal(m)
        }
        LocalSwarmError::InvalidInput(m) | LocalSwarmError::Sanitize(m) => {
            McpToolError::invalid_argument(m)
        }
        LocalSwarmError::NotFound(m) => McpToolError::not_found(m),
        LocalSwarmError::Unavailable(m) => McpToolError::unavailable(m),
    }
}
