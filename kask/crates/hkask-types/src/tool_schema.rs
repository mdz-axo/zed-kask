//! Provider-neutral encoding for arbitrary JSON tool arguments.
//!
//! Unconstrained schemas are not a portable function-calling contract: some
//! providers reject boolean schemas, while schema-constrained generation can
//! empty open objects. Arbitrary JSON therefore crosses the model boundary as
//! JSON text in a declared string. Decode it before tool execution; structured
//! values and literal strings must round-trip without provider-specific rules.
//!
//! This module also checks schemas for boolean positions rejected by some APIs.

use std::borrow::Cow;
use std::ops::Deref;

use schemars::JsonSchema;
use schemars::Schema;
use schemars::SchemaGenerator;
use serde::{Deserialize, Serialize};

/// An arbitrary structured value encoded as JSON text in tool-call arguments.
///
/// Construct from a `Value` internally; serialization produces a JSON string
/// containing its complete JSON document. Deserialization requires that text
/// and rejects malformed documents. A literal string is encoded as JSON string
/// text too, so JSON-looking strings are never mistaken for objects.
#[derive(Debug, Clone, Default)]
pub struct AnyJsonValue(pub serde_json::Value);

impl Serialize for AnyJsonValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_json_text(&self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for AnyJsonValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        serde_json::from_str(&text).map(Self).map_err(|error| {
            serde::de::Error::custom(format!("expected complete JSON text: {error}"))
        })
    }
}

/// Serialize a native tool's structured map using the same wire contract.
pub fn serialize_json_text<T: Serialize, S: serde::Serializer>(
    value: &T,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let text = serde_json::to_string(value).map_err(serde::ser::Error::custom)?;
    serializer.serialize_str(&text)
}

/// Decode a native map argument without reinterpreting its nested string values.
pub fn deserialize_json_text_object<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
    field: &str,
) -> Result<std::collections::HashMap<String, serde_json::Value>, D::Error> {
    let value = AnyJsonValue::deserialize(deserializer).map_err(|error| {
        serde::de::Error::custom(format!(
            "{field}: expected JSON text encoding an object: {error}"
        ))
    })?;
    serde_json::from_value(value.0).map_err(|error| {
        serde::de::Error::custom(format!(
            "{field}: expected JSON text encoding an object: {error}"
        ))
    })
}

/// The scalar schema shared by native maps and arbitrary MCP values.
pub fn json_text_schema(_: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({
        "type": "string",
        "title": "JSON text",
        "description": "Complete JSON text encoding the value, for example {\"count\":7}. Encode a literal string as quoted JSON text."
    })
}

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

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_text_schema(generator)
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

    #[test]
    fn arbitrary_tool_json_round_trips_as_text_without_changing_values() {
        for value in [
            json!({"count": 7, "nested": {"enabled": true}, "items": [null, "quoted \"text\""]}),
            json!([1, false, {"unicode": "café"}]),
            json!("{\"literal\":\"JSON-looking string\"}"),
            json!(42),
            json!(true),
            json!(null),
        ] {
            let input = AnyJsonValue::from(value.clone());
            let wire = serde_json::to_value(&input).expect("serialize tool JSON");
            assert!(
                wire.is_string(),
                "arbitrary JSON must have a scalar wire type"
            );
            let decoded: AnyJsonValue = serde_json::from_value(wire).expect("decode tool JSON");
            assert_eq!(decoded.0, value);
        }
    }

    #[test]
    fn arbitrary_tool_json_rejects_invalid_text_and_unencoded_objects() {
        for wire in [
            json!("{\"cut\":"),
            json!("not JSON"),
            json!({"unencoded": true}),
        ] {
            assert!(serde_json::from_value::<AnyJsonValue>(wire).is_err());
        }
    }

    #[test]
    fn any_json_value_schema_declares_json_text_not_an_open_object() {
        let schema = serde_json::to_value(schemars::schema_for!(AnyJsonValue)).expect("schema");
        assert_eq!(schema["type"], "string");
        assert!(schema.get("properties").is_none());
        assert!(schema.get("additionalProperties").is_none());
    }

    /// The schema must stay free of bare booleans (Ollama/Gemini reject
    /// them) — including inside `additionalProperties`.
    #[test]
    fn any_json_value_schema_has_no_boolean_positions() {
        let schema = serde_json::to_value(schemars::schema_for!(AnyJsonValue)).expect("schema");
        assert!(find_boolean_schema_positions(&schema).is_empty());
    }

    #[test]
    fn arbitrary_json_property_schema_matches_its_serialized_wire_value() {
        #[derive(Serialize, Deserialize, JsonSchema)]
        struct Request {
            payload: AnyJsonValue,
        }
        let schema = serde_json::to_value(schemars::schema_for!(Request)).expect("schema");
        assert_eq!(schema["properties"]["payload"]["type"], "string");
        assert_eq!(schema["required"], json!(["payload"]));
        let expected = json!({"dynamic": [1, "plain string", null]});
        let wire = serde_json::to_value(Request {
            payload: expected.clone().into(),
        })
        .expect("request wire JSON");
        assert!(wire["payload"].is_string());
        let decoded: Request = serde_json::from_value(wire).expect("request decode");
        assert_eq!(decoded.payload.0, expected);
    }
}
