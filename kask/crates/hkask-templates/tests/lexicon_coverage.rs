//! Lexicon coverage integration test.
//!
//! Property check: every `lexicon_terms` entry declared across every registry
//! manifest must be well-formed (match `^[a-z][a-z0-9_]*$`). Catches casing
//! drift, separator drift, and whitespace before they enter the registry.
//!
//! The former allowlist check (`is_known` against a 420-term `KNOWN_TERMS`
//! array) was removed via essentialist 3-gate challenge — it was a closed
//! loop with no external consumer. The format check (`is_well_formed`) is
//! the only validation that catches real errors.
//!
//! # Principle grounding
//! - P8 (Semantic Grounding): format violations are caught before runtime
//! - P3 (Generative Space): templates use canonical naming convention

use hkask_templates::vocabulary::is_well_formed;
use serde::Deserialize;
use std::path::Path;

/// Minimal manifest shape — only the fields needed to extract `lexicon_terms`.
#[derive(Debug, Deserialize)]
struct ManifestFile {
    #[serde(default)]
    templates: Vec<TemplateEntry>,
}

#[derive(Debug, Deserialize)]
struct TemplateEntry {
    id: String,
    #[serde(default)]
    lexicon_terms: Vec<String>,
}

/// Every `lexicon_terms` entry across every registry manifest is well-formed.
#[test]
fn all_manifest_lexicon_terms_are_well_formed() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");

    let manifest_dirs = [
        workspace_root.join("registry/templates"),
        workspace_root.join("registry/manifests"),
    ];

    let mut errors = Vec::new();
    let mut manifests_checked = 0;
    let mut terms_checked = 0;

    for manifest_dir in &manifest_dirs {
        if !manifest_dir.exists() {
            eprintln!("{} not found — skipping", manifest_dir.display());
            continue;
        }

        for entry in walkdir::WalkDir::new(manifest_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "yaml"))
        {
            let path = entry.path();
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(format!("{}: IO error: {}", path.display(), e));
                    continue;
                }
            };

            // Skip pipeline configs that don't have a `templates:` key.
            if !content.contains("\ntemplates:") && !content.starts_with("templates:") {
                continue;
            }

            let manifest: ManifestFile = match serde_yaml_neo::from_str(&content) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!(
                        "{}: parse error (skipped — caught by yaml_schema_validation): {}",
                        path.display(),
                        e
                    );
                    continue;
                }
            };

            manifests_checked += 1;
            for tmpl in &manifest.templates {
                for term in &tmpl.lexicon_terms {
                    terms_checked += 1;
                    if !is_well_formed(term) {
                        errors.push(format!(
                            "{}: template '{}' declares ill-formed lexicon term '{}' (must match ^[a-z][a-z0-9_]*$)",
                            path.display(),
                            tmpl.id,
                            term
                        ));
                    }
                }
            }
        }
    }

    if !errors.is_empty() {
        panic!(
            "{} lexicon violations across {} manifests ({} terms checked):\n{}",
            errors.len(),
            manifests_checked,
            terms_checked,
            errors.join("\n")
        );
    }

    eprintln!(
        "Validated {} lexicon terms across {} manifests — all well-formed",
        terms_checked, manifests_checked
    );
}
