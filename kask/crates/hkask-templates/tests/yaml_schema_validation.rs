//! YAML schema validation tests — Wave 6 Task 6.1
//!
//! Validates that all registry manifest YAML files are well-formed
//! and contain required fields. Catches malformed manifests at test time
//! rather than at runtime.
//!
//! # Principle grounding
//! - P8 (Semantic Grounding): config errors should be caught before runtime
//! - P11 (Digital Public/Private Sphere): visibility must be canonical

use serde::Deserialize;
use std::path::Path;

/// Minimal manifest structure for validation.
#[derive(Debug, Deserialize)]
struct ManifestFile {
    manifest: ManifestHeader,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ManifestHeader {
    id: String,
    name: String,
    description: String,
    version: String,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    functional_role: Option<String>,
}

// [P3] Motivating: Generative Space — validates registry manifests are well-formed
//Constraining: Semantic Grounding — required fields present and correctly typed
// All registry manifests are well-formed YAML with required fields.

#[test]
fn all_skill_manifests_are_well_formed() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let manifest_dir = workspace_root.join("registry/manifests");
    if !manifest_dir.exists() {
        eprintln!("{} not found — skipping test", manifest_dir.display());
        return;
    }

    let mut errors = Vec::new();
    let mut count = 0;

    for entry in walkdir::WalkDir::new(manifest_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "yaml"))
    {
        count += 1;
        let path = entry.path();
        match std::fs::read_to_string(path) {
            Ok(content) => {
                // Skip pipeline configs (no `manifest:` key at top level).
                if !content.contains("\nmanifest:") && !content.starts_with("manifest:") {
                    eprintln!("Skipping non-manifest YAML: {}", path.display());
                    continue;
                }
                match serde_yaml_neo::from_str::<ManifestFile>(&content) {
                    Ok(mf) => {
                        assert!(
                            !mf.manifest.id.is_empty(),
                            "{}: manifest.id is empty",
                            path.display()
                        );
                        assert!(
                            !mf.manifest.name.is_empty(),
                            "{}: manifest.name is empty",
                            path.display()
                        );
                        // P3: description must be present (Generative Space requires discoverability)
                        assert!(
                            !mf.manifest.description.is_empty(),
                            "{}: manifest.description is empty",
                            path.display()
                        );
                        // P7: version must be present (Evolutionary Architecture requires versioning)
                        assert!(
                            !mf.manifest.version.is_empty(),
                            "{}: manifest.version is empty",
                            path.display()
                        );
                        // P11: visibility must be present and canonical (Public or Private only)
                        let vis = mf.manifest.visibility.as_deref().unwrap_or("");
                        assert!(
                            !vis.is_empty(),
                            "{}: manifest.visibility is missing",
                            path.display()
                        );
                        assert!(
                            vis == "Public" || vis == "Private",
                            "{}: manifest.visibility is '{vis}' — must be Public or Private (P11)",
                            path.display()
                        );
                        // functional_role should be present if the manifest uses it
                        // (Note: some manifests like kata and improv use alternative structural schemas)
                    }
                    Err(e) => {
                        errors.push(format!("{}: YAML parse error: {}", path.display(), e));
                    }
                }
            }
            Err(e) => {
                errors.push(format!("{}: IO error: {}", path.display(), e));
            }
        }
    }

    if !errors.is_empty() {
        panic!(
            "{} of {} manifests failed validation:\n{}",
            errors.len(),
            count,
            errors.join("\n")
        );
    }

    eprintln!("Validated {} manifests — all well-formed", count);
}

// [P3] Motivating: Generative Space — validates registry manifests are well-formed
//Constraining: Semantic Grounding — required fields present and correctly typed
#[test]
fn invalid_yaml_is_rejected() {
    let invalid = "id: 123\nname: []\n"; // name should be string, not array
    let result = serde_yaml_neo::from_str::<ManifestFile>(invalid);
    assert!(result.is_err(), "invalid YAML should be rejected");
}

