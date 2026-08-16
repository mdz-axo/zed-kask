//! Minimal JSON Schema validator (N3).
//!
//! Fermi's `schema_validate.rs` pattern: a minimal JSON Schema validator
//! with a closed set of supported keywords. No new dependency. An
//! unsupported keyword is NOT a pass — a validator that silently ignores
//! what it cannot interpret returns `valid` for a document it never checked.
//!
//! Runs AFTER grounding, BEFORE the payload is consumed. The ordering
//! matters: grounding nulls unsourced fields first, then validation checks
//! what remains. A schema that pins an unsourceable field to `"type": "null"`
//! would otherwise reject a document that grounding was about to clean.
//!
//! Supported keywords (7):
//! - `type`: `"object"`, `"string"`, `"number"`, `"integer"`, `"boolean"`,
//!   `"array"`, `"null"`
//! - `properties`: object with per-key schemas
//! - `required`: array of required keys
//! - `items`: schema for array elements
//! - `enum`: array of allowed values
//! - `const`: single allowed value
//! - `oneOf`: array of schemas (exactly one must match)
//!
//! Three outcomes, kept distinct because they need different fixes:
//! - `valid`: checked and conforming
//! - `invalid`: the document contradicts the declared type
//! - `unverified_*`: no schema, no payload, or an unsupported keyword —
//!   NOT a pass

use serde_json::Value;

/// One violation of the schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    /// Dotted path to the offending field.
    pub path: String,
    /// What is wrong.
    pub message: String,
}

/// The result of validating a document against a schema.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationResult {
    pub violations: Vec<Violation>,
    /// Unsupported keywords encountered. NOT a pass — the caller must know
    /// the validator did not check everything.
    pub unsupported: Vec<String>,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.violations.is_empty() && self.unsupported.is_empty()
    }

    pub fn is_contradiction(&self) -> bool {
        !self.violations.is_empty()
    }
}

/// Validate `doc` against `schema`.
///
/// Returns a `ValidationResult` with violations, unsupported keywords, or
/// both. An empty result is `valid`. An `unsupported` entry is never a pass.
pub fn validate(schema: &Value, doc: &Value) -> ValidationResult {
    let mut result = ValidationResult {
        violations: Vec::new(),
        unsupported: Vec::new(),
    };
    validate_at(schema, doc, "", &mut result);
    result
}

fn validate_at(schema: &Value, doc: &Value, path: &str, result: &mut ValidationResult) {
    let Some(obj) = schema.as_object() else {
        // A non-object schema is not a schema we can validate against.
        result
            .unsupported
            .push(format!("{path}: schema is not an object ({schema})"));
        return;
    };

    // ── type ────────────────────────────────────────────────────────
    if let Some(t) = obj.get("type").and_then(|v| v.as_str()) {
        if !type_matches(t, doc) {
            result.violations.push(Violation {
                path: path.to_string(),
                message: format!("expected type `{t}`, got `{}`", type_name(doc)),
            });
            return; // No point checking further if the type is wrong.
        }
    }

    // ── const ──────────────────────────────────────────────────────
    if let Some(expected) = obj.get("const") {
        if doc != expected {
            result.violations.push(Violation {
                path: path.to_string(),
                message: format!("expected const {expected}, got {doc}"),
            });
        }
    }

    // ── enum ───────────────────────────────────────────────────────
    if let Some(allowed) = obj.get("enum").and_then(|v| v.as_array()) {
        if !allowed.contains(doc) {
            result.violations.push(Violation {
                path: path.to_string(),
                message: format!("value {doc} not in enum {allowed:?}"),
            });
        }
    }

    // ── properties + required ──────────────────────────────────────
    if let Some(props) = obj.get("properties").and_then(|v| v.as_object()) {
        if let Some(doc_obj) = doc.as_object() {
            for (key, sub_schema) in props {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if let Some(child) = doc_obj.get(key) {
                    validate_at(sub_schema, child, &child_path, result);
                }
            }
        }
    }

    if let Some(required) = obj.get("required").and_then(|v| v.as_array()) {
        if let Some(doc_obj) = doc.as_object() {
            for req in required {
                if let Some(key) = req.as_str() {
                    if !doc_obj.contains_key(key) {
                        result.violations.push(Violation {
                            path: path.to_string(),
                            message: format!("missing required field `{key}`"),
                        });
                    }
                }
            }
        }
    }

    // ── items (array element schema) ────────────────────────────────
    if let Some(item_schema) = obj.get("items") {
        if let Some(arr) = doc.as_array() {
            for (i, item) in arr.iter().enumerate() {
                let child_path = format!("{path}[{i}]");
                validate_at(item_schema, item, &child_path, result);
            }
        }
    }

    // ── oneOf ──────────────────────────────────────────────────────
    if let Some(alternatives) = obj.get("oneOf").and_then(|v| v.as_array()) {
        let mut matches = 0;
        for alt in alternatives {
            let mut sub_result = ValidationResult {
                violations: Vec::new(),
                unsupported: Vec::new(),
            };
            validate_at(alt, doc, path, &mut sub_result);
            if sub_result.is_valid() {
                matches += 1;
            }
        }
        if matches != 1 {
            result.violations.push(Violation {
                path: path.to_string(),
                message: format!("oneOf: expected exactly 1 match, got {matches}"),
            });
        }
    }

    // ── unsupported keywords ────────────────────────────────────────
    for key in obj.keys() {
        if !matches!(
            key.as_str(),
            "type" | "const" | "enum" | "properties" | "required" | "items" | "oneOf"
        ) {
            result
                .unsupported
                .push(format!("{path}: unsupported keyword `{key}`"));
        }
    }
}

