//! Token-budget adequacy audit: verifies that every template in the registry
//! declares a `max_tokens` sufficient for its output schema complexity.
//!
//! The `[inference]` block's `max_tokens` controls the LLM's *output* token
//! limit. When a template has a structured output schema (the `contract:
//! output:` block), the model must emit the entire JSON response — all fields,
//! with their content — within `max_tokens`. Templates with many fields
//! (especially arrays of objects) need a higher `max_tokens` than the 2048
//! default.
//!
//! Without this audit, a template with a 13-field output schema (some arrays)
//! at `max_tokens: 2048` will silently truncate — the model runs out of tokens
//! before emitting the structured-output tool call, and the executor returns
//! `TemplateError::ParseFailure { .. }` (D25). This
//! is a runtime failure that's invisible at manifest-load time.
//!
//! # Principle grounding
//! - P1 (Correctness): the declared `max_tokens` must be sufficient for the
//!   declared output schema — an advertised contract with an insufficient
//!   budget is a silent failure waiting to happen.
//! - P4 (Clear Boundaries): the budget boundary must be adequate for the
//!   output boundary; a too-low `max_tokens` makes the output boundary
//!   unreachable.

use hkask_templates::load_manifest_from_yaml;
use hkask_templates::test_utils::{parse_and_strip_inference_block, strip_front_matter};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The default `max_tokens` when no `[inference]` block declares one.
const DEFAULT_MAX_TOKENS: u32 = 2048;

/// Minimum `max_tokens` for templates with 7+ output fields.
const MIN_MAX_TOKENS_COMPLEX: u32 = 4096;

/// Minimum `max_tokens` for templates with 10+ output fields.
const MIN_MAX_TOKENS_VERY_COMPLEX: u32 = 6144;

/// Field count thresholds.
const COMPLEX_FIELD_THRESHOLD: usize = 7;
const VERY_COMPLEX_FIELD_THRESHOLD: usize = 10;

/// Walk the registry templates directory and return all `.j2` file paths.
fn template_files() -> Vec<PathBuf> {
    let registry_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("registry")
        .join("templates");
    let mut files: Vec<PathBuf> = Vec::new();
    collect_j2_files(&registry_root, &mut files);
    files.sort();
    files
}

/// Actions that do not call inference and therefore do not consume
/// `max_tokens`. Templates used only by these steps are exempt from the
/// token-budget audit — they render deterministically with no LLM call,
/// so output truncation (D25) cannot occur.
const NON_INFERENCE_ACTIONS: &[&str] = &["render", "populate"];

/// Walk the registry manifests directory, load every manifest, and collect
/// the `template_ref` values from steps whose `action` is in
/// `NON_INFERENCE_ACTIONS`. These templates are exempt from the
/// token-budget audit because they never call inference.
///
/// Returns a set of template refs (without `.j2` extension) that should
/// be skipped by the audit.
fn non_inference_template_refs() -> HashSet<String> {
    let manifests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("registry")
        .join("manifests");
    let mut exempt: HashSet<String> = HashSet::new();

    let Ok(entries) = fs::read_dir(&manifests_dir) else {
        return exempt;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().is_some_and(|e| e == "yaml") {
            continue;
        }
        let yaml = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        for step in &manifest.steps {
            if NON_INFERENCE_ACTIONS.contains(&step.action.as_str()) {
                if let Some(ref template_ref) = step.template_ref {
                    // Strip .j2 extension to match the path comparison
                    // in is_non_inference_template (which also strips .j2).
                    let ref_no_ext = template_ref.strip_suffix(".j2").unwrap_or(template_ref);
                    exempt.insert(ref_no_ext.to_string());
                }
            }
        }
    }
    exempt
}

/// Check whether a template path is used only by non-inference steps.
/// Cross-references the template's path against the exempt set.
fn is_non_inference_template(path: &Path, exempt: &HashSet<String>) -> bool {
    let registry_templates = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("registry")
        .join("templates");
    let relative = match path.strip_prefix(&registry_templates) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let stem = relative.to_string_lossy().to_string();
    let stem_no_ext = stem.strip_suffix(".j2").unwrap_or(&stem);
    exempt.contains(stem_no_ext)
}

