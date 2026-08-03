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

use std::borrow::Cow;
use std::ops::Deref;

use schemars::Schema;
use schemars::SchemaGenerator;
use schemars::JsonSchema;
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

impl AnyJsonValue {
    /// Extract the wrapped [`serde_json::Value`].
    #[must_use]
    pub fn into_inner(self) -> serde_json::Value {
        self.0
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::schema_for;

    /// The schema must be a JSON object, never the bare boolean `true` that
    /// `serde_json::Value` produces. Ollama rejects boolean property schemas.
    #[test]
    fn any_json_value_schema_is_object_not_boolean() {
        let schema = schema_for!(AnyJsonValue);
        let value = serde_json::to_value(&schema).expect("schema serializes");
        assert!(
            value.is_object(),
            "AnyJsonValue schema must be a JSON object, got: {value}"
        );
        assert!(
            !value.is_boolean(),
            "AnyJsonValue schema must not be a bare boolean"
        );
    }

    /// `AnyJsonValue` serializes transparently as the inner `Value`.
    #[test]
    fn any_json_value_round_trips_as_value() {
        let inner = serde_json::json!({"answer": 42, "list": [1, 2, 3]});
        let wrapped = AnyJsonValue(inner.clone());
        let serialized = serde_json::to_value(&wrapped).expect("serialize");
        assert_eq!(serialized, inner);

        let back: AnyJsonValue = serde_json::from_value(inner.clone()).expect("deserialize");
        assert_eq!(&*back, &inner);
    }

    /// Default (omitted field) is `Value::Null`, matching `serde_json::Value`'s
    /// default so `#[serde(default)]` behavior is unchanged.
    #[test]
    fn any_json_value_default_is_null() {
        assert!(AnyJsonValue::default().is_null());
    }
}
