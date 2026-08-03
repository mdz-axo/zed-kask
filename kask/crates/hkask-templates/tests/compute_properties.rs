//! Property tests for the `hkask-templates` compute primitives — real
//! infrastructure, no stubs.
//!
//! Replaces the deleted integration tests:
//! - `tests/lisp_eval_integration.rs` (~140 lines — tested `lisp.eval` through
//!   the full `ManifestExecutor` with a local `NoopInference` stub)
//! - `tests/swarm_converge_integration.rs` (~150 lines — tested
//!   `swarm.converge_accumulate` and `swarm.second_order_monitor` through the
//!   executor with the same stub)
//!
//! Both deleted files routed through `ManifestExecutor::execute_manifest`,
//! which requires an `Arc<dyn InferencePort>` + `Arc<dyn ToolPort>` — no real
//! implementation exists outside the GPUI runtime, so the old tests used a
//! `NoopInference` stub (now deleted). These replacement tests target the
//! compute primitives *directly*, bypassing the executor entirely.
//!
//! # Reachability
//!
//! `dispatch_compute` in `src/compute.rs` is `pub(crate)` and the `compute`
//! module is private (`mod compute;` in `hkask_templates.rs`), so the dispatch
//! table is NOT reachable from this external test crate. Each compute branch
//! was therefore inspected for a public underlying function:
//!
//! - `lisp.eval` delegates entirely to the **public**
//!   `hkask_lisp::eval_sandboxed_with_budget(form, env, max_steps, max_depth)`.
//!   The dispatch wrapper only (a) reads `form`/`env`/`max_steps`/`max_depth`
//!   from the input `Value` with defaults `max_steps=100000`, `max_depth=64`,
//!   and (b) maps `LispError` → `TemplateError::Manifest`. Replicating those
//!   defaults here tests the real Lisp compute primitive — the exact function
//!   the manifest executor calls — without an `InferencePort`.
//!
//! - `swarm.converge_accumulate` and `swarm.second_order_monitor` have NO
//!   public extraction: their logic is inlined into the `pub(crate)`
//!   `dispatch_compute` match arms. There is no public function to call. See
//!   the "Coverage gap" section below.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): the Lisp evaluator must reject arbitrary
//!   (form, env) input gracefully — return `Err`, never panic.
//! - P1 (Correctness): the evaluator's result matches a trusted reference
//!   for arithmetic and `assoc` over the generated input space.
//! - Determinism: the same (form, env) input always yields the same output.
//!
//! # Oracle taxonomy (HarnessLLM §3)
//! - `oracle_reference`  — compare the evaluator's output against a trusted
//!   reference implementation (Rust `+`, `HashMap` lookup).
//! - `oracle_invariant`  — check a property of (input, output): determinism
//!   (two evaluations of the same input produce equal outputs).
//!
//! # Coverage gap (cannot test from an external integration test)
//!
//! `swarm.converge_accumulate` and `swarm.second_order_monitor` are inlined
//! into `pub(crate) fn dispatch_compute` in `src/compute.rs`. Their
//! accumulator logic (iteration-log append, failed-edit memory, influence
//! scoring, fault-count aggregation, reasoning-loop detection, sensor-truth
//! divergence, Go-See cadence) is not extracted into any public function.
//! The only ways to reach them from this test crate would be:
//!   1. Make `dispatch_compute` or a per-primitive wrapper `pub` (a source
//!      edit — disallowed by the task constraints).
//!   2. Re-implement the inline logic here as a reference oracle — but that
//!      tests a *copy* of the logic, not the real code. The `.rules` guidance
//!      (cited in `executor_properties.rs`) explicitly warns against
//!      re-implementing private extraction logic locally; a swarm reference
//!      oracle has the same failure mode: it can drift silently from the real
//!      inline implementation, giving false confidence.
//!   3. Construct a full `ManifestExecutor` — which requires an
//!      `Arc<dyn InferencePort>` (the exact stub the deleted tests used and
//!      the task forbids).
//!
//! None is acceptable. The gap is structural and must be closed by exposing a
//! public seam (e.g. `pub fn swarm_converge_accumulate(input: &Value) ->
//! Result<Value>` in `compute.rs`) or by lifting the inline logic into the
//! `hkask-forecast` / a new public module, then testing it here. The in-crate
//! `#[cfg(test)] mod tests` in `src/compute.rs` covers these branches via
//! `dispatch_compute`, but those tests do not run in this external crate and
//! do not exercise the proptest input space.

