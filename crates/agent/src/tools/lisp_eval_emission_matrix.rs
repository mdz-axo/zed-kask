//! Emission-variant matrix for the `lisp_eval` tool input surface
//! (lisp-repair L0 failure-class inventory).
//!
//! Every variant here is a shape a temperature-0 model has been observed or
//! is plausibly able to emit for `env` / `form` / the budget fields, driven
//! through the REAL deserialization path (`serde_json::from_value` — which
//! exercises `deserialize_maybe_stringified` via the serde attribute) and
//! then `evaluate_lisp`. Each test pins today's outcome class; the exact
//! current message is carried in the assertion so L1's
//! teach-the-next-emission improvements have a before/after receipt.
//!
//! Failure classes (lisp-repair contract): `refuse` = typed rejection
//! (acceptable when actionable), `lie` = silent wrong answer (never
//! acceptable), `hang` = budget miss surfaced as a typed error (acceptable),
//! `crash` = panic (never acceptable), `lockout` = framework refusal of a
//! corrected/varying input (never acceptable). The `unteachable` label marks
//! the worst refuse sub-class: a shape that deserializes cleanly but leaves
//! the form's bindings missing, producing an error that does not name the
//! actual defect — the anatomy of the 2026-09-28 audit lockout (5 identical
//! malformed-env retries hard-refused by the per-input tracker).

use serde_json::{Value, json};

use super::lisp_eval_tool::{LispEvalToolInput, evaluate_lisp};

/// The real path: deserialize the tool arguments, then evaluate.
fn run(args: Value) -> Result<Value, String> {
    let input: LispEvalToolInput =
        serde_json::from_value(args).map_err(|e| format!("deserialize: {e}"))?;
    evaluate_lisp(input)
}

// ── env: tolerated variants ──────────────────────────────────────────────────

#[test]
fn env_bare_object_is_tolerated() {
    assert_eq!(
        run(json!({"form": "(+ a 1)", "env": {"a": 1}})),
        Ok(json!(2))
    );
}

#[test]
fn env_stringified_object_is_tolerated() {
    // deserialize_maybe_stringified: models that emit env as a stringified
    // JSON string get the parsed object.
    assert_eq!(
        run(json!({"form": "(+ a 1)", "env": "{\"a\": 1}"})),
        Ok(json!(2))
    );
}

#[test]
fn env_stringified_with_surrounding_whitespace_is_tolerated() {
    assert_eq!(
        run(json!({"form": "(+ a 1)", "env": "  {\"a\": 1}  "})),
        Ok(json!(2))
    );
}

#[test]
fn env_missing_defaults_to_empty() {
    // #[serde(default)]: a form with no bindings runs against an empty env.
    assert_eq!(run(json!({"form": "(+ 1 2)"})), Ok(json!(3)));
}

// ── env: refused variants (typed errors — the message is the specimen) ──────

#[test]
fn env_null_is_refused() {
    // today: "deserialize: invalid type: null, expected a map" — the error
    // does NOT name the field (`env`). L1 fix target: name field + accepted
    // shape (the "deserialize: " prefix is this harness's; the model-visible
    // message is the tool framework's rendering of the same serde error).
    let out = run(json!({"form": "(+ 1 2)", "env": null}));
    assert_eq!(
        out,
        Err("deserialize: invalid type: null, expected a map".to_string())
    );
}

#[test]
fn env_array_is_refused() {
    // today: "deserialize: invalid type: sequence, expected a map"
    let out = run(json!({"form": "(+ 1 2)", "env": [["a", 1]]}));
    assert_eq!(
        out,
        Err("deserialize: invalid type: sequence, expected a map".to_string())
    );
}

#[test]
fn env_number_is_refused() {
    // today: "deserialize: invalid type: integer `5`, expected a map"
    let out = run(json!({"form": "(+ 1 2)", "env": 5}));
    assert_eq!(
        out,
        Err("deserialize: invalid type: integer `5`, expected a map".to_string())
    );
}

#[test]
fn env_double_stringified_is_refused() {
    // The string parses to a JSON string, not a map.
    // today: "deserialize: env: invalid type: string \"{\\\"a\\\": 1}\", expected a map"
    let out = run(json!({"form": "(+ a 1)", "env": "\"{\\\"a\\\": 1}\""}));
    assert!(matches!(out, Err(e) if e.contains("invalid type") && e.contains("expected a map")));
}

#[test]
fn env_invalid_json_string_is_refused() {
    // today: "deserialize: env: key must be a string at line 1 column 2" (serde_json parse error)
    let out = run(json!({"form": "(+ a 1)", "env": "{a: 1}"}));
    assert!(out.is_err());
}

#[test]
fn env_python_repr_string_is_refused() {
    // Python-style single quotes are not JSON — a common temperature-0
    // emission class for models trained on repr() output.
    // today: "deserialize: env: key must be a string at line 1 column 1"
    let out = run(json!({"form": "(+ a 1)", "env": "{'a': 1}"}));
    assert!(out.is_err());
}

#[test]
fn env_stringified_array_is_refused() {
    // Parses to a JSON array, not a map.
    let out = run(json!({"form": "(+ 1 2)", "env": "[1, 2]"}));
    assert!(
        matches!(out, Err(e) if e.contains("invalid type: sequence") && e.contains("expected a map"))
    );
}

