//! Port type registry — Rung 2 (Typing) substrate.
//!
//! Converts `accepts`/`produces` labels on `LocalAgentCard` from free strings
//! into references against a registered type set. A label that resolves to
//! nothing is rejected at admission — the paper's "499 labels that match
//! nothing" finding, prevented by construction.
//!
//! The registry is seeded from `BUILTIN_PORT_TYPES` (the labels already in
//! use by existing cards and by `build_task_agent_card` in the kata-kanban
//! server). Runtime extension is via `register_type`; file-backed loading is
//! wired through `LocalAgentRegistry`'s `port_types.json` extension file,
//! which the clone path uses to admit third-party (ABW catalogue) port
//! labels without papering over the gate for locally-authored cards.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A registered port type with an optional JSON Schema for output validation.
///
/// The schema is the paper's "one artifact, two uses": the same schema
/// that makes composition checkable (the `produces` label resolves) also
/// validates the agent's actual output at invocation time. When `schema`
/// is `None`, only label resolution is checked (the current behavior).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortTypeEntry {
    /// Optional JSON Schema (subset supported by `crate::schema_validate`).
    /// When present, the agent's output is validated against this schema
    /// at invocation time. When absent, only label resolution is checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,
}

/// Built-in port types derived from existing cards (the paper's "start with
/// what's already in use"). These are the only labels that can form a seam
/// today.
///
/// Upgrade hazard: cards using labels not in this set (e.g. `"query"`,
/// `"analysis"` from prior versions) are silently skipped on load after
/// upgrade. The `load()` warn names the rejected label; operators must
/// update the card to use a built-in label.
pub const BUILTIN_PORT_TYPES: &[&str] = &["text", "json", "task", "task_result"];

/// The schema for `task_result` outputs — the single source of truth for the
/// `task` agent contract's structured fields. Registered into the
/// `PortRegistry` at construction so both swarm and kata-kanban paths
/// validate against the same schema (the paper's "one artifact, two uses").
/// `deliverable_path` and `test_verdict` are `["string", "null"]` because
/// they may be absent — the schema must accept the document with or without
/// them.
///
/// Returned as a function (not a `const`) because `serde_json::Value` has
/// non-const destructors and cannot be constructed in a `const` context.
pub fn task_result_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "deliverable_path": { "type": ["string", "null"] },
            "test_verdict": { "type": ["string", "null"] },
            "summary": { "type": "string" },
            "approach": { "type": "string" }
        }
    })
}

/// Registered port types. A port label is a reference to a type, not a free
/// string. The registry is seeded from `BUILTIN_PORT_TYPES`; operators extend
/// it by adding labels to the built-in set (a code change) or by calling
/// `register_type` at runtime.
#[derive(Debug, Clone)]
pub struct PortRegistry {
    types: HashMap<String, PortTypeEntry>,
}

impl PortRegistry {
    /// Construct from the built-in seed. `task_result` carries the
    /// `TASK_RESULT_SCHEMA` so both swarm and kata-kanban validate `task`
    /// agent outputs against the single source of truth.
    pub fn builtin() -> Self {
        let mut types: HashMap<String, PortTypeEntry> = BUILTIN_PORT_TYPES
            .iter()
            .map(|s| ((*s).to_string(), PortTypeEntry::default()))
            .collect();
        types.insert(
            "task_result".to_string(),
            PortTypeEntry {
                schema: Some(task_result_schema()),
            },
        );
        Self { types }
    }

    /// Does the label name a registered type?
    pub fn resolves(&self, label: &str) -> bool {
        self.types.contains_key(label)
    }

    /// Get the schema for a registered type, if present.
    pub fn schema_for(&self, label: &str) -> Option<&serde_json::Value> {
        self.types
            .get(label)
            .and_then(|entry| entry.schema.as_ref())
    }

    /// Register a type with an optional schema. If the type already exists,
    /// its entry is replaced.
    pub fn register_type(&mut self, label: &str, schema: Option<serde_json::Value>) {
        self.types
            .insert(label.to_string(), PortTypeEntry { schema });
    }

    /// Merge a map of registered types into this registry (extension load).
    /// Existing entries with the same label are replaced by the incoming
    /// entry — the extension file is the newer state.
    pub fn merge_entries(&mut self, entries: &HashMap<String, PortTypeEntry>) {
        for (label, entry) in entries {
            self.types.insert(label.clone(), entry.clone());
        }
    }

