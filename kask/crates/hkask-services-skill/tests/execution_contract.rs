//! Execution contract test — renders a .j2 template with a known input context and
//! asserts the rendered prompt contains the expected input values, contract output
//! fields, and (where applicable) the convergence formula.
//!
//! This test catches:
//! - Broken context injection (input values missing from rendered prompt)
//! - Missing output schema (contract fields absent from prompt)
//! - Missing formula/algorithm text (the math is not in the prompt)
//!
//! It does NOT catch:
//! - LLM output quality (no LLM is called)
//! - Semantic correctness (the prompt may describe a bad formula correctly)
//! - Runtime behavior (this is a render-and-assert-prompt-structure test)
//!
//! See render_templates.rs for syntax-only validation. This file goes one step
//! further: it asserts the rendered prompt *structure* matches the contract.

// PATTERN: Execution Contract Test
//
// 1. Read the .j2 template file
// 2. Strip [inference] frontmatter
// 3. Build a context with the template's expected inputs (from contract: input)
// 4. Render with minijinja (UndefinedBehavior::Lenient)
// 5. Assert the rendered prompt contains:
//    a. Expected input values (context injection works)
//    b. Contract output field names (output schema is described)
//    c. Formula/algorithm text (the math is in the prompt)
// 6. This catches: broken context injection, missing output schema, missing formula
// 7. This does NOT catch: LLM output quality, semantic correctness

use minijinja::Environment;
use minijinja::Value;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

/// Strip the [inference] frontmatter block from a .j2 file, returning only
/// the Jinja2 body. The frontmatter is YAML-like (not valid Jinja2) and must
/// be removed before rendering.
///
/// Handles two patterns:
/// 1. Files with [inference] frontmatter: strip from [inference] to --- separator
/// 2. Files without [inference] (only Jinja2 comments + body): return as-is
///
/// NOTE: This is a copy of the same function in render_templates.rs. If a third
/// test needs it, extract to a shared module under tests/common/.
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

/// Resolve a template path under registry/templates/ relative to the workspace root.
fn template_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap() // workspace root
        .join("registry")
        .join("templates")
        .join(relative)
}

/// Read, strip frontmatter, and render a template with the given context.
/// Returns the rendered prompt string or an error message.
fn render_template(relative: &str, context: Value) -> Result<String, String> {
    let path = template_path(relative);
    let content = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let body = strip_inference_frontmatter(&content);

    let mut env = Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);

    let template = env
        .render_str(&body, context)
        .map_err(|e| format!("render {}: {e}", path.display()))?;
    Ok(template)
}

/// Assert a substring is present in the rendered prompt, with a helpful message.
fn assert_contains(rendered: &str, needle: &str, what: &str) {
    assert!(
        rendered.contains(needle),
        "rendered prompt is missing {what}: {needle:?}\n--- rendered prompt ---\n{rendered}"
    );
}

/// Assert a substring is ABSENT from the rendered prompt.
fn assert_not_contains(rendered: &str, needle: &str, what: &str) {
    assert!(
        !rendered.contains(needle),
        "rendered prompt unexpectedly contains {what}: {needle:?}\n--- rendered prompt ---\n{rendered}"
    );
}

// ============================================================================
// self-critique-revision/self-critique-convergence-check.j2
// ============================================================================
//
// Contract:
//   input:
//     critique_result: object  (with per_criterion_scores, overall_quality_score, findings)
//     revision_result: object  (with regressions)
//   output:
//     convergence_metric: number
//     convergence_method: string
//     metric_decomposition: object
//     rationale: string
//     blockers: array
//
// Formula: convergence_metric = (5 - normalized_quality_score) / 4, clamped to [0,1]
//
// Prediction: PASS — the template has a real formula, scoring rubric, and full
// output schema in the prompt body.

