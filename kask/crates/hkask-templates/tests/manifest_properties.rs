//! Structural property tests for skill manifests.
//!
//! These tests verify invariants that the skill-maintenance-validate template
//! encodes as LLM-validated checks (E9, E10, E7) plus structural properties
//! that are NOT in the validate template but should hold for all manifests:
//!
//! - Loop steps have `convergence_signal` (unless single-pass)
//! - Loop steps have `loop_target`
//! - `compute_ref` is a known primitive
//! - `select` steps have `template_ref` resolving to an existing file
//! - Step ordinals are sequential
//! - Every step has a non-empty description
//! - Condition expressions don't reference forward step results
//! - `select` step `input_mapping` fields that reference `step_N_result.field`
//!   have corresponding `field` in the referenced template's `contract.output`

use hkask_templates::load_manifest_from_yaml;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

/// Known compute_ref primitives supported by `dispatch_compute`.
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

/// E9: Every step has a non-empty `description` field.
/// E10: Step ordinals are sequential starting from 1.
/// E7: Every `template_ref` resolves to an existing .j2 file.
/// Loop steps have `convergence_signal` and `loop_target`.
/// `compute_ref` is a known primitive.
/// Condition expressions don't reference forward step results.
#[test]
fn all_manifests_have_structural_integrity() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    if !dir.exists() {
        eprintln!("{} not found — skipping test", dir.display());
        return;
    }

    let templates_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/templates");

    let known_refs: HashSet<&str> = KNOWN_COMPUTE_REFS.iter().copied().collect();
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
            Err(_) => continue, // load failures caught by other tests
        };
        checked += 1;
        let fname = path.file_name().unwrap().to_string_lossy();

        // E10: Sequential ordinals starting from 1 (or 0 for pre-processing).
        let start_ordinal = manifest.steps.first().map(|s| s.ordinal).unwrap_or(1);
        if start_ordinal != 0 && start_ordinal != 1 {
            errors.push(format!(
                "{fname}: E10 — first step ordinal is {start_ordinal} (expected 0 or 1)"
            ));
        }
        for (i, step) in manifest.steps.iter().enumerate() {
            let expected = start_ordinal + i as u32;
            if step.ordinal != expected {
                errors.push(format!(
                    "{fname}: E10 — step at index {i} has ordinal {} (expected {expected})",
                    step.ordinal
                ));
            }
        }

        let _ordinals: HashSet<u32> = manifest.steps.iter().map(|s| s.ordinal).collect();

        for step in &manifest.steps {
            // E9: Non-empty description.
            if step.description.trim().is_empty() {
                errors.push(format!(
                    "{fname}: E9 — step {} has empty description",
                    step.ordinal
                ));
            }

            // compute_ref must be a known primitive.
            if step.action == "compute" {
                if let Some(ref compute_ref) = step.compute_ref {
                    if !known_refs.contains(compute_ref.as_str()) {
                        errors.push(format!(
                            "{fname}: step {} — unknown compute_ref '{}'",
                            step.ordinal, compute_ref
                        ));
                    }
                } else {
                    errors.push(format!(
                        "{fname}: step {} — action 'compute' but no compute_ref",
                        step.ordinal
                    ));
                }
            }

            // select/populate steps must have template_ref (unless they have mcp: for direct MCP tool invocation).
            if step.action == "select" || step.action == "populate" {
                if step.mcp.is_some() {
                    // MCP tool invocation — no template_ref needed.
                } else if step.template_ref.is_none() || step.template_ref.as_deref() == Some("") {
                    errors.push(format!(
                        "{fname}: step {} — action '{}' but no template_ref",
                        step.ordinal, step.action
                    ));
                } else if let Some(ref tref) = step.template_ref {
                    // E7: template_ref resolves to an existing file.
                    let tref_str = tref;
                    let file_ref = if tref_str.ends_with(".j2") {
                        tref_str.to_string()
                    } else {
                        format!("{tref_str}.j2")
                    };
                    let template_path = templates_dir.join(&file_ref);
                    if !template_path.is_file() {
                        errors.push(format!(
                            "{fname}: step {} — E7 — template_ref '{}' does not resolve to {}",
                            step.ordinal,
                            tref,
                            template_path.display()
                        ));
                    }
                }
            }

            // Loop steps must have loop_target and convergence_signal (unless single-pass).
            if step.action == "loop" {
                let mapping = step.input_mapping.as_ref();
                let has_loop_target = mapping.and_then(|m| m.get("loop_target")).is_some();
                if !has_loop_target {
                    errors.push(format!(
                        "{fname}: step {} — action 'loop' but no loop_target in input_mapping",
                        step.ordinal
                    ));
                }

                // convergence_signal is required unless max_iterations <= 1 (single-pass).
                let max_iter = manifest.convergence.max_iterations;
                let has_signal = mapping.and_then(|m| m.get("convergence_signal")).is_some();
                if !has_signal && max_iter > 1 {
                    errors.push(format!(
                        "{fname}: step {} — action 'loop' with max_iterations={max_iter} but no convergence_signal",
                        step.ordinal
                    ));
                }

                // Loop payload diagnostic: every key in the loop's
                // input_mapping (except loop_target and convergence_signal)
                // that is NOT in the target step's input_mapping is flagged
                // as a warning. The loop binds these keys into the global
                // context (via insert_protocol), so the target template CAN
                // access them via {{ variable }} even without an explicit
                // input_mapping binding. However, if the key is not in the
                // target's input_mapping, the template's contract.input may
                // not declare it — which means the contract alignment test
                // won't catch a mismatch. This is a style/robustness issue,
                // not a hard bug. Collected as warnings, not errors.
                if let Some(Value::Object(loop_map)) = mapping {
                    let loop_target_str = loop_map
                        .get("loop_target")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let target_ordinal: Option<u32> = loop_target_str
                        .trim_matches(|c: char| !c.is_ascii_digit())
                        .parse()
                        .ok();
                    if let Some(target_ord) = target_ordinal {
                        let target_step = manifest.steps.iter().find(|s| s.ordinal == target_ord);
                        if let Some(target) = target_step {
                            let target_keys: HashSet<String> = target
                                .input_mapping
                                .as_ref()
                                .and_then(|m| m.as_object())
                                .map(|obj| obj.keys().cloned().collect())
                                .unwrap_or_default();
                            for key in loop_map.keys() {
                                if key == "loop_target" || key == "convergence_signal" {
                                    continue;
                                }
                                if !target_keys.contains(key) {
                                    eprintln!(
                                        "  WARN: {fname}: step {} — loop injects '{}' into context but target step {} input_mapping does not declare it (template may still access via global context)",
                                        step.ordinal, key, target_ord
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Condition expressions must not reference forward step results.
            if let Some(ref cond) = step.condition {
                // Condition must not use Jinja syntax. The condition evaluator
                // (condition.rs) does NOT render Jinja — it evaluates raw
                // strings as dot-path lookups, comparisons, and boolean
                // compositions. A `{{ }}` wrapper makes the string unresolvable
                // as a dot path, causing `!=` conditions to always evaluate
                // true (the lhs resolves to a literal string that never
                // equals the rhs) and truthy conditions to always evaluate
                // false (the key is not found). Either way, the condition
                // gate is silently disabled.
                if cond.contains("{{") {
                    errors.push(format!(
                        "{fname}: step {} — condition contains Jinja syntax — the condition evaluator does not render Jinja. Use native syntax (dot paths, ==, !=, AND, OR, NOT). Condition: {cond}",
                        step.ordinal
                    ));
                }
                for cap in FORWARD_REF_RE.captures_iter(cond) {
                    if let Some(n_str) = cap.get(1) {
                        // Check this isn't prev_step_N_result.
                        let match_start = cap.get(0).unwrap().start();
                        let before = &cond[..match_start];
                        if before.ends_with("prev_") {
                            continue;
                        }
                        if let Ok(n) = n_str.as_str().parse::<u32>() {
                            if n >= step.ordinal {
                                errors.push(format!(
                                    "{fname}: step {} — condition references step_{}_result (forward/self reference — resolves to null, silently disabling the condition)",
                                    step.ordinal, n
                                ));
                            }
                        }
                    }
                }
            }

            // Diagnostic: `| default({})` or `| default([])` on step_N_result
            // references in input_mapping. If the step was skipped (condition
            // false) or the tool failed, the default masks the absence — the
            // consuming template can't distinguish "tool returned empty" from
            // "tool failed/skipped." This is a warning, not an error: the
            // default is sometimes correct (optional step, conditional skip).
            // Filter out known-legitimate patterns: `prior_*`, `prev_*`,
            // `previous_*` keys are loop-carried feedback that is intentionally
            // empty on the first iteration.
            if let Some(ref mapping) = step.input_mapping {
                if let Some(obj) = mapping.as_object() {
                    for (key, value) in obj {
                        if let Some(s) = value.as_str() {
                            if s.contains("step_")
                                && s.contains("_result")
                                && (s.contains("| default({})")
                                    || s.contains("| default([])"))
                                && !key.starts_with("prior_")
                                && !key.starts_with("prev_")
                                && !key.starts_with("previous_")
                            {
                                eprintln!(
                                    "  WARN: {fname}: step {} input_mapping '{}' uses | default on a step_N_result reference — verify the default is intentional (optional step / conditional skip), not masking a failure",
                                    step.ordinal, key
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    eprintln!(
        "Structural integrity: {checked} manifests checked — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} structural integrity errors found:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// `select` step `input_mapping` fields that reference `step_N_result.field`
/// should have corresponding `field` in the referenced template's `contract.output`.
///
/// This catches the class of bug where a manifest author references a field
/// that the template doesn't produce — the binding resolves to null silently.
///
/// This is a diagnostic test (does not fail) because some references are
/// intentional (agent-coordinated context, optional fields with defaults).
#[test]
fn select_step_input_mapping_fields_match_template_output() {
    let manifests_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests");
    let templates_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/templates");
    if !manifests_dir.exists() || !templates_dir.exists() {
        eprintln!("registry not found — skipping test");
        return;
    }

    let mut mismatches = Vec::new();
    let mut checked = 0u32;

    for entry in walkdir::WalkDir::new(&manifests_dir)
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
        let fname = path.file_name().unwrap().to_string_lossy().to_string();

        for step in &manifest.steps {
            if step.action != "select" {
                continue;
            }
            let Some(_template_ref) = &step.template_ref else {
                continue;
            };

            // Find all step_N_result.field references in this step's input_mapping.
            let mapping_str = step
                .input_mapping
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_default();

            // Extract (ordinal, field) pairs from step_N_result.field references.
            // Skip prev_step_N_result (cross-iteration, not current-iteration).
            let mut field_refs: Vec<(u32, String)> = Vec::new();
            for cap in STEP_FIELD_RE.captures_iter(&mapping_str) {
                // Check this isn't prev_step_N_result
                let full_match = cap.get(0).unwrap().as_str();
                if full_match.starts_with("prev_") {
                    continue;
                }
                if let (Some(n_str), Some(field_str)) = (cap.get(1), cap.get(2)) {
                    if let Ok(n) = n_str.as_str().parse::<u32>() {
                        field_refs.push((n, field_str.as_str().to_string()));
                    }
                }
            }

            if field_refs.is_empty() {
                continue;
            }

            // For each referenced step, find its template's contract.output fields.
            for (ref_ordinal, field) in &field_refs {
                // Find the step with this ordinal.
                let ref_step = manifest.steps.iter().find(|s| s.ordinal == *ref_ordinal);
                let Some(ref_step) = ref_step else { continue };
                let Some(ref ref_tref) = ref_step.template_ref else {
                    continue;
                };

                // Load the referenced template.
                let ref_file = if ref_tref.ends_with(".j2") {
                    ref_tref.clone()
                } else {
                    format!("{ref_tref}.j2")
                };
                let ref_path = templates_dir.join(&ref_file);
                let Ok(ref_content) = std::fs::read_to_string(&ref_path) else {
                    continue;
                };

                // Extract contract.output keys from the referenced template.
                let output_keys = extract_contract_output_keys(&ref_content);
                if output_keys.is_empty() {
                    continue; // No contract.output — can't check.
                }
                checked += 1;

                if !output_keys.contains(field) {
                    mismatches.push(format!(
                        "{fname} step {} references step_{}_result.{} but template '{}' contract.output has: {:?}",
                        step.ordinal, ref_ordinal, field, ref_tref, output_keys
                    ));
                }
            }
        }
    }

    eprintln!(
        "input_mapping/contract.output cross-check: {checked} field references checked, {} mismatches",
        mismatches.len()
    );
    for m in &mismatches {
        eprintln!("  MISMATCH: {m}");
    }

    // Regression ceiling: the current mismatch count is 0. Any new
    // mismatch is a potential bug (input_mapping references a field the
    // template doesn't produce). Intentional mismatches (optional fields
    // with defaults, agent-coordinated context) should be annotated and
    // the ceiling incremented.
    const MISMATCH_CEILING: usize = 0;
    assert!(
        mismatches.len() == MISMATCH_CEILING,
        "{count} input_mapping/contract.output mismatches (regression ceiling: {MISMATCH_CEILING}). \
         If the new mismatch is intentional, annotate it and increment MISMATCH_CEILING.",
        count = mismatches.len()
    );
}

/// Regex to find `step_N_result` in condition expressions.
/// We check for `prev_` prefix separately since the `regex` crate doesn't
/// support look-behind.
static FORWARD_REF_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"step_(\d+)_result").unwrap());

/// Regex to find `step_N_result.field` (not `prev_step_N_result.field`) in input_mapping.
static STEP_FIELD_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"(?:prev_)?step_(\d+)_result\.(\w+)").unwrap());

/// Extract `contract.output` field names from a .j2 template's frontmatter.
/// Mirrors `extract_contract_input_keys` but reads `contract.output` instead
/// of `contract.input`.
fn extract_contract_output_keys(template_content: &str) -> HashSet<String> {
    let mut keys = HashSet::new();
    let Some(separator_pos) = template_content.find("\n---\n") else {
        return keys;
    };
    let frontmatter = &template_content[..separator_pos];
    // Strip Jinja comments.
    let stripped = strip_jinja_comments(frontmatter);
    let frontmatter = stripped.trim();
    let frontmatter = frontmatter
        .strip_prefix("[inference]")
        .unwrap_or(frontmatter)
        .trim();
    let Ok(parsed) = serde_yaml_neo::from_str::<serde_json::Value>(frontmatter) else {
        return keys;
    };
    let Some(contract) = parsed.get("contract") else {
        return keys;
    };
    let Some(output) = contract.get("output") else {
        return keys;
    };
    if let Some(obj) = output.as_object() {
        for k in obj.keys() {
            keys.insert(k.clone());
        }
    }
    keys
}

/// Strip Jinja comments (`{# ... #}`) from a string.
fn strip_jinja_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'#') {
            chars.next(); // consume '#'
            let mut found_close = false;
            while let Some(c) = chars.next() {
                if c == '#' && chars.peek() == Some(&'}') {
                    chars.next(); // consume '}'
                    found_close = true;
                    break;
                }
            }
            if !found_close {
                result.push('{');
                result.push('#');
            }
        } else {
            result.push(ch);
        }
    }
    result
}