    /// Validate an agent's output against the schema for its `produces` type.
    /// Returns an envelope `ValidationResult` carrying the status
    /// (`Valid` / `Invalid` / `UnsupportedSchema` / `NoSchema`), violations,
    /// and unsupported keywords — the same shape the envelope carries, so
    /// callers can populate the envelope's `validation` field directly
    /// without a lossy intermediate.
    ///
    /// When no schema is registered for any `produces` label, returns
    /// `NoSchema` (label resolution is the only check — the current
    /// behavior). When the `produces` list is empty, returns `NoSchema`.
    ///
    /// Uses `crate::schema_validate` to check the output.
    pub fn validate_output(
        &self,
        produces: &[String],
        output: &serde_json::Value,
    ) -> crate::schema_validate::StatusValidationResult {
        let mut violations = Vec::new();
        let mut unsupported = Vec::new();
        let mut had_schema = false;
        for label in produces {
            let Some(schema) = self.schema_for(label) else {
                continue;
            };
            had_schema = true;
            let report = crate::schema_validate::validate(schema, output);
            if report.is_contradiction() {
                violations.extend(report.violations.iter().map(|v| {
                    crate::schema_validate::SchemaViolation {
                        path: v.path.clone(),
                        message: v.message.clone(),
                    }
                }));
            }
            unsupported.extend(report.unsupported.iter().cloned());
        }
        let status = if !unsupported.is_empty() {
            crate::schema_validate::ValidationStatus::UnsupportedSchema
        } else if !violations.is_empty() {
            crate::schema_validate::ValidationStatus::Invalid
        } else if had_schema {
            crate::schema_validate::ValidationStatus::Valid
        } else {
            crate::schema_validate::ValidationStatus::NoSchema
        };
        crate::schema_validate::StatusValidationResult {
            status,
            violations,
            unsupported,
        }
    }

    /// Number of registered types.
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Is the registry empty?
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}

