//! Additional structural property tests for skill manifests and templates.
//!
//! These tests complement `manifest_compliance.rs` and `manifest_properties.rs`
//! by checking invariants that are NOT in the skill-maintenance-validate
//! template's check catalog but should hold for all manifests:
//!
//! 1. **Loop target validity** — `loop_target` (after stripping Jinja) must
//!    reference an ordinal that exists in the manifest's steps.
//! 2. **Convergence config consistency** — if `convergence_mode` includes
//!    "gap", target fields must be set; if "calibration", prediction/result
//!    fields must be set.
//! 3. **Gas budget adequacy** — `gas.cap` must be >= sum of per-step
//!    `gas_cap` values × `min_iterations`.
//! 4. **No orphan .j2 files** — every `.j2` file in a skill's template
//!    directory should be listed in the crate manifest.
//! 5. **Crate manifest paths resolve** — every `path` in the crate manifest
//!    resolves to an existing file.
//! 6. **Span namespace format** — `span_namespace` matches `reg.skill.<id>`.
//! 7. **Template inference block consistency** — templates with inference
//!    parameters (temperature, max_tokens) must declare `template_type`
//!    KnowAct or WordAct, not FlowDef or RenderAct.

use hkask_templates::load_manifest_from_yaml;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Known compute_ref primitives — kept in sync with `compute.rs`.
const KNOWN_COMPUTE_REFS: &[&str] = &[
    "lisp.eval",
    "shell.exec",
    "kata.object_gap",
    "kata.process_gap",
    "kata.hypotenuse",
    "kata.prediction_vs_result",
    "brier_score",
    "brier_score_multi",
    "brier_interpretation",
    "calibrate_from_fermi",
    "outside_view_adjustment",
    "bayesian_update",
    "apply_calibration_adjustment",
    "combine_tree_probabilities",
    "swarm.converge_accumulate",
    "swarm.second_order_monitor",
    "swarm.filter_proposed_moves",
    "listening.chunk_transcript",
    "listening.verify_citations",
];

fn registry_manifests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests")
}

fn registry_templates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/templates")
}

