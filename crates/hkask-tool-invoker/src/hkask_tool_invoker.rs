//! Governed widget→MCP dispatch, in a leaf crate.
//!
//! This crate holds two things:
//!
//! 1. The [`ToolInvoker`] trait + its process-global accessor
//!    ([`set_tool_invoker`] / [`shared_tool_invoker`]), relocated verbatim from
//!    `crates/swarm_panel/src/tool_invoker.rs`. The production impl
//!    (`PanelToolInvoker`) lives in `crates/zed/src/main.rs` and delegates to
//!    `McpRuntime` (which implements `ToolPort`), so every call flows through
//!    the same governed, OCAP-gated, gas-budgeted path as agent-initiated tool
//!    calls. Relocating the trait to a leaf crate lets the kask GPUI widget
//!    crates dispatch MCP tools without depending on the heavy `swarm_panel`
//!    crate (which would invert sane layering: leaf widgets → heavy panel).
//!
//!    `swarm_panel` re-exports these symbols so its existing call sites compile
//!    unchanged. The `McpRuntime`/`ToolPort`/token minting stays in `main.rs`
//!    (it needs `hkask-capability` + `hkask-mcp`); only the trait + the global
//!    accessor live here.
//!
//! 2. [`BlockProvenance`] — the payload a rendered widget block carries so the
//!    widget can re-issue the *originating* MCP tool with modified args. Without
//!    it, a widget can display an artifact but cannot iterate on it; the user
//!    must re-ask the agent. Provenance is `#[serde(default)]` and additive on
//!    each block body, so existing tolerant parsers are unaffected.
//!
//! ## Why a leaf crate
//!
//! `hkask-viz-core` depends one-way on the widget crates
//! (`hkask-viz-core/Cargo.toml`). Widget crates do not depend on `viz-core`.
//! A widget that wants to dispatch therefore cannot reach `ToolInvoker` via
//! `viz-core`, and depending on `swarm_panel` would pull `agent`, `editor`,
//! `project`, … into a leaf widget. This crate is the minimal shared seam: it
//! depends only on `gpui` (`Task`), `serde`, and `serde_json`.

use std::sync::Arc;

use gpui::Task;
use serde::Deserialize;
use serde_json::Value;

/// Invoke MCP tools directly from UI surfaces (panels, widgets), bypassing the
/// agent conversation loop. The implementation (in `main.rs` as
/// `PanelToolInvoker`) delegates to `McpRuntime` (which implements `ToolPort`),
/// so every call flows through the same governed, OCAP-gated, gas-budgeted path
/// as agent-initiated tool calls.
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
///
/// Returns `None` when the invoker has not been wired (e.g. before the deferred
/// post-login task runs, or when the MCP runtime is unavailable). Callers MUST
/// surface this as a visible error rather than silently no-op'ing — see the
/// `.rules` "Process-global hooks set at runtime need a startup-failure signal"
/// trap.
pub fn shared_tool_invoker() -> Option<Arc<dyn ToolInvoker>> {
    TOOL_INVOKER.lock().expect("TOOL_INVOKER poisoned").clone()
}

/// Provenance for a rendered widget block: which MCP tool produced this
/// artifact, with which args, and under which regulation span.
///
/// A widget carries this so it can re-issue the originating tool with modified
/// args — letting the user iterate on the displayed artifact (e.g. scrub a
/// portfolio date range, override a scenario probability, move a kanban task)
/// without re-explaining the request to the agent. The block body is the agent's
/// output, so provenance is only as honest as the emitter; MCP servers bake it
/// into their `display_hint` blocks (authoritative) rather than relying on the
/// agent to copy it faithfully.
///
/// Every field is `#[serde(default)]` so adding provenance to a block body is
/// non-breaking: bodies emitted before provenance lands parse with all fields
/// empty, and the widget falls back to a read-only display.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BlockProvenance {
    /// The MCP tool name that produced this block (e.g. `"portfolio_returns"`).
    #[serde(default)]
    pub tool: Option<String>,
    /// The MCP server name that hosts the tool (e.g. `"hkask-mcp-companies"`).
    #[serde(default)]
    pub server: Option<String>,
    /// The args the tool was invoked with, as a JSON object. A widget re-issues
    /// the tool by merging its modification into this object.
    #[serde(default)]
    pub args: Value,
    /// The `reg.*` span id under which the producing tool call was traced, for
    /// observability and re-ask detection.
    #[serde(default)]
    pub span_id: Option<String>,
}

impl BlockProvenance {
    /// Whether this provenance is sufficient to re-issue the tool: it needs
    /// both a tool name and a server name. Widgets use this to decide whether
    /// to show an active affordance or a disabled "ask the agent" hint.
    pub fn is_dispatchable(&self) -> bool {
        self.tool.is_some() && self.server.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_defaults_empty_and_not_dispatchable() {
        let p = BlockProvenance::default();
        assert!(p.tool.is_none());
        assert!(p.server.is_none());
        assert!(p.args.is_null());
        assert!(p.span_id.is_none());
        assert!(!p.is_dispatchable());
    }

    #[test]
    fn provenance_parses_partial_body() {
        let p: BlockProvenance = serde_json::from_str(r#"{"tool":"portfolio_returns"}"#).unwrap();
        assert_eq!(p.tool.as_deref(), Some("portfolio_returns"));
        assert!(p.server.is_none());
        assert!(!p.is_dispatchable());
    }

    #[test]
    fn provenance_dispatchable_when_tool_and_server_present() {
        let p: BlockProvenance = serde_json::from_str(
            r#"{"tool":"scenario_quantify","server":"hkask-mcp-scenarios","args":{"event_id":"e1"}}"#,
        )
        .unwrap();
        assert!(p.is_dispatchable());
    }

    #[test]
    fn provenance_absent_field_parses_as_empty() {
        // A block body emitted before provenance lands has no `provenance` key.
        // The widget parses the body; provenance defaults empty. This pins that
        // adding the field is non-breaking.
        let body: serde_json::Value =
            serde_json::from_str(r#"{"viz":"scenarios","pipeline":{}}"#).unwrap();
        let p: BlockProvenance = body
            .get("provenance")
            .cloned()
            .map(|v| serde_json::from_value(v).unwrap_or_default())
            .unwrap_or_default();
        assert!(!p.is_dispatchable());
    }
}
