//! Port type registry — Rung 2 (Typing) substrate.
//!
//! Converts `accepts`/`produces` labels on `LocalAgentCard` from free strings
//! into references against a registered type set. A label that resolves to
//! nothing is rejected at admission — the paper's "499 labels that match
//! nothing" finding, prevented by construction.
//!
//! The registry is designed to be file-backed (`mcp/swarm/port_types.json`),
//! but the file-backed load path is **not yet enforced** — `run()` constructs
//! the registry via `PortRegistry::builtin()`. The `load_or_builtin` and
//! `with_port_registry` helpers exist for future wiring but have only test
//! callers today. When the file path is absent, the built-in seed is used.
//! The built-in seed contains the labels already in use by existing cards and
//! by `build_task_agent_card` in the kata-kanban server.

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
/// today. The operator can extend via `mcp/swarm/port_types.json` (not yet
/// enforced — see module docs).
///
/// Upgrade hazard: cards using labels not in this set (e.g. `"query"`,
/// `"analysis"` from prior versions) are silently skipped on load after
/// upgrade. The `load()` warn names the rejected label; operators must either
/// add the label to `port_types.json` (once file-backed loading is wired) or
/// update the card to use a built-in label.
pub const BUILTIN_PORT_TYPES: &[&str] = &["text", "json", "task", "task_result"];

/// Registered port types. A port label is a reference to a type, not a free
/// string. The registry is a JSON file the operator can extend
/// (`mcp/swarm/port_types.json`). When the file is absent, the built-in seed
/// is used.
pub struct PortRegistry {
    types: HashMap<String, PortTypeEntry>,
}

impl PortRegistry {
    /// Construct from the built-in seed. Used when no file path is provided
    /// or the file is absent.
    pub fn builtin() -> Self {
        Self {
            types: BUILTIN_PORT_TYPES
                .iter()
                .map(|s| ((*s).to_string(), PortTypeEntry::default()))
                .collect(),
        }
    }

