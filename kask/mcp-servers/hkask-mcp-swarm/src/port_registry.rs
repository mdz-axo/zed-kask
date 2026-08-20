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
    /// Optional JSON Schema (subset supported by `hkask_verification::schema_validate`).
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
/// grounding nulls unsourced fields — the schema must accept the cleaned
/// document, not just the pre-grounding one.
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
    /// Uses `hkask_verification::schema_validate` to check the output —
    /// the same validator used for grounding contract output validation.
    /// Unsupported schema keywords are NOT a pass (the `.rules` trap).
    pub fn validate_output(
        &self,
        produces: &[String],
        output: &serde_json::Value,
    ) -> hkask_verification::envelope::ValidationResult {
        let mut violations = Vec::new();
        let mut unsupported = Vec::new();
        let mut had_schema = false;
        for label in produces {
            let Some(schema) = self.schema_for(label) else {
                continue;
            };
            had_schema = true;
            let report = hkask_verification::schema_validate::validate(schema, output);
            if report.is_contradiction() {
                violations.extend(report.violations.iter().map(|v| SchemaViolation {
                    path: v.path.clone(),
                    message: v.message.clone(),
                }));
            }
            unsupported.extend(report.unsupported.iter().cloned());
        }
        let status = if !unsupported.is_empty() {
            ValidationStatus::UnsupportedSchema
        } else if !violations.is_empty() {
            ValidationStatus::Invalid
        } else if had_schema {
            ValidationStatus::Valid
        } else {
            ValidationStatus::NoSchema
        };
        ValidationResult {
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
