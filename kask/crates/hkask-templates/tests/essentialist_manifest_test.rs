//! Manifest compliance test for the essentialist skill's lisp.eval
//! convergence step. Pins the top-level `listp` guard.
//!
//! Regression history: the form called `(assoc "elimination_report"
//! step_1_result)` without checking the argument type first. When the step-1
//! LLM output was not a JSON object (markdown-wrapped JSON, prose, or an
//! error envelope), `step_1_result` arrived as a String and `assoc` errored
//! with "type error: expected list, got string" — failing the whole cascade.
//! The skill could never run; the manual fallback was to apply the 3-gate
//! methodology by hand.
//!
//! The fix follows the established pattern (company-research-deep GORILLA step,
//! upstream-rebase verification step): `(if (not (listp step_1_result)) 0 ...)`
//! — the stable 0 reading the manifest documents ("lets Cauchy fire after
//! min_iterations"), same contract as the nested `is_null` guards.

use hkask_lisp::eval_sandboxed;
use hkask_templates::load_manifest_from_yaml;
use serde_json::json;
use std::path::Path;

fn registry_manifests_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests")
}

fn load_named_manifest(name: &str) -> hkask_templates::BundleManifest {
    let path = registry_manifests_dir().join(format!("{name}.yaml"));
    let yaml = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {name}.yaml at {}: {e}", path.display()));
    load_manifest_from_yaml(&yaml).unwrap_or_else(|e| panic!("{name}.yaml must parse: {e:?}"))
}

fn extract_lisp_form(manifest: &hkask_templates::BundleManifest, ordinal: u32) -> String {
    let step = manifest
        .steps
        .iter()
        .find(|s| s.ordinal == ordinal)
        .unwrap_or_else(|| panic!("step {} not found", ordinal));
    let input = step
        .input_mapping
        .as_ref()
        .expect("compute step has input_mapping");
    let form = input
        .get("form")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("step {} input_mapping has no form string", ordinal));
    form.to_string()
}

/// The manifest declares a lisp.eval compute step at ordinal 2 whose form
/// guards EVERY hop of the nested lookup via the recursive `walk` helper
/// (data-as-program: the path is a list, the walker checks `listp` per hop).
/// A top-level-only guard would still crash on a string at a deeper level
/// if the form ever grows; the walker makes that impossible by construction.
#[test]
fn essentialist_convergence_step_guards_every_hop() {
    let manifest = load_named_manifest("essentialist");
    let form = extract_lisp_form(&manifest, 2);
    assert!(
        form.contains("(listp data)"),
        "the walker must type-check each hop before assoc — without it the \
         cascade fails with 'type error: expected list, got string' when any \
         level of step_1_result is not a JSON object"
    );
    assert!(
        form.contains("walk (cdr keys)"),
        "the walker must recurse on the remaining path — a non-recursive form \
         cannot express the 3-level elimination_report path"
    );
}

/// Regression: step_1_result arrives as a STRING (the exact failure that broke
/// the skill in live use). The form must return 0, not error.
#[test]
fn essentialist_lisp_form_handles_string_step_1_result() {
    let manifest = load_named_manifest("essentialist");
    let form = extract_lisp_form(&manifest, 2);
    let env = json!({
        "step_1_result": "## Elimination Report\n\nThe artifact survived G1..."
    });
    let result = eval_sandboxed(&form, &env).expect("form must not error on string step_1_result");
    assert_eq!(result, json!(0), "string output must read as signal 0");
}

/// Scalars (float, boolean) must also read as 0 — same guard.
#[test]
fn essentialist_lisp_form_handles_scalar_step_1_result() {
    let manifest = load_named_manifest("essentialist");
    let form = extract_lisp_form(&manifest, 2);
    for scalar in [json!(42.0), json!(true)] {
        let env = json!({ "step_1_result": scalar });
        let result =
            eval_sandboxed(&form, &env).expect("form must not error on scalar step_1_result");
        assert_eq!(result, json!(0), "scalar output must read as signal 0");
    }
}

/// Happy path: a well-formed report extracts items_removed.
#[test]
fn essentialist_lisp_form_extracts_items_removed() {
    let manifest = load_named_manifest("essentialist");
    let form = extract_lisp_form(&manifest, 2);
    let env = json!({
        "step_1_result": {
            "elimination_report": {
                "essentialism_score": {
                    "items_removed": 7,
                    "total_items": 15,
                    "percentage": 46.7
                }
            }
        }
    });
    let result = eval_sandboxed(&form, &env).expect("form must not error on object step_1_result");
    assert_eq!(result, json!(7), "items_removed is the convergence signal");
}

/// The nested is_null guards: absent elimination_report / essentialism_score /
/// items_removed each read as 0 (the documented stable reading).
#[test]
fn essentialist_lisp_form_returns_zero_for_missing_nested_fields() {
    let manifest = load_named_manifest("essentialist");
    let form = extract_lisp_form(&manifest, 2);
    for env in [
        json!({ "step_1_result": {} }),
        json!({ "step_1_result": { "elimination_report": {} } }),
        json!({ "step_1_result": { "elimination_report": { "essentialism_score": {} } } }),
    ] {
        let result = eval_sandboxed(&form, &env)
            .expect("form must not error on partially-populated step_1_result");
        assert_eq!(
            result,
            json!(0),
            "missing nested field must read as signal 0"
        );
    }
}
