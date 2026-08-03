//! Property tests for the `pub(crate)` compute dispatch table exposed under
//! the `test-utils` feature gate (`hkask_templates::test_utils::dispatch_compute`).
//!
//! `compute_properties.rs` (the sibling file) documents a coverage gap: the
//! swarm accumulators and second-order monitor are inlined into the
//! `pub(crate) fn dispatch_compute` match arms and have no public extraction,
//! so they were unreachable from this external test crate. The `test_utils`
//! module re-exports `dispatch_compute`, closing that gap. These tests drive
//! the real dispatch table directly — no `InferencePort`, no stub.
//!
//! # Oracle taxonomy (HarnessLLM §3)
//! - `oracle_invariant` — never-panic (catch_unwind), determinism (two calls
//!   with the same input produce equal output), and shape conformance.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): `dispatch_compute` is total — for any `compute_ref`
//!   string and any JSON input it returns `Ok` or `Err`, never panics.
//! - P1 (Correctness): the deterministic accumulators produce identical output
//!   for identical valid input.
//! - Determinism: the swarm accumulators are pure functions of their input
//!   (no `SystemTime`, no RNG) — the same input must yield the same output.

use hkask_templates::test_utils::dispatch_compute;
use hkask_test_harness::{OracleVerdict, arb_json_value, oracle_invariant};
use proptest::prelude::*;
use serde_json::{Value, json};

/// Finite f64 strategy (mirrors `arb_json_value`'s finite filter) so the
/// `swarm.converge_accumulate` `d` field is always a valid finite number.
fn arb_finite_f64() -> impl Strategy<Value = f64> {
    any::<f64>().prop_filter("must be finite", |f| f.is_finite())
}

/// A valid `swarm.converge_accumulate` input: `{"d": <finite f64>}`. All other
/// fields (`task_success`, `deficit_class`, `iteration_log`, `failed_edits`,
/// `influence_scores`, `fault_count`, `agent_at_fault`, `decisions`,
/// `swarm_state`) default gracefully inside `dispatch_compute`.
fn arb_converge_valid() -> impl Strategy<Value = Value> {
    arb_finite_f64().prop_map(|d| json!({ "d": d }))
}

/// A valid `swarm.second_order_monitor` input: an `iteration_log` array of
/// entries each carrying `d`/`s`/`deficit_class`/`decision_action`. Any JSON is
/// accepted (the branch defaults missing fields), but a shaped log exercises
/// the reasoning-loop and sensor-truth-divergence detection paths.
fn arb_monitor_valid() -> impl Strategy<Value = Value> {
    prop::collection::vec(
        (
            arb_finite_f64(),
            prop::option::of(arb_finite_f64()),
            prop::string::string_regex("[a-z_]{0,12}").unwrap(),
            prop::string::string_regex("[a-z_]{0,16}").unwrap(),
        ),
        0..8,
    )
    .prop_map(|entries| {
        let log: Vec<Value> = entries
            .into_iter()
            .map(|(d, s, deficit, action)| {
                let mut e = json!({
                    "d": d,
                    "deficit_class": deficit,
                    "decision_action": action,
                });
                if let Some(sv) = s {
                    e["s"] = json!(sv);
                }
                e
            })
            .collect();
        json!({ "iteration_log": log })
    })
}