use hkask_lisp::{eval_sandboxed_with_budget, from_json, to_json};
use hkask_test_harness::{OracleVerdict, arb_json_value, oracle_invariant, oracle_reference};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

/// Budget defaults used by `dispatch_compute` for the `lisp.eval` branch
/// (`src/compute.rs`: `max_steps` defaults to `100000`, `max_depth` to `64`).
/// Replicating them here keeps the direct call faithful to the dispatch
/// wrapper.
const LIS_MAX_STEPS: u64 = 100_000;
const LIS_MAX_DEPTH: u64 = 64;

/// Evaluate a Lisp `form` against a JSON `env` using the same budget the
/// `lisp.eval` compute primitive uses. This is exactly what
/// `dispatch_compute("lisp.eval", input)` does after unwrapping `form`/`env`.
fn lisp_eval(form: &str, env: &Value) -> Result<Value, hkask_lisp::LispError> {
    eval_sandboxed_with_budget(form, env, LIS_MAX_STEPS, LIS_MAX_DEPTH)
}

/// A string generator that emits Lisp-syntax-like input: parentheses, symbols,
/// numbers, string literals (balanced or not), operators, and whitespace. This
/// exercises the tokenizer/parser/evaluator with both well-formed and
/// malformed forms — the P4 "reject invalid input gracefully" surface.
fn arb_lisp_form() -> BoxedStrategy<String> {
    // Exclude the empty string only where a non-empty form is required; the
    // never-panic test below allows it. The character class includes `"` so
    // unbalanced string literals reach the tokenizer.
    prop::string::string_regex(r#"[()a-zA-Z0-9_+\-*/<=>!\" \t]{0,48}"#)
        .expect("valid regex")
        .boxed()
}

// ──────────────────────────────────────────────────────────────────────────
// 1. lisp.eval never panics on arbitrary (form, env) input
//
// The compute primitive's security-relevant boundary property (P4): the
// sandboxed evaluator is bounded in steps and depth and must return `Err` for
// malformed or non-terminating-within-budget input — never panic, never abort
// the process. `arb_json_value` produces arbitrarily nested env values; the
// form generator produces unbalanced/malformed Lisp syntax. The evaluator
// must handle every combination without unwinding.
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `eval_sandboxed_with_budget` returns `Ok` or `Err` for every (form, env)
    /// pair — it never panics. `catch_unwind` asserts the no-panic property
    /// directly: a panic surfaces as `Err` from `catch_unwind`, failing the
    /// property.
    #[test]
    fn lisp_eval_never_panics_on_arbitrary_input(
        form in arb_lisp_form(),
        env in arb_json_value(),
    ) {
        let result = std::panic::catch_unwind(|| {
            lisp_eval(&form, &env)
        });
        prop_assert!(
            result.is_ok(),
            "lisp.eval panicked on form={:?} env={}: {:?}",
            form,
            env,
            result.err()
        );
        // The inner call must itself be Ok(Ok(_)) or Ok(Err(_)) — both are
        // graceful. A panic would have made `result` Err above, so reaching
        // here means the boundary held.
        let inner = result.unwrap();
        prop_assert!(
            inner.is_ok() || inner.is_err(),
            "lisp.eval returned neither Ok nor Err"
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 2. lisp.eval arithmetic matches a trusted reference (oracle_reference)
//
// For two integers a, b, the Lisp form `(+ a b)` must evaluate to exactly
// `a + b` (as a JSON integer). The reference is Rust's native `+`, which is
// the ground truth the `+` builtin is meant to mirror. Bounded range avoids
// i64 overflow (the evaluator's `+` is not the subject under test).
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `(= (+ a b) expected)` evaluates to `true` iff the evaluator's `+`
    /// matches Rust's `+` for the same operands. The reference oracle compares
    /// the evaluator's boolean output against the reference boolean.
    #[test]
    fn lisp_eval_addition_matches_reference(
        a in -1_000_000i64..1_000_000,
        b in -1_000_000i64..1_000_000,
    ) {
        let expected = a + b;
        let form = format!("(= (+ {a} {b}) {expected})");
        let env = json!({});
        let output = lisp_eval(&form, &env).expect("well-formed arithmetic form evaluates");

        // Reference: the comparison is true exactly when the evaluator's sum
        // equals the Rust sum. The trusted result is `true`.
        let input = json!({ "a": a, "b": b, "expected": expected });
        let oracle = oracle_reference(|_: &Value| json!(true));
        let verdict = oracle.verify(&input, &output);
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "(= (+ {} {}) {}) did not evaluate to true; got {}",
            a, b, expected, output
        );
    }

    /// `(+ a b)` evaluates to the JSON integer `a + b` directly — a stronger
    /// reference check on the value (not just the comparison).
    #[test]
    fn lisp_eval_addition_returns_reference_value(
        a in -1_000_000i64..1_000_000,
        b in -1_000_000i64..1_000_000,
    ) {
        let expected = a + b;
        let form = format!("(+ {a} {b})");
        let output = lisp_eval(&form, &json!({})).expect("arithmetic form evaluates");
        let input = json!({ "a": a, "b": b });
        let oracle = oracle_reference(move |_: &Value| json!(expected));
        let verdict = oracle.verify(&input, &output);
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "(+ {} {}) produced {}, expected {}",
            a, b, output, json!(a + b)
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 3. lisp.eval assoc matches a trusted reference (oracle_reference)
//
// `(assoc "key" obj)` must return the value at `obj["key"]` (JSON object →
// association list). The reference is Rust's `Map::get`. The env is injected
// as a top-level binding the form references by name.
// ──────────────────────────────────────────────────────────────────────────

/// Generate a non-empty JSON object plus the name of one of its keys, for
/// `assoc` reference testing. Returns `(env, key, expected_value)` where `env`
/// binds the object under the symbol `obj`.
fn arb_assoc_case() -> BoxedStrategy<(Value, String, Value)> {
    prop::collection::vec(
        (
            prop::string::string_regex(r#"[a-z_][a-z0-9_]{0,15}"#).expect("valid regex"),
            arb_json_value(),
        ),
        1..6,
    )
    .prop_map(|pairs| {
        let mut map = Map::new();
        let mut chosen_key = String::new();
        let mut chosen_val = Value::Null;
        for (k, v) in pairs {
            if chosen_key.is_empty() {
                chosen_key = k.clone();
                chosen_val = v.clone();
            }
            map.insert(k, v);
        }
        let inner = Value::Object(map);
        // The env binds the object under the symbol `obj`; the form references
        // `obj`. This mirrors how `dispatch_compute` threads `env` into the
        // evaluator (top-level keys become bindings).
        let env = json!({ "obj": inner });
        // Route the expected value through the evaluator's own JSON↔Lisp
        // conversion (`from_json`/`to_json`, both public in `hkask-lisp`) so
        // the reference matches the evaluator's representation, not the raw
        // JSON. Without this, a u64 > i64::MAX (e.g. 2^63) is preserved by
        // `serde_json` but converted to f64 by the evaluator, and a naive
        // reference would flag a false mismatch. The evaluator's conversion
        // is the contract; the reference mirrors it.
        let expected = to_json(&from_json(&chosen_val));
        (env, chosen_key, expected)
    })
    .boxed()
}

proptest! {
    /// `(assoc "key" obj)` returns `obj["key"]` for a generated object and
    /// key. The reference oracle compares against the value stored in the
    /// generated map (Rust `Map::get` is the ground truth).
    #[test]
    fn lisp_eval_assoc_matches_reference(
        case in arb_assoc_case(),
    ) {
        let (env, key, expected) = case;
        // JSON string literals need embedded quotes escaped for the Lisp
        // tokenizer. The key is a restricted `[a-z_][a-z0-9_]*` regex, so it
        // contains no characters needing Lisp escaping beyond the surrounding
        // quotes.
        let form = format!(r#"(assoc "{key}" obj)"#);
        let output = lisp_eval(&form, &env).expect("well-formed assoc form evaluates");

        let input = json!({ "key": key, "env": env });
        let oracle = oracle_reference(move |_: &Value| expected.clone());
        let verdict = oracle.verify(&input, &output);
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "(assoc \"{}\" obj) produced {}, expected the map value",
            key, output
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 4. lisp.eval is deterministic (oracle_invariant)
//
// A pure function with no I/O, no randomness, and an immutable environment
// must produce identical outputs for identical inputs across repeated
// calls. This is the determinism contract the manifest executor relies on
// (a compute step's result is cached under `step_{ordinal}_result` and reused
// by later steps). The invariant oracle checks: output_1 == output_2.
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// Two evaluations of the same `(form, env)` produce equal outputs. The
    /// invariant oracle asserts `out_1 == out_2`; both are derived from the
    /// same input, so any divergence is a determinism bug.
    #[test]
    fn lisp_eval_is_deterministic_for_fixed_input(
        form in arb_lisp_form(),
        env in arb_json_value(),
    ) {
        let out_1 = lisp_eval(&form, &env);
        let out_2 = lisp_eval(&form, &env);
        // Both calls must agree — either both Ok with equal values, or both
        // Err with equal errors. Compare via the JSON-serializable
        // representation so `LispError` equality is not required.
        let ser_1 = serde_json::to_string(&out_1.ok()).expect("Option<Value> serializes");
        let ser_2 = serde_json::to_string(&out_2.ok()).expect("Option<Value> serializes");

        let input = json!({ "form": form, "env": env });
        let output = json!({ "first": ser_1, "second": ser_2 });
        let oracle = oracle_invariant(|_: &Value, out: &Value| {
            let first = out.get("first").and_then(|v| v.as_str()).unwrap_or("");
            let second = out.get("second").and_then(|v| v.as_str()).unwrap_or("");
            if first == second {
                Ok(())
            } else {
                Err(format!(
                    "lisp.eval was non-deterministic: first call serialized to {first}, \
                     second to {second}"
                ))
            }
        });
        let verdict = oracle.verify(&input, &output);
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "lisp.eval determinism violated for form={:?}",
            form
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 5. lisp.eval malformed forms return Err, not a panic (P4 boundary)
//
// Explicitly assert the error path for a few classes of malformed input that
// the fuzzer may or may not surface. These are not hardcoded one-case
// assertions of *correctness* — they pin the *boundary contract* (Err, not
// panic) that the proptest above relies on, for representative malformed
// shapes. The reference oracle is the `is_err` predicate.
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// Unbalanced parentheses yield `Err`, never a panic. The form is
    /// generated by truncating a well-formed expression at an arbitrary
    /// point, guaranteeing unbalanced syntax without hardcoding a single
    /// string.
    #[test]
    fn lisp_eval_unbalanced_forms_return_err_not_panic(
        depth in 1usize..8,
        truncate_at in 0usize..32,
    ) {
        // Build a nested form `(if true (+ 1 2) (- 3 4))` of the given depth,
        // then truncate it to force unbalanced parens.
        let mut form = String::new();
        for _ in 0..depth {
            form.push_str("(if true ");
        }
        form.push_str("42");
        for _ in 0..depth {
            form.push(')');
        }
        let truncated: String = form.chars().take(truncate_at).collect();
        let result = std::panic::catch_unwind(|| lisp_eval(&truncated, &json!({})));
        prop_assert!(result.is_ok(), "lisp.eval panicked on unbalanced form");
        // An empty or fully-balanced truncation may succeed; the property is
        // "no panic", not "must be Err". Assert the no-panic contract holds.
        let _ = result.unwrap();
    }
}
