//! Emission-variant matrix for the `lisp_eval` tool input surface
//! (lisp-repair L0 failure-class inventory).
//!
//! Every variant here is a shape a temperature-0 model has been observed or
//! is plausibly able to emit for `env` / `form` / the budget fields, driven
//! through the REAL deserialization path (`serde_json::from_value` — which
//! exercises `deserialize_env_field` / the `deserialize_teaching_field` core
//! via the serde attribute) and then `evaluate_lisp`. Each test pins the L1
//! outcome: the messages are the L1 TEACHING messages (field + received shape
//! + accepted shape); each comment carries the pre-L1 message as the
//! before/after receipt (lisp-repair L1: an error that does not teach
//! produces identical retries — the 2026-09-28 audit lockout anatomy).
//!
//! Failure classes (lisp-repair contract): `refuse` = typed rejection
//! (acceptable when actionable), `lie` = silent wrong answer (never
//! acceptable), `hang` = budget miss surfaced as a typed error (acceptable),
//! `crash` = panic (never acceptable), `lockout` = framework refusal of a
//! corrected/varying input (never acceptable). The `unteachable` label marks
//! the worst refuse sub-class: a shape that deserializes cleanly but leaves
//! the form's bindings missing, producing an error that does not name the
//! actual defect — the anatomy of the 2026-09-28 audit lockout (5 identical
//! malformed-env retries hard-refused by the per-input tracker). L1 closed
//! the unteachable class: the unbound-symbol error now names the available
//! bindings and the flatten fix.

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
    // deserialize_env_field: models that emit env as a stringified
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
    // before L1: "deserialize: invalid type: null, expected a map" — no field
    // name. After L1: the teaching message (field + received + accepted).
    let out = run(json!({"form": "(+ 1 2)", "env": null}));
    assert_eq!(
        out,
        Err("deserialize: env: received null, expected a JSON object like {\"binding\": value} (a stringified JSON object is also accepted)".to_string())
    );
}

#[test]
fn env_array_is_refused() {
    // before L1: "deserialize: invalid type: sequence, expected a map"
    let out = run(json!({"form": "(+ 1 2)", "env": [["a", 1]]}));
    assert_eq!(
        out,
        Err("deserialize: env: received an array, expected a JSON object like {\"binding\": value} (a stringified JSON object is also accepted)".to_string())
    );
}

#[test]
fn env_number_is_refused() {
    // before L1: "deserialize: invalid type: integer `5`, expected a map"
    let out = run(json!({"form": "(+ 1 2)", "env": 5}));
    assert_eq!(
        out,
        Err("deserialize: env: received number 5, expected a JSON object like {\"binding\": value} (a stringified JSON object is also accepted)".to_string())
    );
}

#[test]
fn env_double_stringified_is_refused() {
    // The string parses to a JSON string, not a map. before L1 the message
    // carried position noise ("at line 1 column 12"); after L1 the teaching
    // message shows the received string — no position noise.
    let out = run(json!({"form": "(+ a 1)", "env": "\"{\\\"a\\\": 1}\""}));
    assert!(
        matches!(out, Err(e) if e.contains("env: received string") && e.contains("expected a JSON object"))
    );
}

#[test]
fn env_invalid_json_string_is_refused() {
    // before L1: "deserialize: key must be a string at line 1 column 2"
    // (position noise, no field name). After L1: the teaching message shows
    // the model its own bytes.
    let out = run(json!({"form": "(+ a 1)", "env": "{a: 1}"}));
    assert_eq!(
        out,
        Err("deserialize: env: received string \"{a: 1}\", expected a JSON object like {\"binding\": value} (a stringified JSON object is also accepted)".to_string())
    );
}

#[test]
fn env_python_repr_string_is_refused() {
    // Python-style single quotes are not JSON — a common temperature-0
    // emission class for models trained on repr() output.
    // before L1: "deserialize: key must be a string at line 1 column 1"
    let out = run(json!({"form": "(+ a 1)", "env": "{'a': 1}"}));
    assert_eq!(
        out,
        Err("deserialize: env: received string \"{'a': 1}\", expected a JSON object like {\"binding\": value} (a stringified JSON object is also accepted)".to_string())
    );
}

#[test]
fn env_stringified_array_is_refused() {
    // Parses to a JSON array, not a map.
    // before L1: "deserialize: invalid type: sequence, expected a map at line 1 column 0"
    let out = run(json!({"form": "(+ 1 2)", "env": "[1, 2]"}));
    assert_eq!(
        out,
        Err("deserialize: env: received string \"[1, 2]\", expected a JSON object like {\"binding\": value} (a stringified JSON object is also accepted)".to_string())
    );
}

// ── env: the formerly-unteachable class (L1 closed it) ─────────────────────

#[test]
fn env_nested_under_env_key_error_teaches_the_flatten_fix() {
    // The model wraps env one level too deep: the tool arguments carry
    // {"env": {"env": {...}}}. Deserialization SUCCEEDS — the inner object
    // becomes a single binding named "env" — and pre-L1 the form failed with
    // "unbound symbol: a", an error that did not name the nesting: the anatomy
    // of the 2026-09-28 audit lockout (5 identical retries). L1: the
    // unbound-symbol error names the available bindings and the flatten fix,
    // so the first error changes the next emission.
    let out = run(json!({"form": "(+ a 1)", "env": {"env": {"a": 1}}}));
    assert_eq!(
        out,
        Err("unbound symbol: a — env bindings: [\"env\"] (an env binding named \"env\" was received — if env was nested one level too deep, flatten it: {\"a\": 1}, not {\"env\": {\"a\": 1}})".to_string())
    );
}

