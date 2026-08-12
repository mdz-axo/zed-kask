use hkask_templates::load_manifest_from_yaml;
use std::collections::HashSet;
use std::path::Path;

/// Canonical action set enforced by the ManifestExecutor.
const CANONICAL_ACTIONS: &[&str] = &[
    "select", "populate", "compute", "execute", "feedback", "validate", "retrieve", "render",
    "flowdef", "loop", "choice", "abort", "escalate",
];

/// Valid categories for a manifest in `registry/manifests/`.
///
/// Only `skill` is permitted. The 41 non-`skill` manifests (`pipeline`,
/// `qa-script`, `runtime-config`, `daemon-process`) were deleted after an audit
/// found no runtime consumer for any of them: `resolve_manifest` rejects
/// non-`skill` categories with `NotASkill` (`manifest_loader.rs`), and the
/// execution boundary rejects again (`kask_bridge::skill_executor`), so they
/// were embedded by `build.rs`, seeded to disk, and never read.
///
/// The `category` field itself is retained: it is the security gate for
/// `lisp.eval` and `shell.exec` (see `compute.rs`), which must run only in
/// operator-reviewed `skill` manifests. Narrowing this list to `skill` makes a
/// reintroduced infrastructure manifest fail CI rather than ship as dead
/// surface. To add a new category, first wire a loader that can execute it.
const VALID_CATEGORIES: &[&str] = &["skill"];

/// Regression test: every loadable manifest must use only canonical actions,
/// have a gas block with cap > 0, have an rjoule block when inference is used,
/// and (for skill category) have a convergence block.
///
/// This test encodes the compliance standards that the skill-maintenance skill
/// enforces at authoring/audit time. A future skill author using
/// skill-maintenance-validate would catch the same issues this test catches.
#[test]
fn all_manifests_are_executor_compliant() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let canonical: HashSet<&str> = CANONICAL_ACTIONS.iter().copied().collect();
    let valid_categories: HashSet<&str> = VALID_CATEGORIES.iter().copied().collect();

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "yaml") {
            let yaml = std::fs::read_to_string(path).unwrap();
            if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
                continue;
            }
            let manifest = match load_manifest_from_yaml(&yaml) {
                Ok(m) => m,
                Err(_e) => {
                    // Load failures are caught by all_manifests_load_successfully.
                    // Don't double-report here.
                    continue;
                }
            };
            checked += 1;
            let fname = path.file_name().unwrap().to_string_lossy();

            // 1. Category validity
            if let Some(ref cat) = manifest.category
                && !valid_categories.contains(cat.as_str())
            {
                errors.push(format!(
                    "{fname}: manifest.category='{cat}' is not valid (must be one of {VALID_CATEGORIES:?})"
                ));
            }

            // 2. Action compliance
            for step in &manifest.steps {
                if !canonical.contains(step.action.as_str()) {
                    errors.push(format!(
                        "{fname}: step {} has non-canonical action '{}'",
                        step.ordinal, step.action
                    ));
                }
            }

            // 3. Gas block
            if manifest.gas.cap == 0 {
                errors.push(format!("{fname}: gas.cap == 0 (must be > 0)"));
            }

            // 4. rJoule block — required when inference is used
            let uses_inference = manifest.steps.iter().any(|s| s.action == "select");
            if uses_inference && manifest.rjoule.cap == 0 {
                errors.push(format!(
                    "{fname}: uses inference (action: select) but rjoule.cap == 0"
                ));
            }

            // 5. Convergence block — required for skill category
            if manifest.is_skill() {
                if manifest.convergence.threshold <= 0.0 {
                    errors.push(format!(
                        "{fname}: skill manifest has convergence.threshold <= 0"
                    ));
                }
                if manifest.convergence.max_iterations == 0 {
                    errors.push(format!(
                        "{fname}: skill manifest has convergence.max_iterations == 0"
                    ));
                }
                if manifest.convergence.convergence_field.is_empty() {
                    errors.push(format!(
                        "{fname}: skill manifest has empty convergence.convergence_field"
                    ));
                }
                if manifest.convergence.on_not_reached.is_empty() {
                    errors.push(format!(
                        "{fname}: skill manifest has empty convergence.on_not_reached"
                    ));
                }
            }
        }
    }

    eprintln!(
        "Checked {checked} manifests — {} compliance errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} compliance errors found:\n{}",
        errors.len(),
        errors.join("\n")
    );
}
