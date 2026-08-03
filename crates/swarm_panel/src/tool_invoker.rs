//! Global tool-invoker hook for direct MCP tool dispatch from the swarm panel.
//!
//! The swarm panel fetches agent/swarm data and performs lifecycle actions
//! (hire, fire, clone, publish, etc.) through the governed, OCAP-gated MCP
//! runtime. This hook is set from the zed composition root (`main.rs`) after
//! the deferred task resolves the bridge ports, and read by `SwarmPanel`
//! methods via [`shared_tool_invoker`].
//!
//! Previously lived in the `kask_panel` crate (D10) alongside the now-removed
//! chat panel and standalone visualization views. Moved here when the kask
//! panel was replaced by inline chat-stream widgets (D18 viz block renderers).

use std::sync::Arc;

use gpui::Task;
use serde_json::Value;

/// Invoke MCP tools directly from UI panels, bypassing the agent conversation
/// loop. The implementation (in `main.rs` as `PanelToolInvoker`) delegates to
/// `McpRuntime` (which implements `ToolPort`), so every call flows through the
/// same governed, OCAP-gated, gas-budgeted path as agent-initiated tool calls.
pub trait ToolInvoker: Send + Sync {
    /// Invoke a tool on a specific MCP server. Returns the result as JSON text.
    fn invoke_tool(&self, server: &str, tool: &str, args: Value) -> Task<Result<String, String>>;
}

static TOOL_INVOKER: std::sync::Mutex<Option<Arc<dyn ToolInvoker>>> = std::sync::Mutex::new(None);

/// Inject the global tool invoker (composition root).
///
/// Called from `main.rs` after the deferred task resolves the bridge ports.
/// Re-settable — later calls replace the earlier invoker.
pub fn set_tool_invoker(invoker: Option<Arc<dyn ToolInvoker>>) {
    *TOOL_INVOKER.lock().expect("TOOL_INVOKER poisoned") = invoker;
}

/// Access the global tool invoker.
pub fn shared_tool_invoker() -> Option<Arc<dyn ToolInvoker>> {
    TOOL_INVOKER.lock().expect("TOOL_INVOKER poisoned").clone()
}