/// Check whether the template's output schema contains any `array` fields
/// without a `max_items` constraint. Unbounded arrays can hold arbitrarily
/// many items, requiring a higher `max_tokens` than the field-count heuristic
/// suggests. Returns true if at least one unbounded array field is found.
fn has_unbounded_array_output(template_content: &str) -> bool {
    let raw_block = extract_inference_block_raw(template_content);
    let mut in_output = false;
    let mut current_field_is_array = false;
    let mut current_field_has_max_items = false;

    for line in raw_block.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("  output:") {
            in_output = true;
            continue;
        }
        if in_output {
            if !trimmed.starts_with("    ") {
                in_output = false;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("    ") {
                if rest.is_empty() || rest.starts_with('#') {
                    continue;
                }
                // A new field starts — check if the previous field was an
                // unbounded array.
                if current_field_is_array && !current_field_has_max_items {
                    return true;
                }
                // Reset for the new field.
                current_field_is_array = false;
                current_field_has_max_items = false;
                // Check if this field is an array type.
                if rest.contains("array") {
                    current_field_is_array = true;
                }
            }
            // Check for max_items constraint on the current field.
            if trimmed.contains("max_items") {
                current_field_has_max_items = true;
            }
        }
    }
    // Check the last field.
    if current_field_is_array && !current_field_has_max_items {
        return true;
    }
    false
}

/// Check whether the template has high reasoning overhead — `thinking_budget`
/// set to "full" or "minimal" (not "off"), or `work_effort` set to "high".
/// These templates are at higher risk of truncation because the model spends
/// tokens on reasoning, leaving fewer tokens for the structured output.
fn has_high_reasoning_overhead(template_content: &str) -> bool {
    let after_fm = strip_front_matter(template_content);
    let (_, inference) = parse_and_strip_inference_block(after_fm);

    // thinking_budget: "full" or "minimal" means the model reasons before
    // emitting output. "off" means no reasoning overhead.
    if let Some(ref tb) = inference.thinking_budget {
        if tb == "full" || tb == "minimal" {
            return true;
        }
    }

    // work_effort is not stored in InferenceBlock — check the raw text.
    // The [inference] block after frontmatter contains `work_effort = "high"`.
    if after_fm.contains("work_effort = \"high\"") {
        return true;
    }

    false
}

/// Combined risk check: a template is at high risk of truncation if it has
/// an unbounded array output AND high reasoning overhead. This is the
/// condition that requires the elevated `max_tokens` floor.
fn has_unbounded_array_with_high_reasoning(template_content: &str) -> bool {
    has_unbounded_array_output(template_content) && has_high_reasoning_overhead(template_content)
}

fn collect_j2_files(dir: &std::path::Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_j2_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "j2") {
            files.push(path);
        }
    }
}

/// Minimum `max_tokens` for templates with unbounded array output fields
/// AND high thinking budget or high work effort. These templates are at
/// risk of truncation because the model spends tokens on reasoning, leaving
/// less for the structured output. Templates with `thinking_budget = "off"`
/// and `work_effort = "medium"` are lower risk and exempt from this floor.
const MIN_MAX_TOKENS_UNBOUNDED_ARRAY_HIGH_EFFORT: u32 = 4096;

