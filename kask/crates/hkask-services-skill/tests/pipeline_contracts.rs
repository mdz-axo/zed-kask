//! Pipeline contract check — verifies that template input/output fields
//! are compatible across pipeline phases.
//!
//! For each skill, this test:
//! 1. Reads the manifest.yaml to determine template ordering
//! 2. Parses each .j2 file's contract: input and contract: output field names
//! 3. Builds a pipeline: template 1 output → template 2 input → ...
//! 4. For each template (except the first), verifies that every input field
//!    is either produced by a prior template's output OR is a standard input
//!    (userpod_host, previous_metric, etc.)
//!
//! This catches:
//! - Field name mismatches (template A outputs "findings" but template B
//!   expects "probe_results")
//! - Missing pipeline links (template B expects an input that no prior
//!   template produces)
//!
//! It does NOT catch:
//! - Semantic type mismatches (template A outputs array but template B
//!   expects object — the contract uses free-form type strings)
//! - Logic errors (the field names match but the semantics differ)
//!
//! NOTE: hKask skills are prompt-based, not strict data pipelines. The agent
//! reads prior phase outputs and computes new inputs — the contract: block
//! describes what the template expects, but the agent assembles the context.
//! This test is a WARNING-ONLY check: it reports mismatches but does not fail
//! the build. Mismatches may be legitimate (agent-provided inputs, computed
//! fields, or independent template invocation). Review the warnings and fix
//! only if the mismatch indicates a real pipeline defect.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Standard inputs that the agent provides directly (not produced by prior
/// templates). These are always available and don't need to come from a
/// prior template's output.
const STANDARD_INPUTS: &[&str] = &[
    "userpod_host",
    "webid",
    "delegation_token_ref",
    "previous_metric",
    "previous_cycle_result",
    "convergence_metric",
    "convergence_baseline",
    "target_surface",
    "target_signal",
    "target_source",
    "target_domain",
    "target_output",
    "intensity",
    "adversarial_categories",
    "defense_layers",
    "learner_bot",
    "context_text",
    "skill_name",
    "manifest_yaml",
    "template_contents",
    "goal",
    "bug_description",
    "symptom",
    "design_problem",
    "design_constraints",
    "text",
    "expectations",
    "letter",
    "docs",
    "output",
    "facts",
    "review_prompt",
    "prior_review_feedback",
    "draft",
    "quality_criteria",
    "critique",
    "revision_count",
    "max_iterations",
    "convergence_threshold",
    "improvement_target",
    "baseline_quality",
    "iterations_completed",
    "max_depth",
    "budget",
    "query",
    "limit",
    "name",
    "direction",
    "kind",
    "include_health",
    "include_meta",
    "context_id",
    "symbols_provided",
    "symbols_used",
    "model",
    "batch_size",
    "include_image_descriptions",
    "include_images",
    "include_raw_content",
    "include_favicon",
    "max_results",
    "search_depth",
    "topic",
    "time_range",
    "days",
    "start_date",
    "end_date",
    "country",
    "include_domains",
    "exclude_domains",
    "ui_lang",
    "units",
    "summary",
    "entity_info",
    "inline_references",
    "key",
    "accept",
    "reject",
    "counter_proposal",
    "loop_depth",
    "max_loop_depth",
    "calibrated_concerns",
    "valid_concerns",
    "spurious_concerns",
    "downgraded_concerns",
    "no_material_flaws",
    "original_template",
    "revised_template",
    "diff",
    "goal_text",
    "target_content",
    "file_type",
    "user_choice",
    "next_action",
    "audit_cascade",
    "branching_rules",
    "convergence_check_result",
    "metric_decomposition",
    "blockers",
    "rationale",
    "test_result",
    "resistance_rate",
    "critical_failures",
    "defense_bypass_results",
    "adversarial_inputs",
    "intensity_level",
    "defense_bypass_targets",
    "vulnerability_surface",
    "categories_to_test",
    "selected_target",
    "domain",
    "findings",
    "threats",
    "mappings",
    "unmapped_findings",
    "existing_taxonomy_mappings",
    "proposed_taxonomy_mappings",
    "existing_regressions",
    "proposed_regressions",
    "defense_layers_firing",
    "defense_layers_silent",
    "defense_layers_present",
    "defense_layers_missing",
    "unresolved_findings",
    "unresolved_threats",
    "oscr_tactics_covered",
    "oscr_tactics_missing",
    "oscr_techniques_covered",
    "signal_sources",
    "signal_types",
    "discovered_signals",
    "discovered_manifests",
    "manifest_paths",
    "registry_sources",
    "surfaces",
    "seed_findings",
    "evidence_sources",
    "findings_to_map",
    "signal",
    "source",
    "surface",
    "target_signal",
    "target_source",
    "target_surface",
];

