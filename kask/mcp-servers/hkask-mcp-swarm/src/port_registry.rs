//! Port type registry — Rung 2 (Typing) substrate.
//!
//! Converts `accepts`/`produces` labels on `LocalAgentCard` from free strings
//! into references against a registered type set. A label that resolves to
//! nothing is rejected at admission — the paper's "499 labels that match
//! nothing" finding, prevented by construction.
//!
//! The registry is file-backed (`mcp/swarm/port_types.json`). When the file is
//! absent, the built-in seed is used and a `warn!` is emitted naming the
//! missing path — the `.rules` trap on silent fallbacks. The built-in seed
//! contains the labels already in use by existing cards and by
//! `build_task_agent_card` in the kata-kanban server.

use std::collections::HashSet;

/// Built-in port types derived from existing cards (the paper's "start with
/// what's already in use"). These are the only labels that can form a seam
/// today. The operator can extend via `mcp/swarm/port_types.json`.
pub const BUILTIN_PORT_TYPES: &[&str] = &["text", "json", "task", "task_result"];

/// Registered port types. A port label is a reference to a type, not a free
/// string. The registry is a JSON file the operator can extend
/// (`mcp/swarm/port_types.json`). When the file is absent, the built-in seed
/// is used.
pub struct PortRegistry {
    types: HashSet<String>,
}

impl PortRegistry {
    /// Construct from the built-in seed. Used when no file path is provided
    /// or the file is absent.
    pub fn builtin() -> Self {
        Self {
            types: BUILTIN_PORT_TYPES.iter().map(|s| (*s).to_string()).collect(),
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
                    let types: HashSet<String> = labels.into_iter().collect();
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
        self.types.contains(label)
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
        assert!(!registry.resolves("task_result"), "task_result not in custom file");

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
        assert!(registry.resolves("text"), "empty file falls back to built-in");

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
        assert!(registry.resolves("text"), "unparseable file falls back to built-in");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