/// Parse the `contract: output:` block from a template's `[inference]` section
/// and count the number of declared output fields. Returns 0 if no output
/// block is found.
fn count_output_fields(template_content: &str) -> usize {
    // The output block is indented under `output:` within the `contract:` section.
    // Fields are indented 4 spaces from the `output:` key.
    // We look for `output:` and count subsequent `    fieldname:` lines.
    let mut in_output = false;
    let mut count = 0;
    for line in template_content.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("  output:") {
            in_output = true;
            continue;
        }
        if in_output {
            // End of output block: a line at 2-space indent or less, or a new
            // top-level key, or the end of the inference block.
            if !trimmed.starts_with("    ") {
                in_output = false;
                continue;
            }
            // A field: `    fieldname: type`
            if let Some(rest) = trimmed.strip_prefix("    ") {
                // Skip blank lines and comments
                if rest.is_empty() || rest.starts_with('#') {
                    continue;
                }
                // Must be `fieldname:` (a key, not a continuation)
                if rest.contains(':') {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Parse the full `[inference]` block (including contract) from a template.
/// `parse_and_strip_inference_block` returns the stripped body and the parsed
/// config, but we need the raw block content to count output fields. We
/// re-extract it here using the same blank-line boundary that
/// `parse_and_strip_inference_block` uses.
fn extract_inference_block_raw(template_content: &str) -> &str {
    let marker = "[inference]";
    let start = match template_content.find(marker) {
        Some(pos) => pos,
        None => return "",
    };
    let after = &template_content[start + marker.len()..];
    // The block ends at the first blank line (\n\n), matching
    // `parse_and_strip_inference_block`'s boundary. Do NOT use \n--- as a
    // boundary — `---` appears inside template content (fences, frontmatter)
    // and would truncate the block early.
    let end = after.find("\n\n").unwrap_or(after.len());
    &template_content[start..start + marker.len() + end]
}

#[test]
fn all_templates_have_adequate_max_tokens_for_output_schema() {
    let templates = template_files();
    assert!(
        !templates.is_empty(),
        "registry templates directory must not be empty"
    );

    let exempt = non_inference_template_refs();
    let mut inadequate: Vec<(String, u32, usize)> = Vec::new();

    for path in &templates {
        // Skip templates used only by render/populate steps — they don't
        // call inference, so max_tokens is irrelevant and truncation (D25)
        // cannot occur.
        if is_non_inference_template(path, &exempt) {
            continue;
        }
        let content = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        // Strip YAML frontmatter first — the first [inference] block is the
        // frontmatter-style block (template_type, contract). The
        // second [inference] block (after frontmatter) holds the inference
        // parameters (temperature, max_tokens, thinking_budget).
        let after_fm = strip_front_matter(&content);
        let (_, inference) = parse_and_strip_inference_block(after_fm);
        let max_tokens = inference.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);

        let raw_block = extract_inference_block_raw(&content);
        let field_count = count_output_fields(raw_block);
        let has_high_risk_array = has_unbounded_array_with_high_reasoning(&content);

        // Determine the required max_tokens. The field-count heuristic
        // handles complex schemas (7+ fields). The high-risk-array check
        // catches templates with unbounded array output AND high reasoning
        // overhead (thinking_budget != "off" or work_effort = "high") —
        // these are at elevated truncation risk because the model spends
        // tokens on reasoning before emitting the structured output.
        let required = if field_count >= VERY_COMPLEX_FIELD_THRESHOLD {
            MIN_MAX_TOKENS_VERY_COMPLEX
        } else if field_count >= COMPLEX_FIELD_THRESHOLD {
            MIN_MAX_TOKENS_COMPLEX
        } else if has_high_risk_array {
            MIN_MAX_TOKENS_UNBOUNDED_ARRAY_HIGH_EFFORT
        } else {
            continue; // Simple enough — no assertion.
        };

        if max_tokens < required {
            let short = path
                .components()
                .rev()
                .take(2)
                .collect::<std::path::PathBuf>()
                .to_string_lossy()
                .to_string();
            inadequate.push((short, max_tokens, field_count));
        }
    }

    if !inadequate.is_empty() {
        let mut msg = String::from(
            "Templates with complex output schemas (7+ fields) have insufficient max_tokens:\n",
        );
        for (name, mt, fields) in &inadequate {
            msg.push_str(&format!(
                "  {name}: max_tokens={mt}, output_fields={fields} — needs >= {}\n",
                if *fields >= VERY_COMPLEX_FIELD_THRESHOLD {
                    MIN_MAX_TOKENS_VERY_COMPLEX
                } else {
                    MIN_MAX_TOKENS_COMPLEX
                }
            ));
        }
        msg.push_str(
            "\nThese templates will silently truncate at runtime — the model runs out of \
             output tokens before emitting the structured-output tool call (D25). Add \
             `max_tokens = <N>` to the [inference] block.",
        );
        panic!("{msg}");
    }
}

/// Verify that the default `max_tokens` (2048) is sufficient for simple
/// templates (0-6 output fields). This is a sanity check — if it fails, the
/// threshold logic above is wrong.
#[test]
fn default_max_tokens_is_sufficient_for_simple_templates() {
    let templates = template_files();
    let exempt = non_inference_template_refs();
    let mut violations = Vec::new();

    for path in &templates {
        if is_non_inference_template(path, &exempt) {
            continue;
        }
        let content = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let after_fm = strip_front_matter(&content);
        let (_, inference) = parse_and_strip_inference_block(after_fm);
        // Only check templates that use the default (no explicit max_tokens).
        if inference.max_tokens.is_some() {
            continue;
        }
        let raw_block = extract_inference_block_raw(&content);
        let field_count = count_output_fields(raw_block);
        let has_high_risk_array = has_unbounded_array_with_high_reasoning(&content);

        if field_count >= COMPLEX_FIELD_THRESHOLD || has_high_risk_array {
            let short = path
                .components()
                .rev()
                .take(2)
                .collect::<std::path::PathBuf>()
                .to_string_lossy()
                .to_string();
            violations.push((short, field_count));
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "Templates with 7+ output fields or unbounded array outputs have NO [inference] block (using the 2048 default):\n",
        );
        for (name, fields) in &violations {
            msg.push_str(&format!(
                "  {name}: {fields} output fields — needs an [inference] block with max_tokens >= {}\n",
                if *fields >= VERY_COMPLEX_FIELD_THRESHOLD {
                    MIN_MAX_TOKENS_VERY_COMPLEX
                } else {
                    MIN_MAX_TOKENS_COMPLEX
                }
            ));
        }
        panic!("{msg}");
    }
}
