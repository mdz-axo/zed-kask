//! Block provenance — the server-authoritative wire contract for widget
//! dispatch (moved from `hkask-tool-invoker` per the LogiSheets plan §5.2).
//!
//! Provenance for a rendered widget block: which MCP tool produced this
//! artifact, with which args, and under which regulation span.
//!
//! A widget carries this so it can re-issue the originating tool with modified
//! args — letting the user iterate on the displayed artifact (e.g. scrub a
//! portfolio date range, override a scenario probability, move a kanban task)
//! without re-explaining the request to the agent. The block body is the agent's
//! output, so provenance is only as honest as the emitter; MCP servers bake it
//! into their `display_hint` blocks (authoritative) rather than relying on the
//! agent to copy it faithfully.
//!
//! Every field is `#[serde(default)]` so adding provenance to a block body is
//! non-breaking: bodies emitted before provenance lands parse with all fields
//! empty, and the widget falls back to a read-only display.
//!
//! ## Why this module exists
//!
//! The type previously lived beside the GPUI-dependent `ToolInvoker` code, so
//! MCP-side block construction could not depend on it without dragging GPUI
//! into a server process — the media MCP server carries a hand-mirrored
//! duplicate (`Provenance::for_tool`) for exactly this reason. The move gives
//! every side one Zed-free, server-authoritative provenance contract;
//! `hkask-tool-invoker` re-exports it so existing widget imports compile
//! unchanged.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

    /// Whether provenance carries no dispatchable signal (no tool, no server,
    /// null/absent args) — the shape a block body emitted before provenance
    /// landed has. Widgets use this to decide between a provenance-driven
    /// dispatch and the hardcoded fallback; any other non-dispatchable shape is
    /// treated as a partial/incomplete provenance and disabled.
    pub fn is_empty(&self) -> bool {
        self.tool.is_none()
            && self.server.is_none()
            && (self.args.is_null()
                || self
                    .args
                    .as_object()
                    .map(serde_json::Map::is_empty)
                    .unwrap_or(false))
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
        assert!(p.is_empty());
    }

    #[test]
    fn provenance_parses_partial_body() {
        let p: BlockProvenance = serde_json::from_str(r#"{"tool":"portfolio_returns"}"#).unwrap();
        assert_eq!(p.tool.as_deref(), Some("portfolio_returns"));
        assert!(p.server.is_none());
        assert!(!p.is_dispatchable());
        assert!(!p.is_empty());
    }

    #[test]
    fn provenance_dispatchable_when_tool_and_server_present() {
        let p: BlockProvenance = serde_json::from_str(
            r#"{"tool":"scenario_quantify","server":"hkask-mcp-scenarios","args":{"event_id":"e1"}}"#,
        )
        .unwrap();
        assert!(p.is_dispatchable());
        assert!(!p.is_empty());
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
        assert!(p.is_empty());
    }

    #[test]
    fn provenance_round_trips_exactly() {
        let p = BlockProvenance {
            tool: Some("portfolio_characteristics".into()),
            server: Some("hkask-mcp-portfolio".into()),
            args: serde_json::json!({"portfolio": "main", "date": "2026-09-18"}),
            span_id: Some("span-42".into()),
        };
        let json = serde_json::to_string(&p).expect("serialize");
        let back: BlockProvenance = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, p, "round trip changed the value");
        let again = serde_json::to_string(&back).expect("reserialize");
        assert_eq!(json, again, "round trip changed the bytes");
    }
}
