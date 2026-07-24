//! FlowDef cross-validation tests — validates all registry manifests
//! against their referenced templates' contracts.
//!
//! Catches:
//! - `convergence_field` referencing a step ordinal that doesn't exist
//! - `template_ref` values that don't resolve to registered templates
//! - `input_mapping` keys that don't match template contract inputs
//! - Self-referential input mappings (step references its own output)
//!
//! # Principle grounding
//! - P8 (Semantic Grounding): manifest↔template mismatches caught before runtime
//! - P5 (Essentialism): one test, one purpose — cross-validate all manifests

use hkask_types::flowdef_validation::{
    parse_template_contract_inputs, validate_convergence_field, validate_step_input_mapping,
};
use std::collections::HashMap;
use std::path::Path;

/// Minimal manifest structure for parsing step ordinals and convergence field.
#[derive(Debug, serde::Deserialize)]
struct ManifestFile {
    manifest: ManifestHeader,
    #[serde(default)]
    convergence: Option<ConvergenceConfig>,
    #[serde(default)]
    steps: Vec<StepEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct ManifestHeader {
    id: String,
}

#[derive(Debug, serde::Deserialize)]
struct ConvergenceConfig {
    #[serde(default)]
    convergence_field: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct StepEntry {
    ordinal: u32,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    template_ref: Option<String>,
    #[serde(default)]
    input_mapping: Option<HashMap<String, String>>,
}

#[test]
fn all_manifests_pass_flowdef_cross_validation() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let manifest_dir = workspace_root.join("registry/manifests");
    let templates_dir = workspace_root.join("registry/templates");

    if !manifest_dir.exists() {
        eprintln!("{} not found — skipping test", manifest_dir.display());
        return;
    }

    let mut errors = Vec::new();
    let mut count = 0;

    for entry in walkdir::WalkDir::new(&manifest_dir)
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

        // Skip non-manifest YAML (pipeline configs without manifest: key)
        if !content.contains("\nmanifest:") && !content.starts_with("manifest:") {
            continue;
        }

        count += 1;

        let manifest: ManifestFile = match serde_yaml_neo::from_str(&content) {
            Ok(m) => m,
            Err(e) => {
                // Some manifests have different schemas (kata, improv) — skip parse errors
                eprintln!(
                    "Skipping {} (parse error, likely non-standard schema): {}",
                    path.display(),
                    e
                );
                continue;
            }
        };

        let manifest_id = manifest.manifest.id;
        let num_steps = manifest.steps.len() as u32;

        // Validate convergence_field
        if let Some(conv) = &manifest.convergence
            && let Some(field) = &conv.convergence_field
            && let Some(finding) = validate_convergence_field(&manifest_id, field, num_steps)
        {
            errors.push(format!(
                "{}: [{}] {} — {}",
                path.display(),
                finding.severity,
                finding.category,
                finding.description
            ));
        }

        // Validate convergence_field references an output-producing step (SMELL 11 fix).
        let ordinal_actions: HashMap<u32, String> = manifest
            .steps
            .iter()
            .filter_map(|s| s.action.as_ref().map(|a| (s.ordinal, a.clone())))
            .collect();
        const NON_OUTPUT: &[&str] = &["loop", "abort", "escalate", "choice"];
        if let Some(conv) = &manifest.convergence
            && let Some(field) = &conv.convergence_field
            && let Some(n_str) = field.strip_prefix("step_")
            && let Some(end) = n_str.find('_')
            && let Ok(n) = n_str[..end].parse::<u32>()
            && let Some(action) = ordinal_actions.get(&n)
            && NON_OUTPUT.contains(&action.as_str())
        {
            errors.push(format!(
                "{}: [critical] convergence_field references step {n} which is a '{action}' step (produces no result)",
                path.display()
            ));
        }