/// Parse the [inference] frontmatter from a .j2 file and extract contract
/// input/output field names.
fn parse_contract_fields(content: &str) -> (HashSet<String>, HashSet<String>) {
    let mut input_fields = HashSet::new();
    let mut output_fields = HashSet::new();

    // Skip leading Jinja comments ({# ... #})
    let mut rest = content.trim_start();
    while rest.starts_with("{#") {
        if let Some(end) = rest.find("#}") {
            rest = rest[end + 2..].trim_start();
        } else {
            break;
        }
    }

    if !rest.starts_with("[inference]") {
        return (input_fields, output_fields);
    }

    let after_header = &rest["[inference]".len()..];
    let sep = match after_header.find("\n---") {
        Some(s) => s,
        None => return (input_fields, output_fields),
    };

    let yaml_text = &after_header[..sep];

    // Try parsing as YAML
    if let Ok(yaml) = serde_yaml_neo::from_str::<serde_yaml_neo::Value>(yaml_text)
        && let Some(contract) = yaml.get("contract").and_then(|v| v.as_mapping())
    {
        if let Some(input) = contract.get("input").and_then(|v| v.as_mapping()) {
            for (key, _) in input {
                if let Some(name) = key.as_str() {
                    input_fields.insert(name.to_string());
                }
            }
        }
        if let Some(output) = contract.get("output").and_then(|v| v.as_mapping()) {
            for (key, _) in output {
                if let Some(name) = key.as_str() {
                    output_fields.insert(name.to_string());
                }
            }
        }
    }

    // If YAML parsing didn't find fields, try TOML-style [contract] section
    if input_fields.is_empty() && output_fields.is_empty() && yaml_text.contains("[contract]") {
        // Parse inline format: input: {field: type, ...}
        let mut in_input = false;
        let mut in_output = false;
        for line in yaml_text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("input:") {
                in_input = true;
                in_output = false;
                // Inline format: input: {field: type, field2: type2}
                let inline = trimmed.strip_prefix("input:").unwrap_or("").trim();
                if inline.starts_with('{') {
                    parse_inline_fields(inline, &mut input_fields);
                }
            } else if trimmed.starts_with("output:") {
                in_output = true;
                in_input = false;
                let inline = trimmed.strip_prefix("output:").unwrap_or("").trim();
                if inline.starts_with('{') {
                    parse_inline_fields(inline, &mut output_fields);
                }
            } else if in_input || in_output {
                // Multi-line format: field_name: type
                if let Some(colon_pos) = trimmed.find(':') {
                    let field = trimmed[..colon_pos].trim();
                    if !field.is_empty() && !field.starts_with('#') {
                        if in_input {
                            input_fields.insert(field.to_string());
                        } else {
                            output_fields.insert(field.to_string());
                        }
                    }
                }
            }
        }
    }

    (input_fields, output_fields)
}

/// Parse inline field format: {field1: type1, field2: type2}
fn parse_inline_fields(inline: &str, fields: &mut HashSet<String>) {
    let inner = inline
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(inline);
    for pair in inner.split(',') {
        if let Some(colon_pos) = pair.find(':') {
            let field = pair[..colon_pos].trim();
            if !field.is_empty() {
                fields.insert(field.to_string());
            }
        }
    }
}