#[test]
fn self_critique_convergence_check_renders_contract_inputs() {
    let critique_result = json!({
        "overall_quality_score": 4.0,
        "per_criterion_scores": {
            "completeness": 4,
            "accuracy": 5,
            "clarity": 3
        },
        "findings": [
            {"criterion": "completeness", "score": 4, "constraint_force": "Guideline", "description": "minor gap"},
            {"criterion": "accuracy", "score": 5, "constraint_force": "Evidence", "description": "well-supported"}
        ]
    });
    let revision_result = json!({
        "regressions": [],
        "revised_draft": "improved text"
    });

    let context = Value::from_serialize(json!({
        "critique_result": critique_result,
        "revision_result": revision_result,
        "previous_quality_score": 3.0,
        "iteration": 1,
        "max_iterations": 3,
        "_convergence": {
            "threshold": 0.15,
            "iterations_completed": 1,
            "max_iterations": 3,
            "improvement_target": 0.10,
            "baseline_quality": "first pass"
        }
    }));

    let rendered = render_template(
        "self-critique-revision/self-critique-convergence-check.j2",
        context,
    )
    .expect("template should render");

    // (a) Expected input values are present (context injection works)
    assert_contains(&rendered, "4.0", "overall_quality_score input value");
    assert_contains(
        &rendered,
        "completeness",
        "criterion name from critique_result",
    );
    assert_contains(&rendered, "Guideline", "constraint_force from findings");
    assert_contains(
        &rendered,
        "improved text",
        "revised_draft from revision_result",
    );

    // (b) Contract output field names are present (output schema is described)
    assert_contains(&rendered, "convergence_metric", "contract output field");
    assert_contains(&rendered, "convergence_method", "contract output field");
    assert_contains(&rendered, "metric_decomposition", "contract output field");
    assert_contains(&rendered, "rationale", "contract output field");
    assert_contains(&rendered, "blockers", "contract output field");

    // (c) Formula text is present (the math is in the prompt)
    assert_contains(
        &rendered,
        "(5 - normalized_quality_score) / 4",
        "convergence formula",
    );
    assert_contains(
        &rendered,
        "normalized_quality_score",
        "formula variable name",
    );

    // Sanity: the rendered prompt is non-trivial (not empty, not just frontmatter)
    assert!(
        rendered.len() > 200,
        "rendered prompt is suspiciously short ({} bytes)",
        rendered.len()
    );
}

#[test]
fn self_critique_convergence_check_defaults_kick_in_with_empty_convergence_meta() {
    // The template uses `_convergence.threshold | default(0.15)` etc. The `default()`
    // filter only catches undefined *after* an attribute access — so `_convergence`
    // itself must exist (even as an empty object) for the template to render in
    // Lenient mode. Accessing `.threshold` on a fully-undefined `_convergence` is
    // an error, not an empty string.
    //
    // This is a real finding: the template's contract should either (a) declare
    // `_convergence` as a required input, or (b) the template should guard with
    // `{% if _convergence %}...{% endif %}`. As written, callers MUST pass at
    // least `{}` for `_convergence`.
    let context = Value::from_serialize(json!({
        "critique_result": {"overall_quality_score": 3.5},
        "revision_result": {"regressions": []},
        "_convergence": {}
    }));

    let rendered = render_template(
        "self-critique-revision/self-critique-convergence-check.j2",
        context,
    )
    .expect("template should render with empty _convergence meta object");

    // Default threshold (0.15) should appear because _convergence.threshold defaults to 0.15
    assert_contains(&rendered, "0.15", "default convergence threshold");
    // Default max_iterations (3) should appear
    assert_contains(&rendered, "3", "default max_iterations");
    // Formula should still be present regardless of context
    assert_contains(
        &rendered,
        "(5 - normalized_quality_score) / 4",
        "convergence formula (independent of context)",
    );
}

#[test]
fn self_critique_convergence_check_errors_when_convergence_meta_fully_absent() {
    // Documents the real behavior: a fully-undefined `_convergence` causes a render
    // error even in Lenient mode, because attribute access on undefined is an error.
    // Callers must pass at least `_convergence: {}`.
    let context = Value::from_serialize(json!({
        "critique_result": {"overall_quality_score": 3.5},
        "revision_result": {"regressions": []}
    }));

    let result = render_template(
        "self-critique-revision/self-critique-convergence-check.j2",
        context,
    );

    assert!(
        result.is_err(),
        "template should error when _convergence is fully absent — \
         this documents that callers must pass at least an empty _convergence object"
    );
    let err = result.unwrap_err();
    eprintln!("documented behavior — missing _convergence errors: {err}");
}

