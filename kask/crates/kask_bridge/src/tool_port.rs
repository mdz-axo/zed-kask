//! `ToolPort` adapter — wraps hKask's `McpRuntime` for the bridge.
//!
//! This is the D3 seam. The `McpRuntime` already implements `ToolPort` (OCAP-gated
//! tool invocation with gas/rjoule tracking and `reg.tool.*` span emission).
//! The bridge holds an `Arc<McpRuntime>` and delegates `ToolPort` calls to it.
//!
//! The `McpRuntime` starts MCP servers as child processes (stdio transport).
//! Long-term (full R4 refactor), the MCP servers will be refactored to take
//! direct in-process handles — but for now, the child-process model works and
//! the `ToolPort` contract is satisfied.

use std::sync::Arc;

use async_trait::async_trait;
use hkask_capability::{DelegationToken, ToolFuture, ToolInfo, ToolPort, ToolPortError};
use hkask_mcp::McpRuntime;
use serde_json::Value;

/// `ToolPort` implementation over hKask's `McpRuntime`.
///
/// The `McpRuntime` manages MCP server processes and implements `ToolPort`
/// directly (OCAP verify → gas reserve → dispatch → settle → span emit).
/// This adapter is a thin pass-through — it exists so the bridge can
/// construct the `ManifestExecutor` with a `ToolPort` that actually works.
pub struct BridgeToolPort {
    runtime: Arc<McpRuntime>,
}

impl BridgeToolPort {
    pub fn new(runtime: Arc<McpRuntime>) -> Self {
        Self { runtime }
    }

    /// Get a reference to the inner `McpRuntime` (for server startup, etc.).
    pub fn runtime(&self) -> &McpRuntime {
        &self.runtime
    }
}

#[async_trait]
impl ToolPort for BridgeToolPort {
    fn invoke<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
        args: Value,
        token: &'a DelegationToken,
    ) -> ToolFuture<'a, Result<Value, ToolPortError>> {
        Box::pin(async move { ToolPort::invoke(&*self.runtime, server, tool, args, token).await })
    }

    fn discover_tools<'a>(&'a self) -> ToolFuture<'a, Vec<String>> {
        Box::pin(async move { self.runtime.discover_tools().await })
    }

    fn get_tool_info<'a>(&'a self, tool_name: &'a str) -> ToolFuture<'a, Option<ToolInfo>> {
        Box::pin(async move { self.runtime.get_tool_info(tool_name).await })
    }
}