// ── env: the unteachable class (deserializes, then fails without teaching) ──

#[test]
fn env_nested_under_env_key_is_unteachable() {
    // The model wraps env one level too deep: the tool arguments carry
    // {"env": {"env": {...}}}. Deserialization SUCCEEDS — the inner object
    // becomes a single binding named "env" — and the form fails with an
    // error that does not name the nesting. This is the anatomy of the
    // 2026-09-28 audit lockout: the error does not change the next
    // emission, so the model retries the identical shape.
    // today: "unbound symbol: a" — no mention of the stray "env" binding.
    // L1 fix target: this error must teach the next emission (name the
    // received env keys and the accepted shape).
    let out = run(json!({"form": "(+ a 1)", "env": {"env": {"a": 1}}}));
    assert_eq!(out, Err("unbound symbol: a".to_string()));
}

// ── form variants ───────────────────────────────────────────────────────────

#[test]
fn form_non_string_is_refused() {
    // today: "deserialize: invalid type: integer `5`, expected a string" —
    // no field name (a model that got both form and env wrong cannot tell
    // which). L1 fix target: name the field.
    let out = run(json!({"form": 5, "env": {}}));
    assert_eq!(
        out,
        Err("deserialize: invalid type: integer `5`, expected a string".to_string())
    );
}

#[test]
fn form_empty_evaluates_to_null() {
    // Documented behavior (hkask_lisp.rs:1746-1748): an empty program
    // returns JSON null, not an error.
    assert_eq!(run(json!({"form": "", "env": {}})), Ok(Value::Null));
}

#[test]
fn form_whitespace_only_evaluates_to_null() {
    assert_eq!(run(json!({"form": "   ", "env": {}})), Ok(Value::Null));
}

#[test]
fn form_unbalanced_parens_is_refused() {
    let out = run(json!({"form": "(+ 1 2", "env": {}}));
    assert!(matches!(out, Err(e) if e.contains("parse error")));
}

// ── budget fields ───────────────────────────────────────────────────────

/// A depth-dominant form: nested `(+ 1 ...)` 4000 deep. Evaluation descends
/// one frame per level, so the default max_depth (1024) trips before the
/// step budget (~3 steps/level ≈ 12k). Used to make the budget fields'
/// application OBSERVABLE.
fn nested_add_form(levels: usize) -> String {
    format!("{}1{}", "(+ 1 ".repeat(levels), ")".repeat(levels))
}

#[test]
fn budget_camelcase_max_depth_is_silently_ignored() {
    // A model emitting camelCase maxDepth (an unknown field to serde) gets
    // the snake_case default silently: the explicit budget never applies.
    // Observable via the depth-dominant form: the eval fails with
    // DepthLimitExceeded(1024) — the DEFAULT, not the emitted 8192 — and
    // the error names the limit but not the ignored field. Recorded as a
    // silent-ignore variant; L1 decides teach-vs-tolerate
    // (deny_unknown_fields would make it a typed refusal; a serde alias
    // would tolerate it).
    let out = run(json!({
        "form": nested_add_form(4000),
        "env": {},
        "maxDepth": 8192
    }));
    assert_eq!(
        out,
        Err(
            "Lisp exceeded max_depth (1024) — input nesting or evaluation recursion is too deep"
                .to_string()
        )
    );
}

#[test]
fn budget_snake_case_max_depth_is_applied() {
    // The control for the camelCase test: the snake_case field name works —
    // the same 4000-deep form evaluates to 4001 under max_depth 8192.
    let out = run(json!({
        "form": nested_add_form(4000),
        "env": {},
        "max_depth": 8192
    }));
    assert_eq!(out, Ok(json!(4001)));
}

#[test]
fn budget_string_value_is_refused() {
    // today: "deserialize: invalid type: string \"100000\", expected u64" —
    // no field name. L1 fix target: name the field.
    let out = run(json!({"form": "(+ 1 2)", "env": {}, "max_steps": "100000"}));
    assert_eq!(
        out,
        Err("deserialize: invalid type: string \"100000\", expected u64".to_string())
    );
}

#[test]
fn step_cost_anomaly_walker_exceeds_default_steps() {
    // L3 measurement target, pinned so it cannot be lost: a 600-element
    // list walker exceeds the DEFAULT 100,000-step budget (observed
    // StepLimitExceeded(100000) — ≥167 steps/element) even when max_depth
    // is raised. The dialect doc's own guidance says raise max_depth for
    // large lists; the step budget, not depth, is the binding constraint
    // at this scale. L3 benches quantify the per-element step cost; if the
    // accounting is inflated, this is the reproducing test for the fix.
    let form =
        "(define count (lambda (lst) (if (is_null lst) 0 (+ 1 (count (cdr lst)))))) (count items)";
    let items: Vec<i32> = (0..600).collect();
    let out = run(json!({"form": form, "env": {"items": items}, "max_depth": 8192}));
    assert_eq!(
        out,
        Err(
            "Lisp work exceeded max_steps (100000) — input, computation, or output is too large"
                .to_string()
        )
    );
}