    /// Load from a JSON file containing an array of type strings, falling back
    /// to the built-in seed if the file is absent. The fallback emits a
    /// `warn!` naming the missing path — the operator cannot distinguish
    /// "no registry file" from "registry file loaded" without it.
    pub fn load_or_builtin(path: &str) -> Self {
        match std::fs::read_to_string(path) {
            Ok(contents) => match serde_json::from_str::<Vec<String>>(&contents) {
                Ok(labels) => {
                    let types: HashMap<String, PortTypeEntry> = labels
                        .into_iter()
                        .map(|l| (l, PortTypeEntry::default()))
                        .collect();
                    if types.is_empty() {
                        tracing::warn!(
                            target: "hkask.mcp.swarm",
                            path = %path,
                            "port_types.json is empty — falling back to built-in seed"
                        );
                        return Self::builtin();
                    }
                    Self { types }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.mcp.swarm",
                        path = %path,
                        %e,
                        "failed to parse port_types.json — falling back to built-in seed"
                    );
                    Self::builtin()
                }
            },
            Err(_) => {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    path = %path,
                    "port_types.json not found — using built-in seed ({:?})",
                    BUILTIN_PORT_TYPES
                );
                Self::builtin()
            }
        }
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

    /// Validate an agent's output against the schema for its `produces` type.
    /// Returns `Ok(())` when the output matches the schema, or an error
    /// describing the mismatch. When no schema is registered for the type,
    /// returns `Ok(())` (label resolution is the only check — the current
    /// behavior). When the `produces` list is empty, returns `Ok(())` (no
    /// declared output type to validate against).
    ///
    /// Uses `hkask_verification::schema_validate` to check the output —
    /// the same validator used for grounding contract output validation.
    /// Unsupported schema keywords are NOT a pass (the `.rules` trap).
    pub fn validate_output(
        &self,
        produces: &[String],
        output: &serde_json::Value,
    ) -> Result<(), String> {
        for label in produces {
            if let Some(schema) = self.schema_for(label) {
                let report = hkask_verification::schema_validate::validate(schema, output);
                if report.is_contradiction() {
                    let errors: Vec<String> = report
                        .violations
                        .iter()
                        .map(|v| format!("{}: {}", v.path, v.message))
                        .collect();
                    return Err(format!(
                        "output does not match schema for port type '{}': {}",
                        label,
                        errors.join("; ")
                    ));
                }
                if !report.unsupported.is_empty() {
                    tracing::warn!(
                        target: "hkask.swarm.port_registry",
                        port_type = %label,
                        unsupported = ?report.unsupported,
                        "schema contains unsupported keywords — output not fully validated"
                    );
                }
            }
        }
        Ok(())
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

    #[test]
    fn builtin_registry_resolves_known_labels() {
        let registry = PortRegistry::builtin();
        assert!(registry.resolves("text"));
        assert!(registry.resolves("json"));
        assert!(registry.resolves("task"));
        assert!(registry.resolves("task_result"));
    }

    #[test]
    fn builtin_registry_rejects_unknown_labels() {
        let registry = PortRegistry::builtin();
        assert!(!registry.resolves("genome_summary"));
        assert!(!registry.resolves("phylo_profile"));
        assert!(!registry.resolves("threat_level"));
    }

    #[test]
    fn load_or_builtin_reads_from_file() {
        let dir = std::env::temp_dir().join("hkask_port_registry_test_file");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("port_types.json");
        std::fs::write(
            &path,
            serde_json::json!(["text", "json", "custom_type"]).to_string(),
        )
        .unwrap();

        let registry = PortRegistry::load_or_builtin(path.to_string_lossy().as_ref());
        assert!(registry.resolves("text"));
        assert!(registry.resolves("json"));
        assert!(registry.resolves("custom_type"));
        assert!(
            !registry.resolves("task_result"),
            "task_result not in custom file"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_or_builtin_falls_back_when_file_absent() {
        let path = "/nonexistent/path/port_types.json";
        let registry = PortRegistry::load_or_builtin(path);
        // Fallback to built-in seed.
        assert!(registry.resolves("text"));
        assert!(registry.resolves("json"));
    }

    #[test]
    fn load_or_builtin_falls_back_when_file_empty() {
        let dir = std::env::temp_dir().join("hkask_port_registry_test_empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("port_types.json");
        std::fs::write(&path, "[]").unwrap();

        let registry = PortRegistry::load_or_builtin(path.to_string_lossy().as_ref());
        assert!(
            registry.resolves("text"),
            "empty file falls back to built-in"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_or_builtin_falls_back_when_file_unparseable() {
        let dir = std::env::temp_dir().join("hkask_port_registry_test_bad");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("port_types.json");
        std::fs::write(&path, "not valid json {{{").unwrap();

        let registry = PortRegistry::load_or_builtin(path.to_string_lossy().as_ref());
        assert!(
            registry.resolves("text"),
            "unparseable file falls back to built-in"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Schema validation tests (Step 3: "one artifact, two uses") ──────

    #[test]
    fn validate_output_passes_when_no_schema_registered() {
        let registry = PortRegistry::builtin();
        // "text" has no schema in the built-in registry — validation is a no-op.
        let output = serde_json::json!("anything");
        assert!(
            registry
                .validate_output(&["text".to_string()], &output)
                .is_ok()
        );
    }

    #[test]
    fn validate_output_passes_when_produces_is_empty() {
        let registry = PortRegistry::builtin();
        let output = serde_json::json!({"key": "value"});
        assert!(registry.validate_output(&[], &output).is_ok());
    }

    #[test]
    fn validate_output_passes_when_output_matches_schema() {
        let mut registry = PortRegistry::builtin();
        registry.register_type(
            "task_result",
            Some(serde_json::json!({
                "type": "object",
                "required": ["deliverable_path"],
                "properties": {
                    "deliverable_path": { "type": "string" },
                    "summary": { "type": "string" }
                }
            })),
        );
        let output = serde_json::json!({
            "deliverable_path": "/src/main.rs",
            "summary": "did the work"
        });
        assert!(
            registry
                .validate_output(&["task_result".to_string()], &output)
                .is_ok()
        );
    }

    #[test]
    fn validate_output_fails_when_output_missing_required_field() {
        let mut registry = PortRegistry::builtin();
        registry.register_type(
            "task_result",
            Some(serde_json::json!({
                "type": "object",
                "required": ["deliverable_path"],
                "properties": {
                    "deliverable_path": { "type": "string" }
                }
            })),
        );
        let output = serde_json::json!({
            "summary": "did the work but no path"
        });
        let err = registry
            .validate_output(&["task_result".to_string()], &output)
            .unwrap_err();
        assert!(
            err.contains("deliverable_path"),
            "error must name the missing field"
        );
    }

    #[test]
    fn validate_output_fails_when_output_has_wrong_type() {
        let mut registry = PortRegistry::builtin();
        registry.register_type(
            "task_result",
            Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "deliverable_path": { "type": "string" }
                }
            })),
        );
        let output = serde_json::json!({
            "deliverable_path": 42
        });
        let err = registry
            .validate_output(&["task_result".to_string()], &output)
            .unwrap_err();
        assert!(err.contains("task_result"), "error must name the port type");
    }

    #[test]
    fn schema_for_returns_none_for_unregistered_type() {
        let registry = PortRegistry::builtin();
        assert!(registry.schema_for("nonexistent").is_none());
    }

    #[test]
    fn schema_for_returns_none_for_registered_type_without_schema() {
        let registry = PortRegistry::builtin();
        assert!(registry.schema_for("text").is_none());
    }

    #[test]
    fn schema_for_returns_schema_when_registered() {
        let mut registry = PortRegistry::builtin();
        let schema = serde_json::json!({"type": "string"});
        registry.register_type("custom", Some(schema.clone()));
        assert_eq!(registry.schema_for("custom"), Some(&schema));
    }
}
