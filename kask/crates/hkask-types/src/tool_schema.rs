//! Tool input schema helpers for hKask MCP servers.
//!
//! The concrete problem this module solves: Ollama's Go API decodes each tool's
//! `parameters.properties` as `map[string]api.ToolProperty` — a struct — and
//! rejects JSON Schema *boolean schemas* (`true`/`false`) used as property
//! values, even though booleans are valid JSON Schema (`true` = "accept any
//! value"). One such boolean in any enabled tool's schema makes Ollama fail the
//! entire chat-completion request with `400 cannot unmarshal bool into ... of
//! type api.ToolProperty`.
//!
//! schemars' `impl JsonSchema for serde_json::Value` returns the bare boolean
//! `true`, so any MCP tool input field typed `serde_json::Value` (or
//! `Option<serde_json::Value>`) renders as `properties.<field> = true` and
//! breaks Ollama-routed turns. [`AnyJsonValue`] is a transparent `Value`
//! wrapper whose `JsonSchema` emits the empty object `{}` — equally permissive
//! ("any value") but JSON-object-shaped so strict tool-schema decoders accept
//! it.
//!
//! This module lives in `hkask-types` (rather than `hkask-mcp-server`) so that
//! [`find_boolean_schema_positions`] without pulling in `hkask-mcp-server`'s
//! heavy transitive deps (`rmcp`, `reqwest`, `hkask-keystore`,
//! `tracing-subscriber`, …). `hkask-mcp-server` re-exports these items for
//! backward compatibility with the many MCP server crates that import them via
//! `hkask_mcp_server::`.

use std::borrow::Cow;
use std::ops::Deref;

use schemars::JsonSchema;
use schemars::Schema;
use schemars::SchemaGenerator;
use serde::{Deserialize, Serialize};

/// A `serde_json::Value` whose `JsonSchema` is the empty object `{}` ("accept any
/// value") instead of the bare boolean `true` that schemars emits for `Value`.
///
/// Serialize/Deserialize are transparent, so the wire value is unchanged (any
/// JSON) — only the generated tool input schema differs. Use this for MCP tool
/// input fields that must accept arbitrary JSON, so the field's schema is a JSON
/// object (accepted by Ollama's `api.ToolProperty`) rather than a boolean.
///
/// Derefs to the inner `serde_json::Value`, so existing call sites using
/// `.is_null()`, `.as_object()`, etc. work unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AnyJsonValue(pub serde_json::Value);

impl Deref for AnyJsonValue {
    type Target = serde_json::Value;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<serde_json::Value> for AnyJsonValue {
    fn from(value: serde_json::Value) -> Self {
        Self(value)
    }
}

impl From<AnyJsonValue> for serde_json::Value {
    fn from(value: AnyJsonValue) -> Self {
        value.0
    }
}

impl JsonSchema for AnyJsonValue {
    // Inline so the property value is the schema object directly, not a
    // `$ref` into `$defs`. (Zed's `adapt_schema_to_format` inlines `$defs`
    // anyway, but inlining here keeps the raw MCP `tools/list` schema clean
    // for any consumer that doesn't run that pass.)
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "AnyJsonValue".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // Empty object schema == "any value", but JSON-object-shaped so strict
        // tool-schema decoders (Ollama's `api.ToolProperty`) don't reject a
        // boolean schema.
        Schema::from(serde_json::Map::new())
    }
}

/// Scan a tool input schema (as produced by `schemars::schema_for!`) for bare
/// boolean JSON Schema values in schema-valued positions, returning the
/// JSON-pointer path of each occurrence.
///
/// schemars renders `serde_json::Value` as the bare boolean `true`. A boolean in
/// a schema position (a property value, `additionalProperties`, `items`, an
/// `allOf`/`anyOf`/`oneOf` member, etc.) is rejected by strict-schema-decoding
/// providers — Ollama fails the whole chat-completion with `400 cannot unmarshal
/// bool into ... of type api.ToolProperty`; Google Gemini's protobuf `Schema` is
/// the same class of failure. MCP server tool-input tests should call this on
/// `schema_for!(TheirRequest)` and assert the result is empty, so a future
/// `serde_json::Value`-typed tool input is caught at CI before it breaks any
/// strict provider.
///
/// Only positions whose JSON-Schema-semantic value is itself a schema are
/// inspected; non-schema boolean fields (`nullable`, `required` arrays, etc.)
/// are ignored.
#[must_use]
pub fn find_boolean_schema_positions(schema: &serde_json::Value) -> Vec<String> {
    let mut found = Vec::new();
    collect_in_schema(schema, "", &mut found);
    found
}

/// `value` is assumed to be a JSON Schema at `path`; collect any bare booleans
/// in schema-valued positions reachable from it.
fn collect_in_schema(value: &serde_json::Value, path: &str, found: &mut Vec<String>) {
    match value {
        serde_json::Value::Bool(_) if !path.is_empty() => {
            // The value itself is a boolean schema at a named schema position.
            // (The root parameters schema is always an object for tool inputs,
            // so a top-level bool has an empty path and is not recorded.)
            found.push(path.to_string());
        }
        serde_json::Value::Object(obj) => {
            // Map-of-schema positions: each value is itself a schema.
            for key in ["properties", "patternProperties", "$defs", "definitions"] {
                if let Some(serde_json::Value::Object(map)) = obj.get(key) {
                    for (k, child) in map {
                        collect_in_schema(child, &format!("{path}/{key}/{k}"), found);
                    }
                }
            }
            // Single-schema positions.
            for key in [
                "additionalProperties",
                "additionalItems",
                "contains",
                "propertyNames",
                "not",
                "if",
                "then",
                "else",
            ] {
                if let Some(child) = obj.get(key) {
                    collect_in_schema(child, &format!("{path}/{key}"), found);
                }
            }
            // `items` is either a single schema or an array of schemas.
            if let Some(items) = obj.get("items") {
                match items {
                    serde_json::Value::Array(arr) => {
                        for (i, item) in arr.iter().enumerate() {
                            collect_in_schema(item, &format!("{path}/items/{i}"), found);
                        }
                    }
                    _ => collect_in_schema(items, &format!("{path}/items"), found),
                }
            }
            // Array-of-schema positions.
            for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
                if let Some(serde_json::Value::Array(arr)) = obj.get(key) {
                    for (i, item) in arr.iter().enumerate() {
                        collect_in_schema(item, &format!("{path}/{key}/{i}"), found);
                    }
                }
            }
            // `dependencies` (draft-07): values are either a schema or an array
            // of property names. Only inspect schema-shaped values.
            if let Some(serde_json::Value::Object(map)) = obj.get("dependencies") {
                for (k, child) in map {
                    if matches!(
                        child,
                        serde_json::Value::Bool(_) | serde_json::Value::Object(_)
                    ) {
                        collect_in_schema(child, &format!("{path}/dependencies/{k}"), found);
                    }
                }
            }
        }
        // A bare array is not a schema; nothing to scan.
        _ => {}
    }
}