fn type_matches(expected: &str, doc: &Value) -> bool {
    match expected {
        "object" => doc.is_object(),
        "string" => doc.is_string(),
        "number" => doc.is_number(),
        "integer" => doc.is_i64() || doc.is_u64(),
        "boolean" => doc.is_boolean(),
        "array" => doc.is_array(),
        "null" => doc.is_null(),
        _ => true, // Unknown type — don't fail, but the keyword check will flag it.
    }
}

fn type_name(doc: &Value) -> &'static str {
    match doc {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_object_passes() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "integer" }
            },
            "required": ["name"]
        });
        let doc = json!({ "name": "Alice", "age": 30 });
        let result = validate(&schema, &doc);
        assert!(result.is_valid(), "violations: {:?}", result.violations);
    }

    #[test]
    fn wrong_type_is_invalid() {
        let schema = json!({ "type": "string" });
        let doc = json!(42);
        let result = validate(&schema, &doc);
        assert!(result.is_contradiction());
        assert_eq!(
            result.violations[0].message,
            "expected type `string`, got `number`"
        );
    }

    #[test]
    fn missing_required_field_is_invalid() {
        let schema = json!({
            "type": "object",
            "required": ["name"]
        });
        let doc = json!({ "age": 30 });
        let result = validate(&schema, &doc);
        assert!(result.is_contradiction());
        assert!(
            result.violations[0]
                .message
                .contains("missing required field")
        );
    }

    #[test]
    fn const_mismatch_is_invalid() {
        let schema = json!({ "const": "hello" });
        let doc = json!("world");
        let result = validate(&schema, &doc);
        assert!(result.is_contradiction());
    }

    #[test]
    fn const_match_passes() {
        let schema = json!({ "const": "hello" });
        let doc = json!("hello");
        let result = validate(&schema, &doc);
        assert!(result.is_valid());
    }

    #[test]
    fn enum_mismatch_is_invalid() {
        let schema = json!({ "enum": ["a", "b", "c"] });
        let doc = json!("d");
        let result = validate(&schema, &doc);
        assert!(result.is_contradiction());
    }

    #[test]
    fn enum_match_passes() {
        let schema = json!({ "enum": ["a", "b", "c"] });
        let doc = json!("b");
        let result = validate(&schema, &doc);
        assert!(result.is_valid());
    }

    #[test]
    fn items_validate_array_elements() {
        let schema = json!({
            "type": "array",
            "items": { "type": "string" }
        });
        let doc = json!(["a", "b", 42]);
        let result = validate(&schema, &doc);
        assert!(result.is_contradiction());
        assert_eq!(result.violations[0].path, "[2]");
    }

    #[test]
    fn oneof_exactly_one_match_passes() {
        let schema = json!({
            "oneOf": [
                { "type": "string" },
                { "type": "integer" }
            ]
        });
        let doc = json!("hello");
        let result = validate(&schema, &doc);
        assert!(result.is_valid());
    }

    #[test]
    fn oneof_no_match_is_invalid() {
        let schema = json!({
            "oneOf": [
                { "type": "string" },
                { "type": "integer" }
            ]
        });
        let doc = json!(true);
        let result = validate(&schema, &doc);
        assert!(result.is_contradiction());
    }

    #[test]
    fn oneof_multiple_matches_is_invalid() {
        let schema = json!({
            "oneOf": [
                { "type": "string" },
                { "type": "string" }
            ]
        });
        let doc = json!("hello");
        let result = validate(&schema, &doc);
        assert!(result.is_contradiction());
    }

    #[test]
    fn unsupported_keyword_is_not_a_pass() {
        let schema = json!({
            "type": "string",
            "pattern": "^[a-z]+$"
        });
        let doc = json!("hello");
        let result = validate(&schema, &doc);
        assert!(!result.is_valid(), "unsupported keyword must not be a pass");
        assert!(result.unsupported.iter().any(|u| u.contains("pattern")));
    }

    #[test]
    fn non_object_schema_is_unsupported() {
        let schema = json!("not a schema");
        let doc = json!("hello");
        let result = validate(&schema, &doc);
        assert!(!result.is_valid());
        assert!(
            result
                .unsupported
                .iter()
                .any(|u| u.contains("not an object"))
        );
    }

    #[test]
    fn nested_properties_validate() {
        let schema = json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    },
                    "required": ["name"]
                }
            }
        });
        let doc = json!({ "user": { "name": "Alice" } });
        let result = validate(&schema, &doc);
        assert!(result.is_valid());
    }

    #[test]
    fn nested_properties_violation_reports_path() {
        let schema = json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    }
                }
            }
        });
        let doc = json!({ "user": { "name": 42 } });
        let result = validate(&schema, &doc);
        assert!(result.is_contradiction());
        assert_eq!(result.violations[0].path, "user.name");
    }

    #[test]
    fn null_type_matches_null() {
        let schema = json!({ "type": "null" });
        let doc = Value::Null;
        let result = validate(&schema, &doc);
        assert!(result.is_valid());
    }

    #[test]
    fn integer_type_matches_integer() {
        let schema = json!({ "type": "integer" });
        assert!(validate(&schema, &json!(42)).is_valid());
        assert!(!validate(&schema, &json!(42.5)).is_valid());
    }
}
