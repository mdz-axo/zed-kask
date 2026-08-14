//! Property tests for the step-condition and choice-branch surfaces exposed
//! under the `test-utils` feature gate (`hkask_templates::test_utils`).
//!
//! `condition.rs` is `pub(crate)` — the `test_utils` module re-exports
//! `evaluate_step_condition` and `parse_choice_condition`, closing the gap
//! that left these pure string-parsing/boolean-evaluation surfaces with only
//! in-crate example tests (one case per behavior). These tests drive the real
//! functions directly across a generated input space.
//!
//! # Oracle taxonomy (HarnessLLM §3)
//! - `oracle_invariant` — never-panic (catch_unwind), determinism (two calls
//!   with the same input produce equal output), and totality.
//! - `oracle_reference` — comparison results match a trusted reference
//!   implementation built from the same contract.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): both functions are total — for any condition
//!   string and any context they return a value, never panic.
//! - P1 (Correctness): AND/OR/NOT compose per boolean algebra; comparisons
//!   match the documented operator semantics.
//! - Determinism: both functions are pure — no I/O, no RNG, no `SystemTime`.

use hkask_templates::test_utils::{evaluate_step_condition, parse_choice_condition};
use hkask_test_harness::arb_json_value;
use proptest::prelude::*;
use serde_json::{Value, json};
use std::collections::HashMap;

// ──────────────────────────────────────────────────────────────────────────
// Input strategies
// ──────────────────────────────────────────────────────────────────────────

/// A context key strategy: short identifier-ish strings (the keys the
/// condition parser reads). Kept simple to keep generated contexts readable
/// when a counterexample shrinks.
fn arb_context_key() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z_][a-z0-9_]{0,15}").expect("valid regex")
}

/// A JSON value strategy restricted to the kinds `evaluate_step_condition`
/// branches on: bool, number, string, array, object, null. Mirrors the
/// `arb_json_value` finite-subset used by the sibling compute tests.
fn arb_branch_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(|f| json!(f)),
        prop::string::string_regex("[a-z0-9_]{0,12}")
            .unwrap()
            .prop_map(Value::String),
        prop::collection::vec(arb_json_value(), 0..4).prop_map(Value::Array),
        prop::collection::hash_map(arb_context_key(), arb_json_value(), 0..4)
            .prop_map(|m| Value::Object(m.into_iter().collect())),
    ]
}

/// A context: a small map of identifier keys to branch-able JSON values.
fn arb_context() -> impl Strategy<Value = HashMap<String, Value>> {
    prop::collection::hash_map(arb_context_key(), arb_branch_value(), 0..6)
}

/// A comparison operator from the set `parse_choice_condition` supports:
/// `<=`, `>=`, `==`, `!=`, `<`, `>` (matching `parse_step_comparison`).
fn arb_choice_op() -> impl Strategy<Value = &'static str> {
    prop::sample::select(&["<=", ">=", "==", "!=", "<", ">"])
}

/// A choice-condition operand: a quoted string literal, a bare number, or a
/// bare-word identifier (context key).
fn arb_operand() -> impl Strategy<Value = String> {
    prop_oneof![
        // quoted string literal
        prop::string::string_regex("[a-z_]{0,8}")
            .unwrap()
            .prop_map(|s| format!("'{s}'")),
        // bare number
        any::<i64>().prop_map(|n| n.to_string()),
        // bare-word identifier (context key)
        arb_context_key(),
    ]
}

/// A choice condition: `<operand> <op> <operand>` using only the operators
/// `parse_choice_condition` supports (no `!=`).
fn arb_choice_condition() -> impl Strategy<Value = String> {
    (arb_operand(), arb_choice_op(), arb_operand())
        .prop_map(|(lhs, op, rhs)| format!("{lhs} {op} {rhs}"))
}

/// Arbitrary condition strings — exercises the boolean-operator, negation,
/// comparison, and bare-key branches. Drawn from a small alphabet so
/// counterexamples shrink to something readable.
fn arb_condition_string() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        arb_context_key(),
        arb_choice_condition(),
        Just("true".into()),
        Just("false".into()),
        Just("True".into()),
        Just("False".into()),
    ];
    // Build a small expression tree: leaf, NOT leaf, leaf AND leaf, leaf OR leaf.
    prop_oneof![
        leaf.clone(),
        leaf.clone().prop_map(|l| format!("NOT {l}")),
        (leaf.clone(), leaf.clone()).prop_map(|(a, b)| format!("{a} AND {b}")),
        (leaf.clone(), leaf).prop_map(|(a, b)| format!("{a} OR {b}")),
    ]
}