/// 1. Loop target validity — `loop_target` must reference an existing ordinal.
///
/// The `loop_target` value is a Jinja expression that renders to a number
/// (e.g., `"{{ 1 }}"` or `"{{ 2 if condition else 4 }}"`). We extract the
/// numeric literals from the expression and check that at least one of them
/// is a valid ordinal in the manifest's steps. If the expression is a simple
/// `"{{ N }}"`, we check that N exists.
#[test]
fn loop_targets_reference_valid_ordinals() {
    let dir = registry_manifests_dir();
    if !dir.exists() {
        eprintln!("{} not found — skipping", dir.display());
        return;
    }

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let fname = path.file_name().unwrap().to_string_lossy();
        let valid_ordinals: HashSet<u32> = manifest.steps.iter().map(|s| s.ordinal).collect();

        for step in &manifest.steps {
            if step.action != "loop" {
                continue;
            }
            let Some(mapping) = &step.input_mapping else {
                continue;
            };
            let Some(lt) = mapping.get("loop_target") else {
                continue;
            };
            let lt_str = lt.as_str().unwrap_or("");

            // Extract numeric literals from the Jinja expression.
            // Patterns: "{{ N }}", "{{ N if ... else M }}", etc.
            let nums: Vec<u32> = LOOP_TARGET_NUM_RE
                .captures_iter(lt_str)
                .filter_map(|c| c.get(1).and_then(|m| m.as_str().parse().ok()))
                .collect();

            if nums.is_empty() {
                // Can't extract numbers — skip (might be a complex expression).
                continue;
            }
            checked += 1;

            // At least one extracted number must be a valid ordinal.
            let any_valid = nums.iter().any(|n| valid_ordinals.contains(n));
            if !any_valid {
                errors.push(format!(
                    "{fname}: step {} — loop_target '{}' references ordinal(s) {nums:?} but valid ordinals are {:?}",
                    step.ordinal, lt_str, valid_ordinals
                ));
            }
        }
    }

    eprintln!(
        "Loop target validity: {checked} loop steps checked — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} loop target validity errors:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// 2. Convergence config consistency — mode-specific fields must be present.
///
/// - If `convergence_mode` contains "gap": `target_artifacts_field` or
///   `target_procedure_field` must be set (gap convergence needs a target).
/// - If `convergence_mode` contains "calibration": `prediction_field` and
///   `result_field` must be set (Brier scoring needs prediction + result).
/// - If `convergence_mode` is empty (legacy): `threshold` > 0 and
///   `convergence_field` must be non-empty.
#[test]
fn convergence_config_is_internally_consistent() {
    let dir = registry_manifests_dir();
    if !dir.exists() {
        return;
    }

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let fname = path.file_name().unwrap().to_string_lossy();
        let conv = &manifest.convergence;
        let mode = &conv.convergence_mode;
        checked += 1;

        // Kata mode checks
        if !mode.is_empty() {
            if mode.contains("gap") {
                let has_target =
                    conv.target_artifacts_field.is_some() || conv.target_procedure_field.is_some();
                if !has_target {
                    errors.push(format!(
                        "{fname}: convergence_mode contains 'gap' but no target_artifacts_field or target_procedure_field set"
                    ));
                }
            }
            if mode.contains("calibration") {
                if conv.prediction_field.is_none() {
                    errors.push(format!(
                        "{fname}: convergence_mode contains 'calibration' but no prediction_field set"
                    ));
                }
                if conv.result_field.is_none() {
                    errors.push(format!(
                        "{fname}: convergence_mode contains 'calibration' but no result_field set"
                    ));
                }
            }
        } else {
            // Legacy mode
            if conv.threshold <= 0.0 {
                errors.push(format!(
                    "{fname}: legacy convergence_mode (empty) but threshold <= 0"
                ));
            }
            if conv.convergence_field.is_empty() {
                errors.push(format!(
                    "{fname}: legacy convergence_mode (empty) but convergence_field is empty"
                ));
            }
        }

        // max_iterations must be > 0
        if conv.max_iterations == 0 {
            errors.push(format!("{fname}: max_iterations is 0"));
        }

        // min_iterations must be <= max_iterations
        if conv.min_iterations > conv.max_iterations {
            errors.push(format!(
                "{fname}: min_iterations ({}) > max_iterations ({})",
                conv.min_iterations, conv.max_iterations
            ));
        }

        // on_not_reached must be a valid value
        let on_not_reached = &conv.on_not_reached;
        if !["abort", "escalate", "proceed"].contains(&on_not_reached.as_str()) {
            errors.push(format!(
                "{fname}: on_not_reached='{on_not_reached}' (must be abort, escalate, or proceed)"
            ));
        }
    }

    eprintln!(
        "Convergence config consistency: {checked} manifests checked — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} convergence config errors:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// 3. Gas budget adequacy — `gas.cap` must be >= sum of per-step `gas_cap`
///    values × `min_iterations`.
///
/// This catches manifests where the total gas budget is too small for the
/// cascade to complete even one minimum iteration cycle. A manifest with
/// 5 steps each with `gas_cap: 6000` and `min_iterations: 2` needs at least
/// `60000` gas.
#[test]
fn gas_budget_is_adequate_for_min_iterations() {
    let dir = registry_manifests_dir();
    if !dir.exists() {
        return;
    }

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let fname = path.file_name().unwrap().to_string_lossy();
        checked += 1;

        let total_step_gas: u64 = manifest.steps.iter().map(|s| s.gas_cap as u64).sum();
        let min_needed = total_step_gas * manifest.convergence.min_iterations as u64;
        let gas_cap = manifest.gas.cap as u64;

        if gas_cap < min_needed {
            errors.push(format!(
                "{fname}: gas.cap={gas_cap} but sum(step.gas_cap)={total_step_gas} × min_iterations={} = {min_needed} needed",
                manifest.convergence.min_iterations
            ));
        }
    }

    eprintln!(
        "Gas budget adequacy: {checked} manifests checked — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} gas budget errors:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// 4. No orphan .j2 files — every `.j2` file in a skill's template directory
///    should be listed in the crate manifest's `templates` array.
///
/// Catches templates that were added to the filesystem but not registered in
/// the crate manifest (invisible to the executor's template loader).
#[test]
fn no_orphan_j2_files() {
    let templates_dir = registry_templates_dir();
    if !templates_dir.exists() {
        return;
    }

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&templates_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "j2") {
            continue;
        }
        // Get the skill directory (parent of the .j2 file, or grandparent
        // if the .j2 is in a subdirectory like media/).
        let skill_dir = path
            .parent()
            .and_then(|p| {
                // If the parent is a subdirectory (e.g., "media"), go up one more.
                let parent_name = p.file_name()?.to_string_lossy().to_string();
                if parent_name == "media" || parent_name == "sub-manifests" {
                    p.parent()
                } else {
                    Some(p)
                }
            })
            .unwrap_or(path.parent().unwrap());

        let skill_name = skill_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let crate_manifest_path = skill_dir.join("manifest.yaml");
        if !crate_manifest_path.is_file() {
            // No crate manifest — skip (caught by R1 check).
            continue;
        }

        // Parse the crate manifest and get listed paths.
        let crate_yaml = std::fs::read_to_string(&crate_manifest_path).unwrap();
        let crate_manifest: serde_yaml_neo::Value = match serde_yaml_neo::from_str(&crate_yaml) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let listed_paths: HashSet<String> = crate_manifest
            .get("templates")
            .and_then(|t| t.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|t| t.get("path").and_then(|p| p.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Check if this .j2 file is listed.
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();
        let relative_path = path
            .strip_prefix(skill_dir)
            .unwrap()
            .to_string_lossy()
            .to_string();

        checked += 1;
        if !listed_paths.contains(&file_name) && !listed_paths.contains(&relative_path) {
            errors.push(format!(
                "{skill_name}/{file_name}: .j2 file exists but is not listed in crate manifest"
            ));
        }
    }

    eprintln!(
        "Orphan .j2 check: {checked} files checked — {} orphans",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} orphan .j2 files:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// 5. Crate manifest paths resolve — every `path` in the crate manifest
///    resolves to an existing file relative to the crate manifest's directory.
#[test]
fn crate_manifest_paths_resolve() {
    let templates_dir = registry_templates_dir();
    if !templates_dir.exists() {
        return;
    }

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&templates_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let manifest_path = entry.path();
        if manifest_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            != Some("manifest.yaml".to_string())
        {
            continue;
        }
        let skill_dir = manifest_path.parent().unwrap();
        let skill_name = skill_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let yaml = std::fs::read_to_string(manifest_path).unwrap();
        let crate_manifest: serde_yaml_neo::Value = match serde_yaml_neo::from_str(&yaml) {
            Ok(v) => v,
            Err(e) => {
                errors.push(format!("{skill_name}: crate manifest parse error: {e}"));
                continue;
            }
        };

        let templates = match crate_manifest
            .get("templates")
            .and_then(|t| t.as_sequence())
        {
            Some(t) => t,
            None => continue,
        };

        for t in templates {
            checked += 1;
            let path_str = t.get("path").and_then(|p| p.as_str()).unwrap_or("");
            if path_str.is_empty() {
                errors.push(format!("{skill_name}: template entry with empty path"));
                continue;
            }
            let resolved = skill_dir.join(path_str);
            if !resolved.is_file() {
                errors.push(format!(
                    "{skill_name}: path '{path_str}' does not resolve to {}",
                    resolved.display()
                ));
            }
        }
    }

    eprintln!(
        "Crate manifest path resolution: {checked} paths checked — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} crate manifest path errors:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// 6. Span namespace format — `span_namespace` must be `reg.skill.<manifest.id>`.
///
/// This is the E11 check, deterministically enforced here (not just
/// LLM-validated via the validate template).
#[test]
fn span_namespace_matches_manifest_id() {
    let dir = registry_manifests_dir();
    if !dir.exists() {
        return;
    }

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let fname = path.file_name().unwrap().to_string_lossy();
        checked += 1;

        let expected = format!("reg.skill.{}", manifest.id);
        let actual = yaml
            .lines()
            .find(|l| l.trim_start().starts_with("span_namespace:"))
            .and_then(|l| l.split(':').nth(1))
            .map(|v| v.trim().trim_matches('"').to_string())
            .unwrap_or_default();

        if actual != expected {
            errors.push(format!(
                "{fname}: span_namespace='{actual}' expected '{expected}'"
            ));
        }

        // Check for abolished spans: list (not emit_spans).
        for line in yaml.lines() {
            let stripped = line.trim_start();
            if stripped.starts_with("spans:") && !stripped.starts_with("emit_spans:") {
                errors.push(format!("{fname}: abolished 'spans:' list found in ledger"));
                break;
            }
        }
    }

    eprintln!(
        "Span namespace: {checked} manifests checked — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} span namespace errors:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// 7. Template inference block consistency — templates with inference
///    parameters (temperature, max_tokens, thinking_budget) in their body
///    `[inference]` block must declare `template_type` KnowAct or WordAct
///    in their frontmatter, not FlowDef or RenderAct.
///
/// FlowDef and RenderAct templates don't use inference — they're rendered
/// without an LLM call. A FlowDef/RenderAct template with inference parameters
/// is a misconfiguration: the parameters would be ignored at runtime.
#[test]
fn template_inference_block_matches_template_type() {
    let templates_dir = registry_templates_dir();
    if !templates_dir.exists() {
        return;
    }

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in walkdir::WalkDir::new(&templates_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "j2") {
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Extract frontmatter template_type.
        let fm_type = extract_frontmatter_template_type(&content);
        if fm_type.is_none() {
            continue; // No frontmatter — skip (R10 warning covers this).
        }
        let fm_type = fm_type.unwrap();
        checked += 1;

        // Check for a body [inference] block (after the --- separator).
        let after_separator = match content.find("\n---\n") {
            Some(pos) => &content[pos + 5..],
            None => continue,
        };

        let has_body_inference = after_separator.contains("[inference]");
        let has_inference_params = after_separator
            .lines()
            .take_while(|l| !l.is_empty() || l.starts_with('['))
            .any(|l| {
                l.contains("temperature")
                    || l.contains("max_tokens")
                    || l.contains("thinking_budget")
                    || l.contains("work_effort")
                    || l.contains("verbosity")
            });

        if has_body_inference && has_inference_params {
            // This template uses inference. Its template_type must be KnowAct
            // or WordAct, not FlowDef or RenderAct.
            if fm_type == "FlowDef" || fm_type == "RenderAct" {
                let rel_path = path.strip_prefix(&templates_dir).unwrap_or(path).display();
                errors.push(format!(
                    "{rel_path}: template_type='{fm_type}' but has body [inference] block with parameters — FlowDef/RenderAct templates don't use inference"
                ));
            }
        }
    }

    eprintln!(
        "Template inference consistency: {checked} templates checked — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} template inference consistency errors:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Extract `template_type` from a .j2 template's frontmatter.
fn extract_frontmatter_template_type(content: &str) -> Option<String> {
    let separator_pos = content.find("\n---\n")?;
    let frontmatter = &content[..separator_pos];
    // Strip Jinja comments.
    let stripped = strip_jinja_comments(frontmatter);
    let frontmatter = stripped
        .trim()
        .strip_prefix("[inference]")
        .unwrap_or(&stripped)
        .trim();
    let parsed: serde_json::Value = serde_yaml_neo::from_str(frontmatter).ok()?;
    parsed
        .get("template_type")
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Strip Jinja comments (`{# ... #}`) from a string.
fn strip_jinja_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'#') {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '#' && chars.peek() == Some(&'}') {
                    chars.next();
                    break;
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Regex to extract numeric literals from Jinja expressions like `"{{ 1 }}"`.
static LOOP_TARGET_NUM_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"\{\{\s*(\d+)\s*").unwrap());
