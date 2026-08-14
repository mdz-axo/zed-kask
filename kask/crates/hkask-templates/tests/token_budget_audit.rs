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
//! `TemplateError::Manifest("Step N truncated at max_tokens...")` (D25). This
//! is a runtime failure that's invisible at manifest-load time.
//!
//! # Principle grounding
//! - P1 (Correctness): the declared `max_tokens` must be sufficient for the
//!   declared output schema — an advertised contract with an insufficient
//!   budget is a silent failure waiting to happen.
//! - P4 (Clear Boundaries): the budget boundary must be adequate for the
//!   output boundary; a too-low `max_tokens` makes the output boundary
//!   unreachable.

use hkask_templates::test_utils::parse_and_strip_inference_block;
use std::fs;
use std::path::PathBuf;

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
/// re-extract it here.
fn extract_inference_block_raw(template_content: &str) -> &str {
    let marker = "[inference]";
    let start = match template_content.find(marker) {
        Some(pos) => pos,
        None => return "",
    };
    let after = &template_content[start + marker.len()..];
    let end = after
        .find("\n---")
        .unwrap_or(after.find("\n\n").unwrap_or(after.len()));
    &template_content[start..start + marker.len() + end]
}

#[test]
fn all_templates_have_adequate_max_tokens_for_output_schema() {
    let templates = template_files();
    assert!(
        !templates.is_empty(),
        "registry templates directory must not be empty"
    );

    let mut inadequate: Vec<(String, u32, usize)> = Vec::new();

    for path in &templates {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let (_, inference) = parse_and_strip_inference_block(&content);
        let max_tokens = inference.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);

        let raw_block = extract_inference_block_raw(&content);
        let field_count = count_output_fields(raw_block);

        let required = if field_count >= VERY_COMPLEX_FIELD_THRESHOLD {
            MIN_MAX_TOKENS_VERY_COMPLEX
        } else if field_count >= COMPLEX_FIELD_THRESHOLD {
            MIN_MAX_TOKENS_COMPLEX
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
    let mut violations = Vec::new();

    for path in &templates {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let (_, inference) = parse_and_strip_inference_block(&content);
        // Only check templates that use the default (no explicit max_tokens).
        if inference.max_tokens.is_some() {
            continue;
        }
        let raw_block = extract_inference_block_raw(&content);
        let field_count = count_output_fields(raw_block);

        if field_count >= COMPLEX_FIELD_THRESHOLD {
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
            "Templates with 7+ output fields have NO [inference] block (using the 2048 default):\n",
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