/// Arbitrary `compute_ref` strings — exercises both known refs and unknown /
/// malformed strings. The totality property must hold for all of them.
fn arb_compute_ref() -> impl Strategy<Value = String> {
    prop::string::string_regex(r#"[A-Za-z0-9_.\-]{0,32}"#).expect("valid regex")
}

// ──────────────────────────────────────────────────────────────────────────
// 1. swarm.converge_accumulate — never panics, deterministic for valid input
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `dispatch_compute("swarm.converge_accumulate", input)` never panics on
    /// arbitrary JSON input — it returns `Ok` or `Err` (the `d` field is
    /// required; missing/non-numeric `d` yields a graceful `Err`).
    #[test]
    fn converge_accumulate_never_panics(input in arb_json_value()) {
        let result = std::panic::catch_unwind(|| {
            dispatch_compute("swarm.converge_accumulate", &input)
        });
        prop_assert!(result.is_ok(), "panicked on input={input}");
        let inner = result.unwrap();
        prop_assert!(inner.is_ok() || inner.is_err(), "neither Ok nor Err");
    }

    /// For valid input (`{"d": <finite f64>}`), the accumulator succeeds and is
    /// deterministic: two calls with the same input produce equal output.
    #[test]
    fn converge_accumulate_deterministic_for_valid_input(input in arb_converge_valid()) {
        let a = dispatch_compute("swarm.converge_accumulate", &input)
            .expect("valid converge input must succeed");
        let b = dispatch_compute("swarm.converge_accumulate", &input)
            .expect("valid converge input must succeed");
        prop_assert_eq!(a, b, "non-deterministic for input={:?}", input);
    }

    /// Invariant: a successful `swarm.converge_accumulate` result is a JSON
    /// object carrying the documented accumulator fields.
    #[test]
    fn converge_accumulate_result_shape(input in arb_converge_valid()) {
        let output = dispatch_compute("swarm.converge_accumulate", &input)
            .expect("valid input succeeds");
        let oracle = oracle_invariant(|_, out: &Value| {
            let obj = out.as_object().ok_or("result not an object")?;
            for key in &["iteration_log", "failed_edits", "influence_scores", "fault_count"] {
                if !obj.contains_key(*key) {
                    return Err(format!("missing field {key}"));
                }
            }
            if obj.get("iteration_log").and_then(|v| v.as_array()).is_none() {
                return Err("iteration_log not an array".into());
            }
            Ok(())
        });
        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 2. swarm.second_order_monitor — never panics, deterministic for valid input
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `dispatch_compute("swarm.second_order_monitor", input)` never panics on
    /// arbitrary JSON and always succeeds (all fields default inside the
    /// branch — there is no required field).
    #[test]
    fn second_order_monitor_never_panics(input in arb_json_value()) {
        let result = std::panic::catch_unwind(|| {
            dispatch_compute("swarm.second_order_monitor", &input)
        });
        prop_assert!(result.is_ok(), "panicked on input={input}");
        let inner = result.unwrap();
        prop_assert!(inner.is_ok(), "second_order_monitor must succeed for any input");
    }

    /// For valid shaped input, the monitor is deterministic.
    #[test]
    fn second_order_monitor_deterministic_for_valid_input(input in arb_monitor_valid()) {
        let a = dispatch_compute("swarm.second_order_monitor", &input)
            .expect("monitor must succeed");
        let b = dispatch_compute("swarm.second_order_monitor", &input)
            .expect("monitor must succeed");
        prop_assert_eq!(a, b, "non-deterministic for input={:?}", input);
    }

    /// Invariant: the monitor result carries the documented signal fields.
    #[test]
    fn second_order_monitor_result_shape(input in arb_monitor_valid()) {
        let output = dispatch_compute("swarm.second_order_monitor", &input)
            .expect("monitor succeeds");
        let oracle = oracle_invariant(|_, out: &Value| {
            let obj = out.as_object().ok_or("result not an object")?;
            for key in &["reasoning_loop", "sensor_truth_divergence", "recommendation"] {
                if !obj.contains_key(*key) {
                    return Err(format!("missing field {key}"));
                }
            }
            Ok(())
        });
        prop_assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 3. dispatch_compute totality — P4 clear boundaries
//
// For any `compute_ref` string and any JSON input, `dispatch_compute` returns
// `Ok` or `Err` — never panics. Unknown refs hit the catch-all arm and return
// a `TemplateError::Manifest`; known refs either succeed or return a typed
// `Err` for missing/invalid fields. `catch_unwind` asserts the no-panic
// property directly across the whole dispatch table.
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    #[test]
    fn dispatch_compute_total_never_panics(
        compute_ref in arb_compute_ref(),
        input in arb_json_value(),
    ) {
        let result = std::panic::catch_unwind(|| {
            dispatch_compute(&compute_ref, &input)
        });
        prop_assert!(
            result.is_ok(),
            "dispatch_compute panicked on ref={compute_ref:?} input={input}"
        );
        let inner = result.unwrap();
        prop_assert!(
            inner.is_ok() || inner.is_err(),
            "dispatch_compute returned neither Ok nor Err for ref={compute_ref:?}"
        );
    }
}
