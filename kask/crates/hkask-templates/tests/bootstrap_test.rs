//! Bootstrap safety net — verifies `Registry::bootstrap()` loads template
//! entries from per-skill manifests at compile time.
//!
//! Catches regressions where `build.rs` fails to discover manifests, or where
//! the manifest deserialization structs diverge from the manifest YAML schema.

use hkask_types::RegistryIndex;
use std::path::Path;

#[test]
fn bootstrap_loads_templates_from_per_skill_manifests() {
    let registry = hkask_templates::Registry::bootstrap();
    let entries = registry.list(None);

    assert!(
        !entries.is_empty(),
        "bootstrap() should load at least one template from per-skill manifests"
    );

    // Every entry must have a non-empty source_path and name.
    for entry in &entries {
        assert!(
            !entry.source_path.is_empty(),
            "entry '{}' has empty source_path",
            entry.id
        );
        assert!(
            !entry.name.is_empty(),
            "entry '{}' has empty name",
            entry.id
        );
    }

    // All source_paths must resolve to real files under the workspace root.
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut missing = Vec::new();
    for entry in &entries {
        let full = workspace_root.join(&entry.source_path);
        if !full.exists() {
            missing.push(format!("{} -> {}", entry.id, entry.source_path));
        }
    }
    assert!(
        missing.is_empty(),
        "{} template source_path(s) do not exist:\n{}",
        missing.len(),
        missing.join("\n")
    );
}