/// Verify the superforecasting manifest loads via the full loader and that
/// its `compute` step (the connected-layers bridge to hkask_forecast) parses
/// correctly with `action: "compute"` and a valid `compute_ref`.
#[test]
fn superforecasting_manifest_loads_with_compute_step() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let manifest_path = workspace_root.join("registry/manifests/superforecasting.yaml");
    if !manifest_path.exists() {
        eprintln!("superforecasting.yaml not found — skipping");
        return;
    }
    let yaml = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = hkask_templates::load_manifest_from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("Failed to load superforecasting manifest: {e}"));

    // 12 select steps + 4 compute steps + 1 loop step = 17 total.
    assert_eq!(
        manifest.steps.len(),
        17,
        "expected 17 steps after Fermi + outside-view + Bayesian + calibration compute insertions"
    );

    // Four compute steps: Fermi (3), outside-view (5), Bayesian (10), calibration (16).
    let compute_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "compute")
        .collect();
    assert_eq!(compute_steps.len(), 4, "manifest must have 4 compute steps");
    assert_eq!(compute_steps[0].ordinal, 3, "Fermi compute at ordinal 3");
    assert_eq!(
        compute_steps[0].compute_ref.as_deref(),
        Some("calibrate_from_fermi")
    );
    assert_eq!(
        compute_steps[1].ordinal, 5,
        "outside-view compute at ordinal 5"
    );
    assert_eq!(
        compute_steps[1].compute_ref.as_deref(),
        Some("outside_view_adjustment")
    );
    assert_eq!(
        compute_steps[2].ordinal, 10,
        "Bayesian compute at ordinal 10"
    );
    assert_eq!(
        compute_steps[2].compute_ref.as_deref(),
        Some("bayesian_update")
    );
    assert_eq!(
        compute_steps[3].ordinal, 16,
        "calibration feedback compute at ordinal 16"
    );
    assert_eq!(
        compute_steps[3].compute_ref.as_deref(),
        Some("apply_calibration_adjustment")
    );

    // The loop step (ordinal 17) must carry the calibration-adjusted prior.
    let loop_step = manifest
        .steps
        .iter()
        .find(|s| s.action == "loop")
        .expect("manifest must have a loop step");
    assert_eq!(loop_step.ordinal, 17, "loop step should be ordinal 17");
}

/// Verify the kali-audit FlowDef manifest loads correctly with the expected
/// PDCA structure: 4 select steps + 1 loop step, convergence field pointing
/// at step 4, and template_refs matching the registry crate.
#[test]
fn kali_audit_manifest_loads_with_correct_structure() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let manifest_path = workspace_root.join("registry/manifests/kali-audit.yaml");
    if !manifest_path.exists() {
        eprintln!("kali-audit.yaml not found — skipping");
        return;
    }
    let yaml = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = hkask_templates::load_manifest_from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("Failed to load kali-audit manifest: {e}"));

    // 4 select steps + 1 loop step = 5 total.
    assert_eq!(
        manifest.steps.len(),
        5,
        "expected 5 steps: select-surface → audit → report → convergence-check → loop"
    );

    // Verify step ordinals are sequential starting at 1.
    for (i, step) in manifest.steps.iter().enumerate() {
        assert_eq!(
            step.ordinal,
            (i + 1) as u32,
            "step ordinals must be sequential starting at 1"
        );
    }

    // Verify step 1 is select-surface.
    assert_eq!(manifest.steps[0].action, "select");
    assert_eq!(
        manifest.steps[0].template_ref.as_deref(),
        Some("kali-audit/select-surface")
    );

    // Verify step 2 is audit.
    assert_eq!(manifest.steps[1].action, "select");
    assert_eq!(
        manifest.steps[1].template_ref.as_deref(),
        Some("kali-audit/audit")
    );

    // Verify step 3 is report.
    assert_eq!(manifest.steps[2].action, "select");
    assert_eq!(
        manifest.steps[2].template_ref.as_deref(),
        Some("kali-audit/report")
    );

    // Verify step 4 is convergence-check.
    assert_eq!(manifest.steps[3].action, "select");
    assert_eq!(
        manifest.steps[3].template_ref.as_deref(),
        Some("kali-audit/convergence-check")
    );

    // Verify step 5 is loop.
    assert_eq!(manifest.steps[4].action, "loop");

    // Verify convergence field points at step 4.
    assert_eq!(
        manifest.convergence.convergence_field,
        "step_4_result.convergence_metric"
    );

    // Verify convergence threshold is 0.10 (stricter than bug-hunt's 0.25).
    assert_eq!(
        manifest.convergence.threshold, 0.10,
        "kali-audit threshold should be 0.10 (security is higher-stakes than bug-hunting)"
    );

    // Verify max_iterations is 3.
    assert_eq!(
        manifest.convergence.max_iterations, 3,
        "max_iterations should be 3"
    );

    // Verify gas cap is positive.
    assert!(manifest.gas.cap > 0, "gas cap must be positive");

    // Verify OCAP delegation chain is required.
    assert!(
        manifest.ocap.delegation_chain_required,
        "delegation chain should be required"
    );
}
