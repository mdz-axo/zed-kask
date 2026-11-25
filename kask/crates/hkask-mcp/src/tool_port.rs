//! Tool dispatch port (dual-axis review Q6: folded from the retired
//! `hkask-tool-port` crate — the deletion test found the crate's
//! dyn-compat claim unexercised by any `Arc<dyn ToolPort>` consumer and
//! `get_tool_info` was dead trait surface; the types live on here, in the
//! sole implementor).
//!
//! # No per-call capability gate
//!
//! `invoke` performs **no** per-call capability check. Every production
//! mint site derives the resource from the same tool name it passes to
//! `invoke`, so a check would compare a value against itself and deny
//! nothing. Capability *separation* is enforced at the boundaries that
//! hold a list the caller cannot choose: the per-request `tool_allowlist`
//! on the inference IPC dispatch and the per-server MCP env/credential
//! allowlists.
//!
//! The FIDES `ToolTaint` labels also lived here once. They were removed
//! with the runtime-policy gate they fed: every `ToolInfo` was labelled
//! `Pure` at its only construction site, so the `Source`→`Sink` block
//! could not fire.

use hkask_types::NotFound;

/// Tool dispatch error types.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolPortError {
    /// The runaway-loop breaker tripped: this agent exhausted its per-tick call
    /// ceiling. Not an authorization decision — see [`crate::McpRuntime::invoke`].
    #[error("Call cap exceeded: {0}")]
    EnergyBudgetExceeded(String),
    #[error("Tool not found: {0}")]
    NotFound(NotFound),
    /// The tool could not be reached and the request **provably never left**:
    /// there was no live connection, or the transport rejected the send.
    ///
    /// Distinct from [`ToolPortError::InvocationFailed`] because the call never
    /// ran, so a caller may retry it without risking a duplicate side effect.
    /// Callers that render errors to a user should present this as a transient
    /// connection state rather than a failure of the requested operation.
    #[error("Tool unavailable: {0}")]
    Unavailable(String),
    /// The request was delivered but the connection dropped before a response
    /// arrived. **The tool may or may not have applied its effect.**
    ///
    /// This is deliberately *not* retryable. `rmcp` reports both "the send
    /// failed" and "the response channel dropped" as `ServiceError::
    /// TransportClosed`, so once a request has been handed to a live peer, a
    /// transport loss cannot be read as proof of non-delivery. Auto-retrying
    /// here would duplicate side effects — two tasks created, a hire charged
    /// twice. The operator must reconcile state and decide.
    #[error("Tool outcome unknown (connection lost mid-call): {0}")]
    Interrupted(String),
    /// The call reached the tool and the tool failed. Retrying repeats it.
    ///
    /// `kind` is the tool's own typed classification, extracted once from
    /// the server's `structured_content` at dispatch (SP-03) — downstream
    /// consumers match on it structurally instead of re-parsing the
    /// `[kind] ` display marker. `None` when the failure predates the tool
    /// (a pure-skill runner rejection, a transport-level error).
    #[error("Tool invocation failed: {detail}")]
    InvocationFailed {
        detail: String,
        kind: Option<hkask_types::McpErrorKind>,
    },
}

impl ToolPortError {
    /// Whether re-issuing the identical call is both plausibly useful and free
    /// of duplicate-side-effect risk.
    ///
    /// True only for [`ToolPortError::Unavailable`], where the request provably
    /// never reached the tool. [`ToolPortError::Interrupted`] is excluded on
    /// purpose: its outcome is unknown, so a retry could apply an effect twice.
    /// A cap breach needs a new regulation tick, and a failed or unknown tool
    /// will fail identically.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, ToolPortError::Unavailable(_))
    }
}

impl From<NotFound> for ToolPortError {
    fn from(nf: NotFound) -> Self {
        ToolPortError::NotFound(nf)
    }
}

/// Canonical tool metadata for a tool the caller already located by server:
/// the server identity is the caller's `server` argument, not a field here.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}
