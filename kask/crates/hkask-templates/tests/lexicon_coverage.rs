//! Lexicon coverage integration test.
//!
//! Property check: every `lexicon_terms` entry declared across every registry
//! manifest must be present in `vocabulary::KNOWN_TERMS`. Catches drift when
//! a manifest adds a term but the vocabulary list isn't updated, and catches
//! the inverse interaction where fixing a manifest parse error exposes new
//! terms to the validator that were previously hidden.
//!
//! # Principle grounding
//! - P8 (Semantic Grounding): vocabulary drift is caught before runtime
//! - P3 (Generative Space): templates are discoverable via canonical vocabulary
//!
//! # Why an integration test (not a unit test)
//! `KNOWN_TERMS` is private to `vocabulary`. `vocabulary::is_known` is the
//! public probe. This test walks the registry on disk (same pattern as
//! `yaml_schema_validation.rs`) and checks each declared term via `is_known`.
//! It also enforces the naming convention via `is_well_formed`.

use hkask_templates::vocabulary::{is_known, is_well_formed};
use serde::Deserialize;
use std::path::Path;

/// Minimal manifest shape — only the fields needed to extract `lexicon_terms`.
///
/// Mirrors `registry::SkillTemplateManifest` (which is private). Kept minimal
/// so it doesn't drift on unrelated schema changes.
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

/// Every `lexicon_terms` entry across every registry manifest is known and
/// well-formed. This is the property check that would have caught the 12
/// missing terms in the initial vocabulary fix.
#[test]
fn all_manifest_lexicon_terms_are_known_and_well_formed() {
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
            // FlowDef manifests use `steps:`, not `templates:` — they don't
            // declare lexicon_terms at the manifest level.
            if !content.contains("\ntemplates:") && !content.starts_with("templates:") {
                continue;
            }

            let manifest: ManifestFile = match serde_yaml_neo::from_str(&content) {
                Ok(m) => m,
                Err(e) => {
                    // Parse failures are caught by `all_skill_manifests_are_well_formed`
                    // in yaml_schema_validation.rs — don't duplicate that here.
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
                    if !is_known(term) {
                        errors.push(format!(
                            "{}: template '{}' declares unknown lexicon term '{}'",
                            path.display(),
                            tmpl.id,
                            term
                        ));
                    }
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
        "Validated {} lexicon terms across {} manifests — all known and well-formed",
        terms_checked, manifests_checked
    );
}