#[test]
fn unbound_symbol_with_empty_env_names_the_empty_binding_list() {
    // The teaching generalizes: any unbound symbol names the available
    // bindings (sorted — byte-identical across processes), so an empty env
    // is visibly empty.
    let out = run(json!({"form": "(+ a 1)", "env": {}}));
    assert_eq!(out, Err("unbound symbol: a — env bindings: []".to_string()));
}

// ── form variants ───────────────────────────────────────────────────────────

#[test]
fn form_non_string_is_refused() {
    // before L1: "deserialize: invalid type: integer `5`, expected a string"
    // — no field name (a model that got both form and env wrong could not
    // tell which). After L1: the teaching message.
    let out = run(json!({"form": 5, "env": {}}));
    assert_eq!(
        out,
        Err("deserialize: form: received number 5, expected a string containing the Lisp form, e.g. \"(+ 1 2)\"".to_string())
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
fn budget_camelcase_max_depth_alias_is_tolerated() {
    // L1 decision (operator-vetoable): camelCase budget fields are tolerated
    // via serde aliases, so an explicit budget is APPLIED rather than silently
    // dropped. Pre-L1 this was a silent-ignore: the camelCase field was an
    // unknown field, serde dropped it, and the eval failed with the DEFAULT
    // limit while the model believed it had raised it — an unteachable
    // failure. The alias makes the emission work as intended; the snake_case
    // control below pins the canonical path.
    let out = run(json!({
        "form": nested_add_form(4000),
        "env": {},
        "maxDepth": 8192
    }));
    assert_eq!(out, Ok(json!(4001)));
}

#[test]
fn unknown_junk_fields_are_tolerated() {
    // The complement of the alias decision: unknown fields stay IGNORED (no
    // deny_unknown_fields), so a model adding junk (e.g. a "reasoning" field)
    // is not hard-refused — the camelCase aliases cover exactly the fields
    // with semantic effect.
    let out = run(json!({"form": "(+ 1 2)", "env": {}, "reasoning": "just checking"}));
    assert_eq!(out, Ok(json!(3)));
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
    // before L1: "deserialize: invalid type: string \"100000\", expected u64"
    // — no field name. After L1: the teaching message.
    let out = run(json!({"form": "(+ 1 2)", "env": {}, "max_steps": "100000"}));
    assert_eq!(
        out,
        Err("deserialize: max_steps: received string \"100000\", expected a non-negative integer (default 100000)".to_string())
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

// ── lie-class specimens (evaluator semantics — L2's fix flips these) ──────
// Found by the L0 contract critic (session 1deaa9ab-f425-4a94-bb76-60c1c23928f6),
// live-falsified. Each assertion pins TODAY's wrong or degraded behavior so
// the L2 fix has a red-first baseline; the fix flips the assertion to the
// typed-error (or operator-ruled) expectation. Naming: `lie_specimen_*` =
// lie-class findings (2, matching the contract's lie count);
// `degradation_specimen_*` = the defensive-degradation family (grouped for
// the operator's assoc ruling, NOT counted as lies).

#[test]
fn lie_specimen_integer_overflow_wraps_silently() {
    // DEFECT SPECIMEN (L2 fix target: checked arithmetic → typed error).
    // Today the engine wraps silently — a wrong answer with no error:
    //   (+ 9223372036854775807 1) → -9223372036854775808
    // (wrapping_add hkask_lisp.rs:899, wrapping_sub :944, wrapping_mul :964,
    // wrapping_abs :1411 — (abs -9223372036854775808) stays negative).
    let out = run(json!({"form": "(+ 9223372036854775807 1)", "env": {}}));
    assert_eq!(out, Ok(json!(-9223372036854775808i64)));
}

#[test]
fn lie_specimen_f64_comparison_silently_miscompares_big_ints() {
    // DEFECT SPECIMEN (L2 fix target: integer-exact comparison).
    // Comparisons coerce to f64 (as_f64, hkask_lisp.rs:867-876): every
    // integer above 2^53 compares wrongly, silently:
    //   (= 9007199254740993 9007199254740992) → true  (the values differ)
    //   (< 9007199254740992 9007199254740993) → false (the smaller is "not less")
    let eq = run(json!({"form": "(= 9007199254740993 9007199254740992)", "env": {}}));
    assert_eq!(eq, Ok(json!(true)));
    let lt = run(json!({"form": "(< 9007199254740992 9007199254740993)", "env": {}}));
    assert_eq!(lt, Ok(json!(false)));
}

#[test]
fn degradation_specimen_string_equals_silent_false_on_non_strings() {
    // DEFECT-ADJACENT SPECIMEN (divergence D11, defensive-degradation
    // family): string= returns false — not a type error — on non-string
    // arguments (hkask_lisp.rs:1322-1325). Grouped with assoc's graceful
    // degradation for the operator's ruling.
    let out = run(json!({"form": "(string= 1 \"1\")", "env": {}}));
    assert_eq!(out, Ok(json!(false)));
}

#[test]
fn degradation_specimen_nth_negative_index_silent_null() {
    // DEFECT-ADJACENT SPECIMEN (defensive-degradation family): a negative
    // nth index casts to a huge usize and returns Nil silently.
    let out = run(json!({"form": "(nth -1 (list 1 2 3))", "env": {}}));
    assert_eq!(out, Ok(Value::Null));
}