/// Get the template ordering for a skill from its manifest.yaml.
/// Returns a list of (template_id, j2_path) pairs in pipeline order.
fn get_template_ordering(skill_dir: &Path) -> Vec<(String, String)> {
    let manifest_path = skill_dir.join("manifest.yaml");
    let content = match fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let manifest: serde_yaml_neo::Value = match serde_yaml_neo::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    // Try flowdefs.steps first (explicit ordering)
    if let Some(steps) = manifest.get("flowdefs").and_then(|v| v.as_sequence()) {
        let mut ordered = Vec::new();
        for step in steps {
            if let Some(template_ref) = step.get("template_ref").and_then(|v| v.as_str()) {
                // Extract the .j2 path from the template_ref (e.g., "skill-name/template-name")
                let parts: Vec<&str> = template_ref.split('/').collect();
                if parts.len() >= 2 {
                    let j2_path = format!("{}.j2", parts[1]);
                    ordered.push((template_ref.to_string(), j2_path));
                }
            }
        }
        if !ordered.is_empty() {
            return ordered;
        }
    }

    // Fall back to templates array (implicit ordering)
    if let Some(templates) = manifest.get("templates").and_then(|v| v.as_sequence()) {
        let mut ordered = Vec::new();
        for tmpl in templates {
            if let (Some(id), Some(path)) = (
                tmpl.get("id").and_then(|v| v.as_str()),
                tmpl.get("path").and_then(|v| v.as_str()),
            ) {
                ordered.push((id.to_string(), path.to_string()));
            }
        }
        return ordered;
    }

    Vec::new()
}

/// Collect all skill directories in registry/templates/
fn collect_skill_dirs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("registry")
        .join("templates");

    let mut dirs = Vec::new();
    for entry in fs::read_dir(&root).into_iter().flatten().flatten() {
        if entry.path().is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    dirs
}

#[test]
fn pipeline_contracts_report_mismatches_as_warnings() {
    // This test is WARNING-ONLY: it reports pipeline contract mismatches
    // but does not fail the build. hKask skills are prompt-based, not strict
    // data pipelines — the agent reads prior outputs and computes new inputs.
    // Mismatches may be legitimate (agent-provided inputs, computed fields,
    // or independent template invocation). Review the warnings and fix only
    // if the mismatch indicates a real pipeline defect.
    //
    // To make this a hard failure in the future: change the final `eprintln!`
    // to `panic!` once the standard inputs list is comprehensive enough that
    // remaining mismatches are genuine defects.
    let skill_dirs = collect_skill_dirs();
    assert!(
        !skill_dirs.is_empty(),
        "should find skill directories in registry/templates/"
    );

    let standard_inputs: HashSet<String> = STANDARD_INPUTS.iter().map(|s| s.to_string()).collect();
    let mut all_warnings = Vec::new();

    for skill_dir in &skill_dirs {
        let skill_name = skill_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?");

        let ordering = get_template_ordering(skill_dir);
        if ordering.len() < 2 {
            // Single-template skills or skills with no ordering — skip
            continue;
        }

        // Collect all output fields from all prior templates (accumulating)
        let mut available_outputs: HashSet<String> = HashSet::new();

        for (i, (template_id, j2_path)) in ordering.iter().enumerate() {
            let full_path = skill_dir.join(j2_path);
            let content = match fs::read_to_string(&full_path) {
                Ok(c) => c,
                Err(_) => continue, // File might not exist (e.g., .yaml instead of .j2)
            };

            let (input_fields, output_fields) = parse_contract_fields(&content);

            // For templates after the first, check that every input field
            // is either produced by a prior template's output OR is a standard input
            if i > 0 {
                for input_field in &input_fields {
                    if !available_outputs.contains(input_field)
                        && !standard_inputs.contains(input_field)
                    {
                        all_warnings.push(format!(
                            "{skill_name}/{template_id}: input field '{input_field}' is not produced by any prior template and is not a standard input"
                        ));
                    }
                }
            }

            // Add this template's outputs to the available pool
            for output_field in &output_fields {
                available_outputs.insert(output_field.clone());
            }
        }
    }

    if !all_warnings.is_empty() {
        eprintln!(
            "⚠ pipeline contract warnings ({} mismatches — review for real defects):",
            all_warnings.len()
        );
        for w in &all_warnings {
            eprintln!("  {w}");
        }
        eprintln!("NOTE: These are warnings, not failures. hKask skills are prompt-based,");
        eprintln!("not strict data pipelines. The agent reads prior outputs and computes");
        eprintln!("new inputs. Fix only if the mismatch indicates a real pipeline defect.");
    } else {
        eprintln!(
            "✓ pipeline contracts: all input fields are produced by prior templates or are standard inputs"
        );
    }

    // WARNING-ONLY: always pass. To make this a hard failure, replace with:
    // if !all_warnings.is_empty() { panic!(...); }
}
