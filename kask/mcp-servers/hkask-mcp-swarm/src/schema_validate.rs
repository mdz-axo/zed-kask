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

    // ── unsupported keywords ────────────────────────────────────────
    // Scan first, before any early returns (e.g. type mismatch). An
    // unsupported keyword in a non-matching oneOf alternative must still
    // surface — "unsupported is NOT a pass." If this ran at the bottom,
    // the type-mismatch `return` would skip it and the keyword would be
    // silently ignored.
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
            // Propagate unsupported keywords from every alternative — an
            // unsupported keyword in a non-matching sibling is still an
            // unchecked keyword, and "unsupported is NOT a pass."
            result
                .unsupported
                .extend(sub_result.unsupported.iter().cloned());
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

/// Five-valued validation status. `UnsupportedSchema`/`NoSchema` are never a
/// pass — a consumer that treats them as one has reintroduced the defect
/// this distinction exists to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Valid,
    Invalid,
    UnsupportedSchema,
    NoSchema,
}

/// One schema violation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchemaViolation {
    pub path: String,
    pub message: String,
}

/// Schema validation result with status.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StatusValidationResult {
    pub status: ValidationStatus,
    pub violations: Vec<SchemaViolation>,
    pub unsupported: Vec<String>,
}
