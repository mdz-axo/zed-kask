//! Seam tests for Candidate C3:
//! 1. `hkask_mcp_server` re-exports `AnyJsonValue` and `find_boolean_schema_positions`
//!    from `hkask_types` at its crate root (`reference.md` contract + macro hygiene).
//! 2. `tool_surface_pin!` enforces `no_boolean_schema_positions_in_live_router` across
//!    every invoking MCP server, rejecting bare boolean property schemas and
//!    `additionalProperties: true` maps with a diagnostic naming the offending tool.
//! 3. `hkask-mcp-server/Cargo.toml` carries neither `reqwest` nor `hkask-inference` in
//!    its production `[dependencies]`.

use std::borrow::Cow;
use std::sync::Arc;

use hkask_mcp_server::{AnyJsonValue, find_boolean_schema_positions};
use rmcp::model::Tool;

struct FixtureRouter(Vec<Tool>);

impl FixtureRouter {
    fn list_all(&self) -> Vec<Tool> {
        self.0.clone()
    }
}

fn tool_with_schema(name: &'static str, schema: serde_json::Value) -> Tool {
    let map = schema
        .as_object()
        .cloned()
        .expect("tool input_schema must be a JSON object");
    Tool::new(
        Cow::Borrowed(name),
        Cow::Borrowed("fixture tool"),
        Arc::new(map),
    )
}

fn clean_router() -> FixtureRouter {
    FixtureRouter(vec![tool_with_schema(
        "fixture_clean_tool",
        serde_json::json!({
            "type": "object",
            "properties": {
                "payload": { "type": "string", "title": "JSON text" }
            },
            "required": ["payload"],
            "additionalProperties": false
        }),
    )])
}

mod pinned_clean_surface {
    use super::clean_router;

    const TOOL_NAMES: &[&str] = &["fixture_clean_tool"];

    hkask_mcp_server::tool_surface_pin!(
        clean_router(),
        "clean_router",
        1,
        fixture_surface_is_exactly_1_registered_tool,
    );
}

#[test]
fn root_reexports_any_json_value_and_boolean_schema_scanner() {
    let decoded: AnyJsonValue =
        serde_json::from_value(serde_json::json!("{\"ok\":true}")).expect("JSON text decodes");
    assert_eq!(decoded.0["ok"], serde_json::json!(true));

    let open = serde_json::json!({
        "type": "object",
        "additionalProperties": true
    });
    assert_eq!(
        find_boolean_schema_positions(&open),
        vec!["/additionalProperties".to_string()]
    );
}

#[test]
#[should_panic(expected = "bad_value_tool live schema contains bare-boolean positions")]
fn router_boolean_schema_gate_rejects_bare_boolean_property_schema() {
    let bad = vec![tool_with_schema(
        "bad_value_tool",
        serde_json::json!({
            "type": "object",
            "properties": {
                "arbitrary": true
            }
        }),
    )];
    hkask_mcp_server::__assert_no_boolean_schema_positions(&bad);
}

#[test]
#[should_panic(expected = "bad_map_tool live schema contains bare-boolean positions")]
fn router_boolean_schema_gate_rejects_open_additional_properties() {
    let bad = vec![tool_with_schema(
        "bad_map_tool",
        serde_json::json!({
            "type": "object",
            "additionalProperties": true
        }),
    )];
    hkask_mcp_server::__assert_no_boolean_schema_positions(&bad);
}

#[test]
fn hkask_mcp_server_manifest_excludes_reqwest_and_hkask_inference() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !manifest.contains("reqwest"),
        "hkask-mcp-server must not pull reqwest into every stdio server's closure"
    );
    assert!(
        !manifest.contains("hkask-inference"),
        "hkask-mcp-server must not pull hkask-inference into every stdio server's closure"
    );
}