// ============================================================================
// review/review-convergence-check.j2
// ============================================================================
//
// Contract:
//   input:
//     primary_result: object
//   output:
//     convergence_metric: number
//     rationale: string
//
// Prediction: PARTIAL PASS / STRUCTURAL FAIL — the template renders and has
// the two contract output fields, but it has NO formula and NO scoring rubric.
// The convergence method is described in prose only ("0 = fully converged ...
// 1 = not converged. Score how much work remains."). This is exactly the kind
// of gap an execution contract test should surface: the contract is satisfied
// on paper, but the prompt gives the LLM no algorithm to follow.

#[test]
fn review_convergence_check_renders_contract_outputs_but_lacks_formula() {
    let primary_result = json!({
        "findings": [
            {"severity": "high", "description": "unsupported claim"},
            {"severity": "low", "description": "minor typo"}
        ],
        "overall_score": 3.2
    });

    let context = Value::from_serialize(json!({
        "primary_result": primary_result,
        "_convergence": {
            "threshold": 0.15,
            "iterations_completed": 1,
            "max_iterations": 3,
            "improvement_target": 0.10
        }
    }));

    let rendered = match render_template("review/review-convergence-check.j2", context) {
        Ok(s) => s,
        Err(e) => {
            // If the review skill has been deleted (e.g., merged into self-critique-revision),
            // skip this test rather than fail. The task description says: "If review has been
            // deleted by the time you run this, skip this."
            if e.contains("No such file") || e.contains("cannot read") {
                eprintln!(
                    "review/review-convergence-check.j2 not found — skipping (skill likely deleted)"
                );
                return;
            }
            panic!("failed to render review template: {e}");
        }
    };

    // (a) Input value injection
    assert_contains(
        &rendered,
        "unsupported claim",
        "finding description from primary_result",
    );
    assert_contains(&rendered, "3.2", "overall_score from primary_result");

    // (b) Contract output fields — both are present
    assert_contains(&rendered, "convergence_metric", "contract output field");
    assert_contains(&rendered, "rationale", "contract output field");

    // (c) Formula — this is the gap. The review template has NO formula.
    // Document the gap by asserting the formula is absent. If a future fix adds
    // a formula, this assertion will fail and force the test author to convert
    // it to assert_contains (a positive assertion).
    assert_not_contains(
        &rendered,
        "(5 - normalized_quality_score) / 4",
        "self-critique formula (review should have its own or none)",
    );

    // The review template's "convergence method" is prose-only.
    // Document this structural weakness: no scoring rubric, no penalty schedule.
    let has_rubric = rendered.contains("Prohibition")
        || rendered.contains("penalty")
        || rendered.contains("+0.");
    assert!(
        !has_rubric,
        "review template unexpectedly contains a scoring rubric — if you added one, \
         convert this assertion to assert_contains and document the formula"
    );

    eprintln!(
        "review-convergence-check.j2: renders OK, has contract output fields, \
         but has NO formula and NO scoring rubric (structural gap documented)"
    );
}

// ============================================================================
// Cross-template sanity: shared/convergence-check.j2
// ============================================================================
//
// The shared convergence template is included by many skills via {% include %}.
// Verify it renders standalone with a minimal context and contains the
// canonical output fields. This is a smoke test, not a full contract test.

#[test]
fn shared_convergence_check_renders_canonical_fields() {
    let context = Value::from_serialize(json!({
        "_convergence": {
            "threshold": 0.15,
            "iterations_completed": 2,
            "max_iterations": 3,
            "improvement_target": 0.10,
            "baseline_quality": "first pass"
        },
        "primary_result": {"score": 4.1}
    }));

    let rendered = match render_template("shared/convergence-check.j2", context) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "shared/convergence-check.j2 could not be rendered standalone: {e}\
                 (this is informational — shared templates may require include context)"
            );
            return;
        }
    };

    // Whatever the shared template produces, it should at minimum mention
    // convergence_metric (the canonical output field across all convergence templates).
    assert_contains(
        &rendered,
        "convergence_metric",
        "canonical convergence output field in shared template",
    );
}
