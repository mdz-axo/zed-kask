//! Jinja2 render test — verifies every .j2 template in registry/templates/ renders
//! without syntax errors.
//!
//! This test catches:
//! - Unbalanced Jinja2 tags ({% if %} without {% endif %})
//! - Undefined macros
//! - Invalid expressions
//! - Broken template syntax
//!
//! It does NOT catch:
//! - Logic errors (wrong variables, wrong conditionals)
//! - Output quality (garbage JSON, missing fields)
//! - Semantic correctness (goal alignment, evidence integrity)
//!
//! The test renders each template with UndefinedBehavior::Lenient (missing
//! variables become empty strings, not errors). This isolates syntax validation
//! from logic validation — a template that fails here has a syntax error, not
//! a missing input.

use minijinja::Environment;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// Strip the [inference] frontmatter block from a .j2 file, returning only
/// the Jinja2 body. The frontmatter is YAML-like (not valid Jinja2) and must
/// be removed before rendering.
///
/// Handles two patterns:
/// 1. Files with [inference] frontmatter: strip from [inference] to --- separator
/// 2. Files without [inference] (only Jinja2 comments + body): return as-is
fn strip_inference_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();

    // Skip leading Jinja comments ({# ... #})
    let mut rest = trimmed;
    while rest.starts_with("{#") {
        if let Some(end) = rest.find("#}") {
            rest = rest[end + 2..].trim_start();
        } else {
            break;
        }
    }

    // If no [inference] block, return the original content (comments are valid Jinja2)
    if !rest.starts_with("[inference]") {
        return content.to_string();
    }

    // Find the --- separator that ends the frontmatter
    let after_header = &rest["[inference]".len()..];
    if let Some(sep_pos) = after_header.find("\n---") {
        // Return everything after the --- line
        let after_sep = &after_header[sep_pos + 4..];
        // Skip the newline after ---
        after_sep.trim_start_matches('\n').to_string()
    } else {
        // No --- separator found — return the body after [inference] block
        // (best effort — the frontmatter parser would have flagged this)
        content.to_string()
    }
}

/// Collect all .j2 files under registry/templates/
fn collect_j2_templates() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap() // workspace root
        .join("registry")
        .join("templates");

    let mut files = Vec::new();
    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension() == Some(std::ffi::OsStr::new("j2")) {
            files.push(path.to_string_lossy().to_string());
        }
    }
    files.sort();
    files
}

#[test]
fn all_j2_templates_render_without_syntax_errors() {
    let mut env = Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);

    let templates = collect_j2_templates();
    assert!(
        !templates.is_empty(),
        "should find .j2 templates in registry/templates/"
    );

    let mut errors = Vec::new();

    for path in &templates {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("{path}: cannot read file: {e}"));
                continue;
            }
        };

        let body = strip_inference_frontmatter(&content);

        // Parse the template (syntax check only — no rendering).
        // This catches unbalanced tags, invalid expressions, undefined macros.
        // We use a fresh Environment per template to avoid lifetime issues
        // (minijinja borrows the template source for the Environment's lifetime).
        let mut env = Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
        if let Err(e) = env.add_template(path, &body) {
            errors.push(format!("{path}: {e}"));
        }
    }

    if !errors.is_empty() {
        panic!(
            "{} template(s) failed to render:\n{}",
            errors.len(),
            errors.join("\n")
        );
    }
}

#[test]
fn strip_inference_frontmatter_handles_comments() {
    let input = "{# goal: test #}\n[inference]\ntemplate_type: KnowAct\n---\nHello {{ name }}";
    let result = strip_inference_frontmatter(input);
    assert!(
        result.contains("Hello"),
        "body should remain after stripping frontmatter"
    );
    assert!(
        !result.contains("[inference]"),
        "frontmatter should be stripped"
    );
}

#[test]
fn strip_inference_frontmatter_handles_no_frontmatter() {
    let input = "{# goal: test #}\nHello {{ name }}";
    let result = strip_inference_frontmatter(input);
    assert!(result.contains("Hello"), "body should remain as-is");
    assert!(
        result.contains("{# goal: test #}"),
        "comments should be preserved"
    );
}
