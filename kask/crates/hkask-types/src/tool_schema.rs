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
//! wrapper whose `JsonSchema` emits an object-typed permissive schema —
//! JSON-object-shaped so strict tool-schema decoders accept it, and
//! concrete enough that schema-guided tool-call parsers bind the value
//! (live-observed 2026-09-10: the earlier empty-object `{}` form was
//! silently dropped from tool-call arguments on the GLM/OpenRouter path).
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

/// A `serde_json::Value` whose `JsonSchema` is an object-typed permissive
/// schema (`{"type": "object", "properties": {}, "additionalProperties": {}}`)
/// instead of the bare boolean `true` that schemars emits for `Value`.
///
/// Serialize/Deserialize are transparent, so the wire value is unchanged (any
/// JSON) — only the generated tool input schema differs. Use this for MCP tool
/// input fields that must accept arbitrary JSON, so the field's schema is a
/// JSON object (accepted by Ollama's `api.ToolProperty`) rather than a
/// boolean, and binds as an object in schema-guided tool-call parsers.
///
/// The earlier empty-object `{}` form ("any value" in JSON Schema) was
/// live-observed dropped from tool-call arguments on the GLM/OpenRouter path
/// (2026-09-10, `company_screener`'s `criteria_overrides`: the model emitted
/// the parameter, prompt/limit arrived, the `{}`-schema parameter vanished
/// before dispatch — reproduced 3×). A concrete object type with permissive
/// `additionalProperties` gives schema-guided parsers something to bind.
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
    // `$ref` into `$defs`. (Zed's `normalize_tool_schema` inlines `$defs`
    // anyway, but inlining here keeps the raw MCP `tools/list` schema clean
    // for any consumer that doesn't run that pass.)
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "AnyJsonValue".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // Object-typed permissive schema. The keys are carried explicitly so
        // Zed's `preprocess_json_schema` (which injects `additionalProperties:
        // false` into type-object schemas lacking it) cannot tighten this
        // into "no properties allowed" — the trap that would reintroduce the
        // drop the concrete type exists to fix. `additionalProperties` is an
        // empty SCHEMA object, never a bare boolean — Ollama's
        // `api.ToolProperty` rejects booleans in schema positions.
        let mut map = serde_json::Map::new();
        map.insert(
            "type".to_string(),
            serde_json::Value::String("object".to_string()),
        );
        map.insert(
            "properties".to_string(),
            serde_json::Value::Object(serde_json::Map::new()),
        );
        map.insert(
            "additionalProperties".to_string(),
            serde_json::Value::Object(serde_json::Map::new()),
        );
        Schema::from(map)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The AnyJsonValue schema must be object-typed and permissive: the empty
    /// `{}` form was live-observed dropped from tool-call arguments by
    /// schema-guided parsing on the GLM/OpenRouter path (2026-09-10), and a
    /// explicit `additionalProperties` preserves arbitrary content through
    /// schema normalization and provider-specific validation.
    #[test]
    fn any_json_value_schema_is_object_typed_and_permissive() {
        let mut schema = serde_json::to_value(schemars::schema_for!(AnyJsonValue)).expect("schema");
        // `schema_for!` adds root metadata (`$schema`, `title`) that
        // `normalize_tool_schema` strips before the schema reaches the
        // model — remove them the same way to assert the wire shape.
        if let Some(object) = schema.as_object_mut() {
            object.remove("$schema");
            object.remove("title");
        }
        assert_eq!(
            schema,
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": {}
            })
        );
    }

    /// The schema must stay free of bare booleans (Ollama/Gemini reject
    /// them) — including inside `additionalProperties`.
    #[test]
    fn any_json_value_schema_has_no_boolean_positions() {
        let schema = serde_json::to_value(schemars::schema_for!(AnyJsonValue)).expect("schema");
        assert!(find_boolean_schema_positions(&schema).is_empty());
    }

    /// A struct field typed AnyJsonValue renders the property as the
    /// object-typed permissive schema — the shape tool-call parsers bind.
    #[test]
    fn any_json_value_property_renders_object_typed_in_parent_schema() {
        // Fixture field: it exists only for the JsonSchema derive — schema
        // generation is type-level and never reads the value. A justified
        // allow, recorded on scripts/dead-code-allow-baseline.txt.
        #[derive(schemars::JsonSchema)]
        struct Request {
            #[allow(dead_code)]
            payload: AnyJsonValue,
        }
        let schema = serde_json::to_value(schemars::schema_for!(Request)).expect("schema");
        assert_eq!(
            schema["properties"]["payload"],
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": {}
            })
        );
    }
}
