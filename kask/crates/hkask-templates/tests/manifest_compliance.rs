use hkask_templates::{extract_contract_input_keys, load_manifest_from_yaml};
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

/// Issue 5: cross-check each `action: select` step's `input_mapping` keys
/// against the referenced template's `contract.input` keys.
///
/// Two mismatch directions, both reported as WARNINGS (not errors) because
/// many are intentional:
///
/// - **mapping-has-not-contract** (potential typos): the manifest's
///   `input_mapping` provides a key the template's `contract.input` does not
///   declare. Often a typo or a stale mapping referencing a renamed input.
///   Actionable: fix the mapping or add the key to the contract.
/// - **contract-has-not-mapping** (template expects, mapping doesn't provide):
///   the template declares a `contract.input` key the manifest's
///   `input_mapping` does not provide. Frequently by-design — the template
///   documents inputs it consumes that the *agent* provides between steps
///   (e.g. `existing_code`, `repl_results`, `lean_diagnostics`), not via
///   `input_mapping`. Informational, not actionable in general.
///
/// This test does NOT fail on mismatches (it would break on the many
/// intentional agent-coordinated-context cases). It emits a diagnostic
/// summary so mismatches are visible in CI output and can be triaged. A
/// future tightening can promote mapping-has-not-contract to an error once
/// the intentional cases are annotated.
#[test]
fn input_mapping_matches_template_contract() {
    let manifests_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    let templates_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/templates");
    if !manifests_dir.exists() || !templates_dir.exists() {
        eprintln!(
            "registry not found (manifests={} templates={}) — skipping test",
            manifests_dir.display(),
            templates_dir.display()
        );
        return;
    }

    let mut mapping_extra: Vec<(String, u32, String, Vec<String>)> = Vec::new();
    let mut contract_missing: Vec<(String, u32, String, Vec<String>)> = Vec::new();
    let mut checked = 0u32;
    let mut skipped_no_template = 0u32;
    let mut skipped_no_contract = 0u32;

    for entry in walkdir::WalkDir::new(&manifests_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.extension().is_some_and(|e| e == "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).expect("manifest readable");
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue, // load failures caught by the load test
        };
        let fname = path
            .file_name()
            .expect("filename")
            .to_string_lossy()
            .to_string();

        for step in &manifest.steps {
            if step.action != "select" {
                continue;
            }
            let Some(template_ref) = &step.template_ref else {
                skipped_no_template += 1;
                continue;
            };
            let template_path = templates_dir.join(format!("{template_ref}.j2"));
            let Some(template_content) = std::fs::read_to_string(&template_path).ok() else {
                eprintln!(
                    "WARN: {fname} step {} references missing template {}",
                    step.ordinal,
                    template_path.display()
                );
                skipped_no_template += 1;
                continue;
            };
            let contract_keys = extract_contract_input_keys(&template_content);
            if contract_keys.is_empty() {
                skipped_no_contract += 1;
                continue;
            }
            checked += 1;

            let mapping_keys: HashSet<String> = step
                .input_mapping
                .as_ref()
                .and_then(|v| v.as_object())
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();

            let extra: Vec<String> = mapping_keys.difference(&contract_keys).cloned().collect();
            if !extra.is_empty() {
                mapping_extra.push((fname.clone(), step.ordinal, template_ref.clone(), extra));
            }

            let missing: Vec<String> = contract_keys.difference(&mapping_keys).cloned().collect();
            if !missing.is_empty() {
                contract_missing.push((fname.clone(), step.ordinal, template_ref.clone(), missing));
            }
        }
    }

    eprintln!(
        "input_mapping/contract cross-check: {checked} steps checked, \
         {} skipped (no template), {} skipped (no contract.input), \
         {} mapping-extra, {} contract-missing",
        skipped_no_template,
        skipped_no_contract,
        mapping_extra.len(),
        contract_missing.len()
    );
    eprintln!("--- mapping-has-not-contract (potential typos / stale mappings) ---");
    for (f, ord, tref, keys) in &mapping_extra {
        eprintln!("  {f} step {ord} ({tref}): {keys:?}");
    }
    eprintln!("--- contract-has-not-mapping (template expects; often agent-coordinated) ---");
    for (f, ord, tref, keys) in &contract_missing {
        eprintln!("  {f} step {ord} ({tref}): {keys:?}");
    }

    // This test is a diagnostic: it does not fail. Mismatches are surfaced in
    // CI output for triage. Promoting mapping-has-not-contract to a hard
    // failure requires first annotating the intentional cases.
    //
    // To tighten: uncomment the assert below once the mapping-extra list is
    // clean (or intentional cases are annotated in the contract).
    // assert!(
    //     mapping_extra.is_empty(),
    //     "{} mapping-extra mismatches (potential typos)",
    //     mapping_extra.len()
    // );
}