// ──────────────────────────────────────────────────────────────────────────
// 1. parse_choice_condition — totality and structure
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `parse_choice_condition` never panics on arbitrary strings and returns
    /// `Some` or `None` — it is total over all `&str`.
    #[test]
    fn parse_choice_condition_never_panics(condition in arb_condition_string()) {
        let result = std::panic::catch_unwind(|| parse_choice_condition(&condition));
        prop_assert!(result.is_ok(), "panicked on condition={:?}", condition);
    }

    /// When `parse_choice_condition` returns `Some((field, op, value))`, the
    /// `op` is one of the documented operators and neither `field` nor `value`
    /// is empty (the parser's own non-empty guard).
    #[test]
    fn parse_choice_condition_some_has_nonempty_field_and_documented_op(
        condition in arb_choice_condition(),
    ) {
        if let Some((field, op, value)) = parse_choice_condition(&condition) {
            prop_assert!(!field.is_empty(), "empty field for {:?}", condition);
            prop_assert!(!value.is_empty(), "empty value for {:?}", condition);
            prop_assert!(
                matches!(op, "<=" | ">=" | "==" | "!=" | "<" | ">"),
                "undocumented op {:?} for {:?}", op, condition
            );
        }
    }

    /// Round-trip: for a constructed `<field> <op> <value>` with an operator
    /// `parse_choice_condition` supports, re-serializing the parsed triple
    /// yields a string that parses back to the same triple (the operator and
    /// the trimmed field/value are preserved). This is the reference oracle:
    /// the parser is a left-inverse of the constructor.
    #[test]
    fn parse_choice_condition_round_trips_constructed_form(
        field in arb_context_key(),
        op in arb_choice_op(),
        value in arb_context_key(),
    ) {
        let constructed = format!("{field} {op} {value}");
        let parsed = parse_choice_condition(&constructed);
        prop_assert!(parsed.is_some(), "constructed form did not parse: {}", constructed);
        let (p_field, p_op, p_value) = parsed.unwrap();
        prop_assert_eq!(p_field, field.as_str());
        prop_assert_eq!(p_op, op);
        prop_assert_eq!(p_value, value.as_str());
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 2. evaluate_step_condition — totality and determinism
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `evaluate_step_condition` never panics on arbitrary condition strings
    /// and arbitrary context — it is total (returns `bool`).
    #[test]
    fn evaluate_step_condition_never_panics(
        condition in arb_condition_string(),
        context in arb_context(),
    ) {
        let result = std::panic::catch_unwind(|| {
            evaluate_step_condition(&condition, &context)
        });
        prop_assert!(result.is_ok(), "panicked on condition={:?} context={:?}", condition, context);
    }

    /// `evaluate_step_condition` is deterministic: two calls with the same
    /// condition and context produce the same bool.
    #[test]
    fn evaluate_step_condition_is_deterministic(
        condition in arb_condition_string(),
        context in arb_context(),
    ) {
        let a = evaluate_step_condition(&condition, &context);
        let b = evaluate_step_condition(&condition, &context);
        prop_assert_eq!(a, b, "non-deterministic for condition={:?}", condition);
    }

    /// Truthiness contract: a context key bound to a value the evaluator
    /// considers truthy evaluates to `true`; a falsy value evaluates to
    /// `false`. The reference is the evaluator's own truthiness rules
    /// (non-empty array/object/string, nonzero number, `true`).
    #[test]
    fn evaluate_step_condition_truthiness_matches_reference(
        key in arb_context_key(),
        value in arb_branch_value(),
    ) {
        let mut context = HashMap::new();
        context.insert(key.clone(), value.clone());
        let result = evaluate_step_condition(&key, &context);

        // Reference: the evaluator's documented truthiness rules, applied to
        // the bound value. We compute the expected bool directly and compare.
        let expected = match &value {
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
            Value::String(s) => !s.is_empty() && s != "false" && s != "0",
            Value::Array(a) => !a.is_empty(),
            Value::Object(o) => !o.is_empty(),
            Value::Null => false,
        };
        prop_assert_eq!(
            result, expected,
            "truthiness mismatch for {}={}", key, value
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 3. evaluate_step_condition — boolean algebra (AND/OR/NOT)
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// NOT inverts: `NOT <key>` is the negation of `<key>` for any context
    /// binding. This pins De Morgan for the unary operator.
    #[test]
    fn evaluate_step_condition_not_inverts(
        key in arb_context_key(),
        value in arb_branch_value(),
    ) {
        let mut context = HashMap::new();
        context.insert(key.clone(), value);
        let direct = evaluate_step_condition(&key, &context);
        let negated = evaluate_step_condition(&format!("NOT {key}"), &context);
        prop_assert_eq!(negated, !direct, "NOT did not invert for {}", key);
    }

    /// AND is conjunction: `a AND b` == `a && b` for any two keys.
    #[test]
    fn evaluate_step_condition_and_is_conjunction(
        a_key in arb_context_key(),
        b_key in arb_context_key(),
        a_val in arb_branch_value(),
        b_val in arb_branch_value(),
    ) {
        let mut context = HashMap::new();
        context.insert(a_key.clone(), a_val);
        context.insert(b_key.clone(), b_val);
        let left = evaluate_step_condition(&a_key, &context);
        let right = evaluate_step_condition(&b_key, &context);
        let combined = evaluate_step_condition(&format!("{a_key} AND {b_key}"), &context);
        prop_assert_eq!(combined, left && right, "AND != conjunction");
    }

    /// OR is disjunction: `a OR b` == `a || b` for any two keys.
    #[test]
    fn evaluate_step_condition_or_is_disjunction(
        a_key in arb_context_key(),
        b_key in arb_context_key(),
        a_val in arb_branch_value(),
        b_val in arb_branch_value(),
    ) {
        let mut context = HashMap::new();
        context.insert(a_key.clone(), a_val);
        context.insert(b_key.clone(), b_val);
        let left = evaluate_step_condition(&a_key, &context);
        let right = evaluate_step_condition(&b_key, &context);
        let combined = evaluate_step_condition(&format!("{a_key} OR {b_key}"), &context);
        prop_assert_eq!(combined, left || right, "OR != disjunction");
    }

    /// De Morgan via OR: `NOT a OR NOT b` == `!(a) || !(b)`. Pins that NOT
    /// binds tighter than OR (the parser splits on ` OR ` before `NOT `).
    #[test]
    fn evaluate_step_condition_de_morgan_via_or(
        a_key in arb_context_key(),
        b_key in arb_context_key(),
        a_val in arb_branch_value(),
        b_val in arb_branch_value(),
    ) {
        let mut context = HashMap::new();
        context.insert(a_key.clone(), a_val);
        context.insert(b_key.clone(), b_val);
        let not_a_or_not_b = evaluate_step_condition(
            &format!("NOT {a_key} OR NOT {b_key}"),
            &context,
        );
        let reference = !evaluate_step_condition(&a_key, &context)
            || !evaluate_step_condition(&b_key, &context);
        prop_assert_eq!(not_a_or_not_b, reference, "De Morgan OR identity failed");
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 4. evaluate_step_condition — comparison semantics
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// Numeric equality: `<key> == <number>` is true iff the context value at
    /// `key` equals the number. The `==`/`!=` arms use numeric comparison via
    /// `as_f64()` when both operands are numbers, so integer context values
    /// compare correctly against float literals. See the
    /// `numeric_equality_integer_vs_float_matches` test for the cross-type case.
    ///
    /// `n` is restricted to a range where `n` and `n+1` are distinct as f64
    /// (f64's 52-bit mantissa loses precision above 2^53, so two adjacent i64s
    /// can round to the same f64 — a test artifact, not an evaluator bug).
    #[test]
    fn evaluate_step_condition_numeric_equality_matches_reference(
        key in arb_context_key(),
        n in -(1i64 << 52)..(1i64 << 52),
    ) {
        let mut context = HashMap::new();
        // Context holds an integer; the rhs literal parses to f64. The `==`
        // arm's numeric comparison (via `as_f64()`) makes this match.
        context.insert(key.clone(), json!(n));
        let condition = format!("{key} == {n}");
        let result = evaluate_step_condition(&condition, &context);
        prop_assert!(result, "numeric self-equality failed for {}={}", key, n);

        // A different number must not match.
        let other = n.wrapping_add(1);
        let condition_neq = format!("{key} == {other}");
        let result_neq = evaluate_step_condition(&condition_neq, &context);
        prop_assert!(!result_neq, "numeric inequality failed for {}={} vs {}", key, n, other);
    }

    /// Numeric equality across integer/float representation: `<key> == <n>`
    /// is true even when the context holds an integer (`Number(0)`) and the
    /// rhs literal parses to a float (`Number(0.0)`). The `==`/`!=` arms use
    /// numeric comparison via `as_f64()` when both operands are numbers, so
    /// serde_json's structural distinction between integer and float no
    /// longer silently breaks condition gates. This was previously a bug
    /// (the `==` arm used structural `Value` equality); the fix aligns it
    /// with the ordering ops' numeric comparison.
    #[test]
    fn evaluate_step_condition_numeric_equality_integer_vs_float_matches(
        key in arb_context_key(),
        n in prop::sample::select(&[0i64, 1, -1, 42, 100]),
    ) {
        let mut context = HashMap::new();
        // Context holds an INTEGER (as `json!(n)` produces for i64).
        context.insert(key.clone(), json!(n));
        let condition = format!("{key} == {n}");
        let result = evaluate_step_condition(&condition, &context);
        // The rhs literal `n` parses to f64; the lhs is an integer. Both are
        // numeric, so `as_f64()` comparison must hold.
        prop_assert_eq!(
            result, true,
            "integer == float-literal failed for {}={} (the == arm must use \
             numeric comparison, not structural Value equality)",
            key, n
        );

        // `!=` must also be numeric: `n != n+1` is true.
        let other = n.wrapping_add(1);
        let condition_neq = format!("{key} != {other}");
        let result_neq = evaluate_step_condition(&condition_neq, &context);
        prop_assert_eq!(
            result_neq, true,
            "integer != float-literal failed for {}={} != {} (the != arm must \
             use numeric comparison)",
            key, n, other
        );
    }

    /// Numeric ordering: `<key> > <n>` is true iff the context value is
    /// greater than `n`. The reference is Rust's `>` on f64 (the evaluator
    /// compares via `as_f64`). `lhs`/`rhs` restricted to f64-exact i64 range.
    #[test]
    fn evaluate_step_condition_numeric_ordering_matches_reference(
        key in arb_context_key(),
        lhs in -(1i64 << 52)..(1i64 << 52),
        rhs in -(1i64 << 52)..(1i64 << 52),
    ) {
        let mut context = HashMap::new();
        context.insert(key.clone(), json!(lhs));
        let condition = format!("{key} > {rhs}");
        let result = evaluate_step_condition(&condition, &context);
        let reference = (lhs as f64) > (rhs as f64);
        prop_assert_eq!(result, reference, "numeric > mismatch for {} > {}", lhs, rhs);

        let condition_ge = format!("{key} >= {rhs}");
        let result_ge = evaluate_step_condition(&condition_ge, &context);
        prop_assert_eq!(result_ge, (lhs as f64) >= (rhs as f64), "numeric >= mismatch");
    }

    /// String equality with quoted literals: `<key> == '<lit>'` is true iff
    /// the context value is a string equal to `lit`. Pins the quoted-literal
    /// resolution path against a string reference.
    #[test]
    fn evaluate_step_condition_string_equality_matches_reference(
        key in arb_context_key(),
        lit in prop::string::string_regex("[a-z_]{0,8}").unwrap(),
    ) {
        let mut context = HashMap::new();
        context.insert(key.clone(), Value::String(lit.clone()));
        let condition = format!("{key} == '{lit}'");
        let result = evaluate_step_condition(&condition, &context);
        prop_assert!(result, "string self-equality failed for {}='{}'", key, lit);

        // A different literal must not match.
        let other = format!("{}_other", lit);
        let condition_neq = format!("{key} == '{other}'");
        let result_neq = evaluate_step_condition(&condition_neq, &context);
        prop_assert!(!result_neq, "string inequality failed");
    }

    /// Boolean literal comparison: `<key> == true` is true iff the context
    /// value is `Bool(true)`. This pins the JSON-literal resolution fix
    /// (without it, `true` resolves to `String("true")` and the gate never
    /// fires — the regression the in-crate test guards).
    #[test]
    fn evaluate_step_condition_boolean_literal_comparison(
        key in arb_context_key(),
        b in any::<bool>(),
    ) {
        let mut context = HashMap::new();
        context.insert(key.clone(), Value::Bool(b));
        let condition = format!("{key} == true");
        let result = evaluate_step_condition(&condition, &context);
        prop_assert_eq!(result, b, "boolean == true mismatch for {}={}", key, b);

        let condition_false = format!("{key} == false");
        let result_false = evaluate_step_condition(&condition_false, &context);
        prop_assert_eq!(result_false, !b, "boolean == false mismatch");
    }

    /// An absent key compared with `==` is false (default-deny): the
    /// evaluator resolves a missing key to `None`, and `None` comparisons
    /// return `false`. This pins the no-silent-match-on-missing-key property
    /// for equality.
    #[test]
    fn evaluate_step_condition_absent_key_equality_is_false(
        key in arb_context_key(),
        n in any::<i64>(),
    ) {
        let context: HashMap<String, Value> = HashMap::new();
        let condition = format!("{key} == {n}");
        let result = evaluate_step_condition(&condition, &context);
        prop_assert!(!result, "absent key matched {}", n);
    }
}