impl Default for PortRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema_validate::{ValidationStatus, validate};
    use serde_json::json;

    // ── schema_validate: the 7-keyword contract ─────────────────────
    // These pin the validator's three-outcome discipline: `valid` means
    // checked-and-conforming; an unsupported keyword is NEVER a pass.

    #[test]
    fn valid_document_passes_with_no_findings() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "count": {"type": "integer"}
            },
            "required": ["name"]
        });
        let doc = json!({"name": "scout", "count": 3});
        let result = validate(&schema, &doc);
        assert!(result.is_valid(), "expected valid, got {result:?}");
    }

    #[test]
    fn type_mismatch_is_a_violation_not_unsupported() {
        let schema = json!({"type": "string"});
        let result = validate(&schema, &json!(42));
        assert!(result.is_contradiction());
        assert!(result.unsupported.is_empty());
    }

    #[test]
    fn missing_required_field_is_flagged_at_root_path() {
        let schema = json!({"type": "object", "required": ["summary"]});
        let result = validate(&schema, &json!({}));
        assert_eq!(result.violations.len(), 1);
        assert_eq!(result.violations[0].path, "");
        assert!(result.violations[0].message.contains("summary"));
    }

    #[test]
    fn enum_rejects_disallowed_value() {
        let schema = json!({"enum": ["red", "green"]});
        let result = validate(&schema, &json!("blue"));
        assert!(result.is_contradiction());
        assert!(validate(&schema, &json!("red")).is_valid());
    }

    #[test]
    fn const_mismatch_is_flagged() {
        let schema = json!({"const": "task_result"});
        assert!(validate(&schema, &json!("task_result")).is_valid());
        assert!(validate(&schema, &json!("other")).is_contradiction());
    }

    #[test]
    fn items_schema_validates_array_elements_with_indexed_paths() {
        let schema = json!({"type": "array", "items": {"type": "integer"}});
        let result = validate(&schema, &json!([1, "two", 3]));
        assert_eq!(result.violations.len(), 1);
        assert_eq!(result.violations[0].path, "[1]");
    }

    #[test]
    fn nested_properties_produce_dotted_paths() {
        let schema = json!({
            "type": "object",
            "properties": {
                "outer": {"properties": {"inner": {"const": 7}}}
            }
        });
        let result = validate(&schema, &json!({"outer": {"inner": 8}}));
        assert_eq!(result.violations[0].path, "outer.inner");
    }

    #[test]
    fn one_of_exactly_one_match_required() {
        let schema = json!({
            "oneOf": [
                {"type": "string"},
                {"type": "integer"}
            ]
        });
        assert!(validate(&schema, &json!("s")).is_valid());
        assert!(validate(&schema, &json!(5)).is_valid());
        // An array matches neither alternative → 0 matches → violation.
        assert!(validate(&schema, &json!([1])).is_contradiction());
    }

    #[test]
    fn unsupported_keyword_is_never_a_pass() {
        // The load-bearing property: a keyword the validator cannot interpret
        // must surface as `unsupported`, making `is_valid()` false — even
        // though the document itself looks conforming.
        let schema = json!({"type": "object", "minProperties": 2});
        let doc = json!({"only": "one"});
        let result = validate(&schema, &doc);
        assert!(!result.is_valid(), "unsupported keyword silently passed");
        assert!(!result.is_contradiction());
        assert!(
            result
                .unsupported
                .iter()
                .any(|u| u.contains("minProperties"))
        );
    }

    #[test]
    fn unsupported_keyword_inside_non_matching_one_of_still_surfaces() {
        // The scan-before-early-return invariant: the type-mismatch return in
        // alternative A must not swallow alternative B's unsupported keyword.
        let schema = json!({
            "oneOf": [
                {"type": "string", "minLength": 1},
                {"type": "object", "patternProperties": {}}
            ]
        });
        let result = validate(&schema, &json!([1]));
        assert!(result.is_contradiction());
        assert!(
            result
                .unsupported
                .iter()
                .any(|u| u.contains("patternProperties")),
            "unsupported keyword in non-matching sibling was swallowed: {result:?}"
        );
    }

    #[test]
    fn integer_type_accepts_whole_numbers_only() {
        let schema = json!({"type": "integer"});
        assert!(validate(&schema, &json!(5)).is_valid());
        assert!(validate(&schema, &json!(5.5)).is_contradiction());
    }

    // ── PortRegistry: the admission gate ────────────────────────────

    #[test]
    fn builtin_registry_resolves_all_builtin_labels() {
        let registry = PortRegistry::builtin();
        for label in BUILTIN_PORT_TYPES {
            assert!(registry.resolves(label), "builtin label `{label}` missing");
        }
        assert!(
            !registry.resolves("query"),
            "legacy free-string label must not resolve"
        );
        assert!(!registry.resolves("analysis"));
    }

    #[test]
    fn task_result_type_carries_the_shared_schema() {
        let registry = PortRegistry::builtin();
        assert!(registry.schema_for("task_result").is_some());
        assert!(registry.schema_for("text").is_none());
    }

    #[test]
    fn register_type_extends_and_replaces() {
        let mut registry = PortRegistry::builtin();
        registry.register_type("custom_label", None);
        assert!(registry.resolves("custom_label"));

        registry.register_type("custom_label", Some(json!({"type": "object"})));
        assert!(registry.schema_for("custom_label").is_some());
    }

    #[test]
    fn validate_output_no_schema_when_labels_have_none() {
        let registry = PortRegistry::builtin();
        let status = registry
            .validate_output(&["text".to_string()], &json!({"anything": true}))
            .status;
        assert_eq!(status, ValidationStatus::NoSchema);
    }

    #[test]
    fn validate_output_no_schema_when_produces_empty() {
        let registry = PortRegistry::builtin();
        let status = registry.validate_output(&[], &json!({})).status;
        assert_eq!(status, ValidationStatus::NoSchema);
    }

    #[test]
    fn validate_output_valid_against_task_result_schema() {
        let registry = PortRegistry::builtin();
        let output = json!({
            "deliverable_path": "/tmp/out.md",
            "test_verdict": null,
            "summary": "done",
            "approach": "direct"
        });
        let result = registry.validate_output(&["task_result".to_string()], &output);
        assert_eq!(result.status, ValidationStatus::Valid, "got {result:?}");
    }

    #[test]
    fn validate_output_invalid_wrong_type_in_task_result() {
        let registry = PortRegistry::builtin();
        let output = json!({"summary": 42});
        let result = registry.validate_output(&["task_result".to_string()], &output);
        assert_eq!(result.status, ValidationStatus::Invalid);
        assert!(result.violations.iter().any(|v| v.path == "summary"));
    }

    #[test]
    fn validate_output_task_result_accepts_absent_optional_fields() {
        // deliverable_path/test_verdict may be absent — the schema must accept
        // the document with or without them (the "may be absent" contract).
        let registry = PortRegistry::builtin();
        let output = json!({"summary": "done", "approach": "direct"});
        let result = registry.validate_output(&["task_result".to_string()], &output);
        assert_eq!(result.status, ValidationStatus::Valid, "got {result:?}");
    }

    #[test]
    fn validate_output_unsupported_keyword_yields_unsupported_status() {
        let mut registry = PortRegistry::builtin();
        registry.register_type(
            "strict",
            Some(json!({"type": "object", "additionalProperties": false})),
        );
        let result = registry.validate_output(&["strict".to_string()], &json!({"a": 1}));
        assert_eq!(result.status, ValidationStatus::UnsupportedSchema);
        assert!(!result.unsupported.is_empty());
    }
}
