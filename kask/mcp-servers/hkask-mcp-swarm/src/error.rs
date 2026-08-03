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
