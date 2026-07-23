use std::future::Future;
use std::pin::Pin;

use crate::DelegationToken;
use hkask_types::NotFound;

/// Governance membrane error types.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolPortError {
    #[error("Capability denied: {0}")]
    CapabilityDenied(String),
    #[error("Gas budget exceeded: {0}")]
    EnergyBudgetExceeded(String),
    #[error("Tool not found: {0}")]
    NotFound(NotFound),
    #[error("Tool invocation failed: {0}")]
    InvocationFailed(String),
}

impl From<NotFound> for ToolPortError {
    fn from(nf: NotFound) -> Self {
        ToolPortError::NotFound(nf)
    }
}

/// Pinned boxed future type used by [`ToolPort`] for dyn-compatibility.
pub type ToolFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Governance membrane for MCP tool invocation.
///
/// `McpRuntime` checks OCAP authority → budget → dispatch → account cost → emit outcome.
///
/// # Authentication asymmetry
///
/// `invoke()` requires a [`DelegationToken`] — every tool execution is OCAP-gated (P4).
/// `discover_tools()` and `get_tool_info()` are **intentionally unauthenticated** — tool
/// schemas are public metadata (the agent must know what tools exist before it can request
/// a token to use them). This follows the MCP protocol's own design: `tools/list` is an
/// unauthenticated handshake. OCAP enforcement applies at the actuator boundary
/// (`invoke`), not the sensor boundary (`discover`).
///
/// # Dyn-compatibility
///
/// All methods return `Pin<Box<dyn Future + Send + '_>>` (via [`ToolFuture`]) so the trait
/// is object-safe: `Arc<dyn ToolPort>` works. This eliminates the adapter layers that
/// previously wrapped `McpRuntime` to satisfy a non-dyn `ToolPort`.
pub trait ToolPort: Send + Sync {
    /// Invoke a tool. Requires a [`DelegationToken`] proving OCAP authorization.
    ///
    /// pre:  token must be valid and not expired
    /// post: returns tool output or `ToolPortError::CapabilityDenied` if token is insufficient
    fn invoke<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
        args: serde_json::Value,
        token: &'a DelegationToken,
    ) -> ToolFuture<'a, Result<serde_json::Value, ToolPortError>>;

    /// Discover available tools. Public metadata — no token required.
    ///
    /// Tool schemas are public per the MCP protocol design:
    /// `tools/list` is an unauthenticated handshake. OCAP enforcement
    /// applies at the actuator boundary (`invoke`), not here.
    fn discover_tools<'a>(&'a self) -> ToolFuture<'a, Vec<String>>;

    /// Get metadata for a specific tool. Public metadata — no token required.
    fn get_tool_info<'a>(&'a self, tool_name: &'a str) -> ToolFuture<'a, Option<ToolInfo>>;
}

/// Canonical tool metadata for OCAP capability matching.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub server_id: String,
    /// The capability required to invoke this tool, derived from the server ID.
    /// Maps `hkask-mcp-<domain>` → `tool:<domain>:execute`.
    /// `None` for servers that don't follow the `hkask-mcp-` naming convention.
    pub required_capability: Option<String>,
    /// FIDES taint label for information flow control (Layer 5 defense).
    /// Source: Microsoft Research FIDES (arXiv:2505.23643)
    /// Defaults to `Pure` (no side effects, no external data).
    /// `Source`: returns untrusted data. `Sink`: state-changing.
    /// `Endorser`: trusted extraction from untrusted input.
    pub taint: hkask_types::ToolTaint,
}
