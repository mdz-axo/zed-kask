//! Global tool-invoker hook for direct MCP tool dispatch from UI surfaces.
//!
//! The trait + accessor live in the `hkask-tool-invoker` leaf crate (relocated
//! from here so the kask GPUI widget crates can dispatch without depending on
//! the heavy `swarm_panel` crate). This module re-exports them so existing
//! `swarm_panel::ToolInvoker` / `swarm_panel::set_tool_invoker` /
//! `swarm_panel::shared_tool_invoker` call sites compile unchanged.
//!
//! The production impl (`PanelToolInvoker`) lives in `crates/zed/src/main.rs`
//! and delegates to `McpRuntime` (which implements `ToolPort`), so every call
//! flows through the same governed, OCAP-gated, gas-budgeted path as
//! agent-initiated tool calls. The hook is set from the zed composition root
//! (`main.rs`) after the deferred task resolves the bridge ports, and read by
//! `SwarmPanel` methods via [`shared_tool_invoker`].
//!
//! Previously lived in the `kask_panel` crate (D10) alongside the now-removed
//! chat panel and standalone visualization views. Moved here when the kask
//! panel was replaced by inline chat-stream widgets (D18 viz block renderers).

pub use hkask_tool_invoker::{ToolInvoker, set_tool_invoker, shared_tool_invoker};
