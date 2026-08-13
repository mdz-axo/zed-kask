use std::future::Future;
use std::pin::Pin;

use hkask_types::NotFound;

/// Tool dispatch error types.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolPortError {
    /// The runaway-loop breaker tripped: this agent exhausted its per-tick call
    /// ceiling. Not an authorization decision — see [`ToolPort::invoke`].
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
    #[error("Tool invocation failed: {0}")]
    InvocationFailed(String),
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

/// Pinned boxed future type used by [`ToolPort`] for dyn-compatibility.
pub type ToolFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Dispatch port for MCP tool invocation.
///
/// `McpRuntime` meters the call → dispatches → emits the outcome span.
///
/// # This port does not authorize
///
/// `invoke` performs **no** per-call capability check. It previously compared a
/// `DelegationToken`'s declared `(resource, resource_id, action)` against the
/// invoked tool, but every production mint site derived `resource_id` from the
/// same tool name it then passed to `invoke` — the comparison was a value
/// against itself and could not deny. Authority is enforced *outside* this
/// port, at the boundaries that hold a list the caller cannot choose:
///
/// - the per-request `tool_allowlist` on the inference IPC dispatch
///   (`kask_bridge::inference_ipc_server`, fail-closed on missing/empty),
/// - each swarm agent card's declared `mcp_tools` allowlist,
/// - the per-server MCP env/credential allowlists.
///
/// The `agent` argument is an accounting identity, not a credential.
///
/// # Dyn-compatibility
///
/// All methods return `Pin<Box<dyn Future + Send + '_>>` (via [`ToolFuture`]) so the trait
/// is object-safe: `Arc<dyn ToolPort>` works. This eliminates the adapter layers that
/// previously wrapped `McpRuntime` to satisfy a non-dyn `ToolPort`.
pub trait ToolPort: Send + Sync {
    /// Invoke a tool on behalf of `agent`.
    ///
    /// `agent` identifies who to charge and attribute the call to — it is a
    /// meter reading, not a capability. The only way this returns an error
    /// before dispatch is [`ToolPortError::EnergyBudgetExceeded`], the
    /// runaway-loop breaker.
    ///
    /// post: returns tool output, or `EnergyBudgetExceeded` if `agent` exhausted
    ///       its per-tick call ceiling
    fn invoke<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
        args: serde_json::Value,
        agent: hkask_types::WebID,
    ) -> ToolFuture<'a, Result<serde_json::Value, ToolPortError>>;

    /// Discover available tools.
    ///
    /// Tool schemas are public per the MCP protocol design: `tools/list` is an
    /// unauthenticated handshake.
    fn discover_tools<'a>(&'a self) -> ToolFuture<'a, Vec<String>>;

    /// Get metadata for a specific tool.
    fn get_tool_info<'a>(&'a self, tool_name: &'a str) -> ToolFuture<'a, Option<ToolInfo>>;
}

/// Canonical tool metadata.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub server_id: String,
}