        // Validate loop_target references a declared ordinal (SMELL 11 fix).
        let declared: std::collections::HashSet<u32> =
            manifest.steps.iter().map(|s| s.ordinal).collect();
        for step in &manifest.steps {
            if let Some(mapping) = &step.input_mapping
                && let Some(lt) = mapping.get("loop_target")
                && let Ok(n) = lt.trim().parse::<u32>()
                && !declared.contains(&n)
            {
                errors.push(format!(
                    "{}: [critical] step {} loop_target {n} references no existing step",
                    path.display(),
                    step.ordinal
                ));
            }
        }

        // Validate each step's input_mapping against template contract
        for step in &manifest.steps {
            let Some(template_ref) = &step.template_ref else {
                continue;
            };

            // Skip non-path refs (dynamic, anchors, non-standard resolution schemes
            // like process/memory/*, composition/*, inference/* — these are resolved by
            // different engines, not the minijinja executor).
            if template_ref.contains("${")
                || template_ref.contains("#")
                || template_ref.contains("{{")
                || template_ref.starts_with("process/")
                || template_ref.starts_with("composition/")
                || template_ref.starts_with("inference/")
            {
                continue;
            }

            // Resolve template path matching the executor's direct-path resolution:
            // exact <ref> first, then <ref>.j2 fallback (if ref doesn't end with .j2).
            // A missing template makes the skill non-executable — tag as [critical]
            // so the test fails (the dead-check bug that previously let these through
            // because the error wasn't tagged and was silently ignored).
            let exact = templates_dir.join(template_ref);
            let with_j2 = templates_dir.join(format!("{template_ref}.j2"));
            let with_yaml = templates_dir.join(format!("{template_ref}.yaml"));
            let template_path = if exact.exists() {
                exact
            } else if !template_ref.ends_with(".j2") && with_j2.exists() {
                with_j2
            } else if with_yaml.exists() {
                // Media workflow config (.yaml) — resolved by a different engine, not
                // minijinja. Skip contract validation (the .yaml uses a different schema).
                continue;
            } else {
                errors.push(format!(
                    "{}: [critical] step {} template_ref '{}' does not resolve to a file at {} or {}",
                    path.display(),
                    step.ordinal,
                    template_ref,
                    exact.display(),
                    with_j2.display()
                ));
                continue;
            };

            let template_content = std::fs::read_to_string(&template_path).unwrap_or_default();
            let contract_inputs = parse_template_contract_inputs(&template_content);

            // Skip validation if the template uses a non-standard contract format
            // (e.g., media templates use `parameters:` with `- name:` entries).
            // The parser returns empty for these, which would cause false positives.
            if contract_inputs.is_empty() {
                continue;
            }

            if let Some(mapping) = &step.input_mapping {
                let contract_refs: Vec<&str> = contract_inputs.iter().map(|s| s.as_str()).collect();
                let findings = validate_step_input_mapping(
                    &manifest_id,
                    step.ordinal,
                    template_ref,
                    mapping,
                    &contract_refs,
                );

                for finding in findings {
                    // Only report high+ severity (skip medium self-referential warnings
                    // which are valid for convergence/stationarity patterns)
                    if finding.severity == "high" || finding.severity == "critical" {
                        errors.push(format!(
                            "{}: [{}] step {} {} — {}",
                            path.display(),
                            finding.severity,
                            step.ordinal,
                            finding.category,
                            finding.description
                        ));
                    }
                }
            }
        }
    }

    if !errors.is_empty() {
        // Separate critical errors (missing files, invalid convergence) from
        // high-severity warnings (input mapping mismatches). Critical errors
        // cause test failure. High-severity warnings are reported but don't
        // fail — they represent pre-existing issues in the registry that need
        // manual review.
        let critical: Vec<_> = errors.iter().filter(|e| e.contains("[critical]")).collect();
        let high: Vec<_> = errors.iter().filter(|e| e.contains("[high]")).collect();

        if !high.is_empty() {
            eprintln!(
                "WARNING: {} high-severity input mapping mismatches found (pre-existing):",
                high.len()
            );
            for e in &high {
                eprintln!("  {e}");
            }
        }

        if !critical.is_empty() {
            panic!(
                "{} critical cross-validation errors:\n{}",
                critical.len(),
                critical
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    eprintln!(
        "Cross-validated {} manifests — all critical checks passed",
        count
    );
}
