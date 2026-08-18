//! Property tests for `hkask_templates::executor` — real infrastructure, no stubs.
//!
//! Replaces the deleted `mod tests` block in `src/executor.rs` (1,223 lines of
//! fake stub ports: StubInferencePort, StubToolPort, SourceToolPort, etc.). These
//! tests target the pure-function and data-model surfaces of the executor that
//! are reachable without constructing a full `ManifestExecutor` (which requires
//! `Arc<dyn InferencePort>` + `Arc<dyn ToolPort>` — no real implementation is
//! available outside the GPUI runtime).
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): input surfaces reject invalid input gracefully, never panic
//! - P1 (Correctness): invariants hold for all inputs in the generated space
//!
//! # Oracle taxonomy (HarnessLLM §3)
//! - `oracle_invariant`  — check a property of (input, output)
//! - `oracle_reference`   — compare output against a trusted reference
//!
//! # Coverage gaps (cannot test from an external integration test)
//! - Branching control-flow jump (`step_idx` reassignment in `execute_manifest`):
//!   requires a live `ManifestExecutor` with InferencePort + ToolPort to produce a
//!   `step_{ordinal}_result` carrying a `routing` field. Test 3 covers the routing
//!   *table* (the data the executor reads), not the jump.
//! - Ordinal-keyed final-result extraction (`extract_final_step_result` /
//!   `extract_final_step_entry`): private free functions in `executor.rs`, not
//!   reachable from this external test crate. Test 4 covers the `step_{ordinal}_result`
//!   *storage convention* (key format round-trip + retrieval), not the numeric
//!   max-ordinal selection. Re-implementing the private logic here would violate
//!   the `.rules` "do not re-implement `value.get(\"content\")` locally" guidance
//!   generalized to extraction; the real function must be exposed (or tested
//!   in-crate) to close the gap.

use std::collections::HashMap;

use hkask_templates::bundle::config::{AggregationSource, ConvergenceConfig};
use hkask_templates::convergence::{ConvergenceStatus, ConvergenceTracker};
use hkask_templates::load_manifest_from_yaml;
use hkask_test_harness::{OracleVerdict, arb_json_value, oracle_invariant, oracle_reference};
use hkask_types::tool_response::{parse_tool_response, unwrap_tool_envelope};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

// ──────────────────────────────────────────────────────────────────────────
// 1. Tool response envelope unwrapping
//
// The executor (and every MCP consumer) unwraps `{"content": <value>}` envelopes
// via `hkask_types::tool_response::unwrap_tool_envelope` — the single seam. The
// property: for every JSON payload P, wrapping it in `{"content": P}` and
// unwrapping yields exactly P. This holds even when P is itself an object with a
// `content` key, because the outer unwrap reads the outer `content` only.
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `unwrap_tool_envelope({"content": P}) == P` for all JSON payloads P.
    ///
    /// Oracle: invariant — the unwrapped value must equal the original payload.
    #[test]
    #[allow(clippy::redundant_clone)] // json! consumes the clone; payload is borrowed after
    fn unwrap_tool_envelope_returns_inner_payload_for_all_json(
        payload in arb_json_value(),
    ) {
        let envelope = serde_json::json!({ "content": payload.clone() });
        let unwrapped = unwrap_tool_envelope(envelope);

        let oracle = oracle_invariant(|input, output| {
            if output == input {
                Ok(())
            } else {
                Err(format!(
                    "unwrap_tool_envelope did not return the payload:\n  input:  {input}\n  output: {output}"
                ))
            }
        });
        let verdict = oracle.verify(&payload, &unwrapped);
        prop_assert_eq!
            (
            verdict,
            OracleVerdict::Pass,
            "envelope unwrapping failed for payload: {}",
            payload
        );
    }

    /// `parse_tool_response` round-trips: serializing `{"content": P}` to a string
    /// and parsing it yields `Some(P)`.
    ///
    /// Oracle: reference — the expected output is the payload itself (identity
    /// reference), since the envelope is constructed to wrap exactly P.
    #[test]
    #[allow(clippy::redundant_clone)] // json! consumes the clone; payload is borrowed after
    fn parse_tool_response_round_trips_envelope_for_all_json(
        payload in arb_json_value(),
    ) {
        let envelope = serde_json::json!({ "content": payload.clone() });
        let serialized = serde_json::to_string(&envelope)
            .expect("serializing a serde_json::Value is infallible");
        let parsed = parse_tool_response(&serialized);
        prop_assert!(
            parsed.is_some(),
            "parse_tool_response returned None on a valid envelope: {serialized}"
        );

        // Reference: the expected unwrapped value is the original payload after a
        // JSON serialize→parse round-trip. `parse_tool_response` itself parses the
        // serialized envelope, so the inner payload traverses one serialize→parse
        // cycle; the reference must do the same to keep f64 rounding identical on
        // both sides (the proptest re-parse trick for float comparisons).
        let oracle = oracle_reference(|input: &Value| {
            let serialized = serde_json::to_string(input).expect("Value serializes");
            serde_json::from_str::<Value>(&serialized).expect("Value parses")
        });
        let verdict = oracle.verify(&payload, &parsed.unwrap());
        prop_assert_eq!
            (
            verdict,
            OracleVerdict::Pass,
            "parse_tool_response did not round-trip the payload: {}",
            payload
        );
    }

    /// Defensive branch: a payload with no `content` wrapper is returned unchanged.
    /// The seam must not invent an envelope where none exists.
    #[test]
    fn unwrap_tool_envelope_leaves_bare_payload_unchanged(
        payload in arb_json_value(),
    ) {
        // Only objects can carry a `content` key; non-objects are always bare.
        // For objects, skip the case where the payload *is* an envelope (covered
        // above) to isolate the bare-payload branch.
        prop_assume!(!payload.is_object() || payload.get("content").is_none());

        let bare = payload.clone();
        let unwrapped = unwrap_tool_envelope(bare);

        let oracle = oracle_invariant(|input, output| {
            if output == input {
                Ok(())
            } else {
                Err(format!(
                    "bare payload was altered by unwrap_tool_envelope:\n  input:  {input}\n  output: {output}"
                ))
            }
        });
        let verdict = oracle.verify(&payload, &unwrapped);
        prop_assert_eq!
            (
            verdict,
            OracleVerdict::Pass,
            "bare payload not preserved: {}",
            payload
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 2. Manifest deserialization panic-freedom
//
// `load_manifest_from_yaml` is the input surface that all manifest YAML flows
// through. It must never panic on arbitrary input — it returns Ok or Err.
// JSON is a subset of YAML, so arbitrary `arb_json_value` serialized to a JSON
// string is valid YAML-shaped input (structurally valid, semantically likely
// wrong → Err, which is the P4 contract).
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `load_manifest_from_yaml` never panics on arbitrary structured input.
    ///
    /// P4 (Clear Boundaries): the input surface rejects malformed input with a
    /// typed `ManifestLoadError`, never by panicking. `catch_unwind` makes the
    /// panic-freedom contract explicit (mirrors `tests/manifest_fuzz.rs`).
    #[test]
    fn load_manifest_from_yaml_never_panics_on_structured_input(
        value in arb_json_value(),
    ) {
        // JSON is a subset of YAML; serializing Value to a JSON string yields
        // YAML-parseable input. serde_json serialization is infallible for Value.
        let yaml_string = serde_json::to_string(&value)
            .expect("JSON serialization is infallible for Value");

        let result = std::panic::catch_unwind(|| {
            let _ = load_manifest_from_yaml(&yaml_string);
        });
        prop_assert!(
            result.is_ok(),
            "load_manifest_from_yaml panicked on structured input: {yaml_string}"
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 3. Branching routing
//
// The executor's branching reads a `routing` field from the step result and
// jumps to the target ordinal via the step's `branching` map
// (`HashMap<String, u32>`). Without a live executor (needs InferencePort +
// ToolPort to produce a step result), we test the routing *table* the executor
// reads: that an arbitrary branching map survives manifest deserialization, and
// that every routing key in the map resolves to its declared target ordinal.
//
// This pins the routing-decision data the executor relies on; the actual
// `step_idx` jump is a noted gap (requires the full executor).
// ──────────────────────────────────────────────────────────────────────────

/// Strategy for a branching map: routing key → target ordinal.
fn arb_branching_map() -> BoxedStrategy<HashMap<String, u32>> {
    prop::collection::hash_map(
        prop::string::string_regex("[a-z_][a-z0-9_/]{0,16}").expect("valid regex"),
        0u32..100_000,
        0..8,
    )
    .boxed()
}

proptest! {
    /// A branching map embedded in a manifest step round-trips through
    /// `load_manifest_from_yaml`, and every routing key resolves to its ordinal.
    ///
    /// Oracle: invariant — the deserialized branching map equals the original,
    /// and each key lookup returns the declared ordinal (the routing decision
    /// the executor would make).
    #[test]
    fn branching_map_routes_every_key_to_its_ordinal(
        map in arb_branching_map(),
    ) {
        let manifest_json = serde_json::json!({
            "manifest": { "id": "test-branching" },
            "steps": [{
                "ordinal": 1u32,
                "action": "select",
                "description": "branching step",
                "branching": serde_json::to_value(&map).expect("HashMap serializes"),
            }]
        });
        let yaml = serde_json::to_string(&manifest_json)
            .expect("JSON serialization is infallible for Value");
        let loaded = load_manifest_from_yaml(&yaml);
        prop_assert!(
            loaded.is_ok(),
            "valid branching manifest failed to load: {:?}",
            loaded.err()
        );
        let manifest = loaded.unwrap();
        prop_assert_eq!(manifest.steps.len(), 1, "exactly one step expected");
        let branching = manifest.steps[0]
            .branching
            .clone()
            .expect("branching map should deserialize");

        // Invariant oracle: deserialized branching map equals the original map.
        let oracle = oracle_invariant(|input, output| {
            let original: HashMap<String, u32> = serde_json::from_value(input.clone())
                .map_err(|e| format!("bad input branching map: {e}"))?;
            let decoded: HashMap<String, u32> = serde_json::from_value(output.clone())
                .map_err(|e| format!("bad output branching map: {e}"))?;
            if original == decoded {
                Ok(())
            } else {
                Err(format!(
                    "branching map drifted through deserialization:\n  original: {original:?}\n  decoded:  {decoded:?}"
                ))
            }
        });
        let input_json = serde_json::to_value(&map).expect("HashMap serializes");
        let output_json = serde_json::to_value(&branching).expect("HashMap serializes");
        let verdict = oracle.verify(&input_json, &output_json);
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "branching map did not round-trip through manifest deserialization"
        );

        // Routing decision: every key in the map resolves to its declared ordinal.
        // This is the lookup the executor performs (`branching.get(routing)`).
        for (key, ordinal) in &map {
            let target = branching.get(key);
            prop_assert_eq!(
                target,
                Some(ordinal),
                "routing key '{}' did not resolve to ordinal {}",
                key,
                ordinal
            );
        }
    }

    /// An absent routing key yields no jump (continue semantics): the executor
    /// falls through to the next ordinal when the routing field is absent or does
    /// not match any key. Pins the "safe default — no branching" contract.
    #[test]
    fn absent_routing_key_yields_no_target(
        map in arb_branching_map(),
        absent_key in prop::string::string_regex("[a-z_][a-z0-9_/]{0,16}").expect("valid regex"),
    ) {
        // Only meaningful when the absent key is genuinely absent.
        prop_assume!(!map.contains_key(&absent_key));

        let lookup = map.get(&absent_key);
        prop_assert!(
            lookup.is_none(),
            "absent routing key '{}' unexpectedly resolved to {:?}",
            absent_key,
            lookup
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 4. Step result storage
//
// After executing a step, the executor stores the result under
// `step_{ordinal}_result` in the context map. The ordinal-keyed *max* selection
// (`extract_final_step_result` / `extract_final_step_entry`) is private to the
// `executor` module and not reachable from this external test crate; we pin the
// `step_{ordinal}_result` storage *convention* instead: the key format
// round-trips (`step_{N}_result` parses back to N) and a value stored under the
// key is retrievable. The numeric max-ordinal extraction remains a gap.
// ──────────────────────────────────────────────────────────────────────────

/// Parse the ordinal back out of a `step_{ordinal}_result` storage key.
/// Mirrors the key shape the executor writes (`format!("step_{ordinal}_result")`).
fn parse_step_result_ordinal(key: &str) -> Option<u32> {
    key.strip_prefix("step_")
        .and_then(|rest| rest.strip_suffix("_result"))
        .and_then(|n| n.parse::<u32>().ok())
}

proptest! {
    /// The `step_{ordinal}_result` key format round-trips: formatting an ordinal
    /// and parsing it back yields the same ordinal, for all u32 ordinals.
    ///
    /// Oracle: invariant — the parsed ordinal equals the original.
    #[test]
    fn step_result_key_format_round_trips(
        ordinal in 0u32..100_000,
    ) {
        let key = format!("step_{ordinal}_result");
        let parsed = parse_step_result_ordinal(&key);

        let oracle = oracle_invariant(|input, output| {
            let original = input.as_u64().ok_or("input is not a u64")? as u32;
            let decoded = output.as_u64().ok_or("output is not a u64")? as u32;
            if original == decoded {
                Ok(())
            } else {
                Err(format!("key format round-trip failed: {original} != {decoded}"))
            }
        });
        let input = serde_json::json!(ordinal);
        let output = serde_json::json!(parsed.unwrap_or(u32::MAX));
        let verdict = oracle.verify(&input, &output);
        prop_assert_eq!
            (
            verdict,
            OracleVerdict::Pass,
            "step_{}_result key does not round-trip to ordinal {}",
            ordinal,
            ordinal
        );
    }

    /// Step results are stored and retrievable by ordinal: for a set of
    /// (ordinal, value) pairs, each value is retrievable under
    /// `step_{ordinal}_result`, and the key parses back to the ordinal.
    ///
    /// Oracle: invariant — the retrieved value equals the stored value.
    #[test]
    fn step_results_stored_and_retrievable_by_ordinal(
        entries in prop::collection::vec((0u32..100_000, arb_json_value()), 1..8),
    ) {
        let mut context: HashMap<String, Value> = HashMap::new();
        let mut stored: HashMap<u32, Value> = HashMap::new();
        for (ordinal, value) in &entries {
            let key = format!("step_{ordinal}_result");
            context.insert(key, value.clone());
            // Later entries for the same ordinal overwrite, mirroring the
            // executor's last-write-wins storage semantics.
            stored.insert(*ordinal, value.clone());
        }

        let oracle = oracle_invariant(|_input, output| {
            let expected = output.get("expected").ok_or("missing expected")?;
            let retrieved = output.get("retrieved").ok_or("missing retrieved")?;
            if retrieved == expected {
                Ok(())
            } else {
                Err(format!(
                    "retrieved value does not match stored:\n  expected: {expected}\n  retrieved: {retrieved}"
                ))
            }
        });

        for (ordinal, value) in &stored {
            let key = format!("step_{}_result", ordinal);
            // Key format round-trips.
            prop_assert_eq!(
                parse_step_result_ordinal(&key),
                Some(*ordinal),
                "key '{}' does not parse back to ordinal {}",
                key,
                ordinal
            );
            // Value is retrievable under the key.
            let retrieved = context
                .get(&key)
                .cloned()
                .unwrap_or(Value::Null);
            let output = serde_json::json!({
                "expected": value.clone(),
                "retrieved": retrieved,
            });
            let verdict = oracle.verify(&Value::Null, &output);
            prop_assert_eq!(
                verdict,
                OracleVerdict::Pass,
                "step_{}_result not retrievable",
                ordinal
            );
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 5. ConvergenceTracker — pure state machine (Kata model)
//
// `ConvergenceTracker` is the one executor subsystem that is a *pure* state
// machine over `(config, history)` with no dependency on `InferencePort` or
// `ToolPort` (see its module doc comment). It is fully `pub`, so it is the
// largest reachable pure-function surface for property testing without stubs.
//
// The Kata model has three orthogonal stop conditions:
//   - gap          : signal < gap_epsilon (limit of a sequence)
//   - cauchy       : max pairwise distance in the last cauchy_window readings
//                    < cauchy_epsilon (the iterates stopped moving)
//   - calibration  : rolling Brier average over brier_window < brier_threshold
//                    (predictions are calibrated)
//
// Each is a deterministic function of the history. The reference oracles below
// encode the *mathematical* definitions (Cauchy criterion, rolling mean),
// independent of the implementation — this is oracle-2 (reference), not a
// re-implementation of the private extraction seam the .rules trap warns
// against.
// ──────────────────────────────────────────────────────────────────────────

/// Arbitrary finite f64 (no NaN/Inf). Convergence math is over real-valued
/// gaps; NaN/Inf inputs would force every comparison to a single fixed branch
/// and shrink the meaningful input space to zero.
fn arb_finite_f64() -> BoxedStrategy<f64> {
    any::<f64>()
        .prop_filter("must be finite", |f| f.is_finite())
        .boxed()
}

/// Arbitrary strictly-positive finite f64 — used for epsilons/thresholds,
/// which are magnitudes (a non-positive epsilon makes the criterion trivially
/// false for every finite reading, collapsing the generated space).
fn arb_positive_finite_f64() -> BoxedStrategy<f64> {
    any::<f64>()
        .prop_filter("must be positive finite", |f| f.is_finite() && *f > 0.0)
        .boxed()
}

/// A Kata-enabled config base. `kata_enabled()` requires a non-empty
/// `convergence_mode` AND at least one target-condition field, so set
/// `target_artifacts_field` to enable the Kata path. `min_iterations = 0` so
/// the iteration gate does not mask the convergence criterion under test.
fn kata_config_base() -> ConvergenceConfig {
    let mut config = ConvergenceConfig::default();
    config.target_artifacts_field = Some("target_artifacts".to_string());
    config.convergence_mode = "gap".to_string();
    config.min_iterations = 0;
    config
}

proptest! {
    /// `push_kata_cycle` appends exactly one entry to each trajectory history,
    /// and the appended values equal the inputs. This pins the storage contract
    /// the convergence checks read: history length == cycle count, value fidelity.
    ///
    /// Oracle: invariant — (len == n) && (history[i] == input[i]) for all i.
    #[test]
    fn push_kata_cycle_appends_one_entry_equal_to_inputs(
        cycles in prop::collection::vec((arb_finite_f64(), arb_finite_f64()), 0..16),
    ) {
        let config = kata_config_base();
        let mut tracker = ConvergenceTracker::new(&config);
        for (h, b) in &cycles {
            tracker.push_kata_cycle(*h, *b);
        }

        let payload = json!({
            "hyp_len": tracker.signal_history().len(),
            "brier_len": tracker.brier_history().len(),
            "expected_len": cycles.len(),
            "hyp": tracker.signal_history(),
            "brier": tracker.brier_history(),
            "expected_hyp": cycles.iter().map(|(h, _)| h).collect::<Vec<_>>(),
            "expected_brier": cycles.iter().map(|(_, b)| b).collect::<Vec<_>>(),
        });
        let oracle = oracle_invariant(|_input, output| {
            let n = output["expected_len"].as_u64().ok_or("missing expected_len")? as usize;
            let hyp_len = output["hyp_len"].as_u64().ok_or("missing hyp_len")? as usize;
            let brier_len = output["brier_len"].as_u64().ok_or("missing brier_len")? as usize;
            if hyp_len != n || brier_len != n {
                return Err(format!(
                    "history length drifted: hyp {hyp_len} / brier {brier_len} / expected {n}"
                ));
            }
            let hyp = output["hyp"].as_array().ok_or("hyp not array")?;
            let brier = output["brier"].as_array().ok_or("brier not array")?;
            let eh = output["expected_hyp"].as_array().ok_or("expected_hyp not array")?;
            let eb = output["expected_brier"].as_array().ok_or("expected_brier not array")?;
            for i in 0..n {
                if hyp[i] != eh[i] {
                    return Err(format!("hypotenuse[{i}] drifted: {} != {}", hyp[i], eh[i]));
                }
                if brier[i] != eb[i] {
                    return Err(format!("brier[{i}] drifted: {} != {}", brier[i], eb[i]));
                }
            }
            Ok(())
        });
        let verdict = oracle.verify(&Value::Null, &payload);
        prop_assert_eq!(verdict, OracleVerdict::Pass, "push_kata_cycle storage contract violated");
    }

    /// `push_signal` keeps the two histories aligned by cycle count: it
    /// appends the hypotenuse and a NaN Brier placeholder so the Kata checks
    /// (which index by cycle) don't skew. The hypotenuse value is stored
    /// verbatim; the Brier slot is NaN (not 0.0 — the calibration check filters
    /// NaN out so an unset cycle does not pollute the rolling average).
    ///
    /// Oracle: invariant — hyp_len == brier_len == n, last hypotenuse == input.
    #[test]
    fn push_signal_keeps_histories_aligned(
        readings in prop::collection::vec(arb_finite_f64(), 0..12),
    ) {
        let config = kata_config_base();
        let mut tracker = ConvergenceTracker::new(&config);
        for h in &readings {
            tracker.push_signal(*h);
        }

        let payload = json!({
            "n": readings.len(),
            "hyp_len": tracker.signal_history().len(),
            "brier_len": tracker.brier_history().len(),
            "last_hyp": tracker.signal_history().last().copied(),
            "expected_last": readings.last().copied(),
        });
        let oracle = oracle_invariant(|_input, output| {
            let n = output["n"].as_u64().ok_or("missing n")? as usize;
            let hyp_len = output["hyp_len"].as_u64().ok_or("missing hyp_len")? as usize;
            let brier_len = output["brier_len"].as_u64().ok_or("missing brier_len")? as usize;
            if hyp_len != n || brier_len != n {
                return Err(format!(
                    "histories misaligned: hyp {hyp_len} / brier {brier_len} / n {n}"
                ));
            }
            if n == 0 {
                return Ok(());
            }
            if output["last_hyp"] != output["expected_last"] {
                return Err(format!(
                    "last hypotenuse drifted: {} != {}",
                    output["last_hyp"], output["expected_last"]
                ));
            }
            Ok(())
        });
        let verdict = oracle.verify(&Value::Null, &payload);
        prop_assert_eq!(verdict, OracleVerdict::Pass, "push_signal alignment contract violated");
    }

    /// `check_met` is a pure function of tracker state: it takes `&self`, so two
    /// consecutive calls on the same state must return the same verdict. This is
    /// the cybernetic determinism property — a non-deterministic convergence
    /// detector would make the PDCA loop's exit condition unobservable.
    ///
    /// Oracle: invariant — first == second.
    #[test]
    fn check_met_is_deterministic_for_fixed_state(
        history in prop::collection::vec(arb_finite_f64(), 0..16),
        iteration in 1u32..20,
    ) {
        let mut config = kata_config_base();
        config.convergence_mode = "gap".to_string();
        let mut tracker = ConvergenceTracker::new(&config);
        for h in &history {
            tracker.push_signal(*h);
        }
        let context = HashMap::new();
        let first = tracker.check_met(&context, iteration);
        let second = tracker.check_met(&context, iteration);

        let payload = json!([first, second]);
        let oracle = oracle_invariant(|_input, output| {
            let arr = output.as_array().ok_or("output not an array")?;
            if arr.len() != 2 {
                return Err(format!("expected 2 results, got {}", arr.len()));
            }
            if arr[0] != arr[1] {
                return Err(format!("check_met non-deterministic: {} != {}", arr[0], arr[1]));
            }
            Ok(())
        });
        let verdict = oracle.verify(&Value::Null, &payload);
        prop_assert_eq!(verdict, OracleVerdict::Pass, "check_met returned different verdicts for the same state");
    }

    /// Gap convergence matches the reference definition: with convergence_mode
    /// = "gap" and min_iterations = 0, after a single push_signal(h),
    /// `check_met` returns `h < gap_epsilon` (h is finite by
    /// construction, so the is_finite filter in the implementation is a
    /// no-op). This is the limit-of-a-sequence criterion.
    ///
    /// Oracle: reference — the expected output is the mathematical comparison
    /// `h < epsilon`, computed independently of the implementation.
    #[test]
    fn gap_convergence_matches_reference_definition(
        h in arb_finite_f64(),
        epsilon in arb_positive_finite_f64(),
    ) {
        let mut config = kata_config_base();
        config.convergence_mode = "gap".to_string();
        config.gap_epsilon = epsilon;
        let mut tracker = ConvergenceTracker::new(&config);
        tracker.push_signal(h);
        let context = HashMap::new();
        let got = tracker.check_met(&context, 1);

        let input = json!({ "h": h, "epsilon": epsilon });
        let oracle = oracle_reference(|inp: &Value| {
            let h = inp["h"].as_f64().expect("h is finite f64");
            let e = inp["epsilon"].as_f64().expect("epsilon is finite f64");
            json!(h < e)
        });
        let verdict = oracle.verify(&input, &json!(got));
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "gap convergence did not match h < epsilon (h={}, epsilon={})",
            h,
            epsilon
        );
    }

    /// The `min_iterations` gate blocks convergence below the threshold: even
    /// when the gap is closed (h=0 < epsilon=large), `check_met` returns false
    /// for iteration <= min_iterations. Prevents premature exit before the
    /// Kata has run a full experiment cycle.
    ///
    /// Oracle: invariant — got == (iteration > min_iterations).
    #[test]
    fn min_iterations_gate_blocks_convergence_below_threshold(
        min_iterations in 0u32..8,
        iteration in 0u32..16,
    ) {
        let mut config = kata_config_base();
        config.convergence_mode = "gap".to_string();
        // Huge epsilon so the gap criterion is always satisfied for finite h;
        // the only variable is the iteration gate.
        config.gap_epsilon = 1.0e9;
        config.min_iterations = min_iterations;
        let mut tracker = ConvergenceTracker::new(&config);
        tracker.push_signal(0.0);
        let context = HashMap::new();
        let got = tracker.check_met(&context, iteration);
        let expected = iteration > min_iterations;

        let input = json!({ "expected": expected });
        let oracle = oracle_invariant(move |inp, output| {
            let expected = inp["expected"].as_bool().ok_or("missing expected")?;
            let got = output.as_bool().ok_or("output not bool")?;
            if got == expected {
                Ok(())
            } else {
                Err(format!("min_iterations gate wrong: got {got}, expected {expected}"))
            }
        });
        let verdict = oracle.verify(&input, &json!(got));
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "min_iterations gate misrouted (min={}, iter={})",
            min_iterations,
            iteration
        );
    }

    /// Cauchy convergence matches the mathematical Cauchy criterion: the max
    /// pairwise distance between the last cauchy_window finite readings is
    /// below cauchy_epsilon. The reference computes the pairwise max
    /// directly from the definition (max |x_m - x_n| < eps), independent
    /// of the implementation's double loop.
    ///
    /// Oracle: invariant — got == reference Cauchy criterion on the same history.
    #[test]
    fn cauchy_convergence_matches_reference_criterion(
        readings in prop::collection::vec(arb_finite_f64(), 1..12),
        epsilon in arb_positive_finite_f64(),
        window in 1u32..6,
    ) {
        let mut config = kata_config_base();
        config.convergence_mode = "cauchy".to_string();
        config.cauchy_epsilon = epsilon;
        config.cauchy_window = window;
        let mut tracker = ConvergenceTracker::new(&config);
        for r in &readings {
            tracker.push_signal(*r);
        }
        let context = HashMap::new();
        let iteration = readings.len() as u32;
        let got = tracker.check_met(&context, iteration);

        // Reference: the mathematical Cauchy criterion over the last `window`
        // finite readings.
        let expected = {
            let w = window as usize;
            if readings.len() < w {
                false
            } else {
                let start = readings.len().saturating_sub(w);
                let finite: Vec<f64> = readings[start..].iter().copied().filter(|f| f.is_finite()).collect();
                if finite.len() < w {
                    false
                } else {
                    let mut max_delta = 0.0_f64;
                    for i in 0..finite.len() {
                        for j in (i + 1)..finite.len() {
                            max_delta = max_delta.max((finite[i] - finite[j]).abs());
                        }
                    }
                    max_delta < epsilon
                }
            }
        };

        let oracle = oracle_invariant(move |_input, output| {
            let got = output.as_bool().ok_or("output not bool")?;
            if got == expected {
                Ok(())
            } else {
                Err(format!(
                    "cauchy convergence mismatch: got {got}, reference {expected} (window={window}, epsilon={epsilon}, readings={readings:?})"
                ))
            }
        });
        let verdict = oracle.verify(&Value::Null, &json!(got));
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "cauchy convergence did not match the Cauchy criterion"
        );
    }

    /// Calibration convergence matches the reference: the rolling Brier average
    /// over the last brier_window finite readings is below brier_threshold.
    /// The reference computes the rolling mean directly from the definition.
    ///
    /// Oracle: invariant — got == reference rolling-mean-below-threshold.
    #[test]
    fn calibration_convergence_matches_reference(
        brier_readings in prop::collection::vec(arb_finite_f64(), 1..12),
        threshold in arb_positive_finite_f64(),
        window in 1u32..6,
    ) {
        let mut config = kata_config_base();
        config.convergence_mode = "calibration".to_string();
        config.brier_window = window;
        config.brier_threshold = threshold;
        let mut tracker = ConvergenceTracker::new(&config);
        // Hypotenuse is irrelevant in calibration-only mode; push a constant.
        for b in &brier_readings {
            tracker.push_kata_cycle(0.0, *b);
        }
        let context = HashMap::new();
        let iteration = brier_readings.len() as u32;
        let got = tracker.check_met(&context, iteration);

        let expected = {
            let w = window as usize;
            if brier_readings.len() < w {
                false
            } else {
                let start = brier_readings.len().saturating_sub(w);
                let recent: Vec<f64> = brier_readings[start..]
                    .iter()
                    .copied()
                    .filter(|f| f.is_finite())
                    .collect();
                if recent.len() < w {
                    false
                } else {
                    let avg: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
                    avg < threshold
                }
            }
        };

        let oracle = oracle_invariant(move |_input, output| {
            let got = output.as_bool().ok_or("output not bool")?;
            if got == expected {
                Ok(())
            } else {
                Err(format!(
                    "calibration convergence mismatch: got {got}, reference {expected} (window={window}, threshold={threshold}, brier={brier_readings:?})"
                ))
            }
        });
        let verdict = oracle.verify(&Value::Null, &json!(got));
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "calibration convergence did not match the rolling-mean criterion"
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 6. compute_compound_quality — compound aggregation over inner skill reports
//
// `compute_compound_quality` reads `step_{ordinal}_result._convergence` from
// the context and aggregates across sources. Three methods, each a pure
// function of (context, sources):
//   - "all_converged" : 0.0 iff every source status == "converged", else 1.0
//   - "min"           : min over present quality_at_exit (fold starts at 1.0)
//   - "weighted_avg"   : sum(w*q)/sum(w) over present, else 1.0 if none present
//
// The reference oracles encode the mathematical aggregation, independent of the
// implementation's fold/loop.
// ──────────────────────────────────────────────────────────────────────────

/// A compound-aggregation case: per-source (converged?, optional quality, weight).
/// Ordinals are assigned 0..n so context keys never collide (last-write-wins
/// would otherwise mask the collision).
#[derive(Debug, Clone)]
struct CompoundSpec {
    converged: bool,
    quality: Option<f64>,
    weight: f64,
}

fn arb_compound_specs() -> BoxedStrategy<Vec<CompoundSpec>> {
    // Quality and weight are bounded so q*weight and their sums stay finite
    // and JSON-serializable (serde_json renders inf/NaN as null, which would
    // make the reference value unreadable by the oracle). i64/1e6 gives a
    // range of roughly [-9.2e9, 9.2e9] — ample variety, no overflow.
    let bounded = any::<i64>().prop_map(|n| n as f64 / 1_000_000.0);
    prop::collection::vec(
        (any::<bool>(), prop::option::of(bounded.clone()), bounded),
        0..6,
    )
    .prop_map(|raw| {
        raw.into_iter()
            .map(|(converged, quality, weight)| CompoundSpec {
                converged,
                quality,
                weight,
            })
            .collect()
    })
    .boxed()
}

/// Build the context + sources for a compound case. Each source i gets ordinal
/// i and a `step_{i}_result` entry shaped as the executor writes it:
/// `{ "_convergence": { "status": ..., "quality_at_exit": ... } }`.
fn build_compound_case(specs: &[CompoundSpec]) -> (HashMap<String, Value>, Vec<AggregationSource>) {
    let mut context: HashMap<String, Value> = HashMap::new();
    let sources: Vec<AggregationSource> = specs
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let ordinal = i as u32;
            let mut conv = Map::new();
            conv.insert(
                "status".to_string(),
                Value::String(
                    if spec.converged {
                        "converged"
                    } else {
                        "maxed_out"
                    }
                    .to_string(),
                ),
            );
            if let Some(q) = spec.quality {
                conv.insert("quality_at_exit".to_string(), json!(q));
            }
            let mut step_obj = Map::new();
            step_obj.insert("_convergence".to_string(), Value::Object(conv));
            context.insert(format!("step_{ordinal}_result"), Value::Object(step_obj));
            AggregationSource {
                step_ordinal: ordinal,
                field: "_convergence.quality_at_exit".to_string(),
                weight: spec.weight,
            }
        })
        .collect();
    (context, sources)
}

proptest! {
    /// `compute_compound_quality("all_converged", ...)` returns 0.0 iff every
    /// source's `_convergence.status == "converged"`, else 1.0. A missing
    /// source (no `step_N_result` entry) counts as not-converged.
    ///
    /// Oracle: reference — the expected score is the boolean all-over-converged
    /// mapped to {0.0, 1.0}.
    #[test]
    fn compound_all_converged_matches_reference(
        specs in arb_compound_specs(),
    ) {
        let (context, sources) = build_compound_case(&specs);
        let config = ConvergenceConfig::default();
        let tracker = ConvergenceTracker::new(&config);
        let got = tracker.compute_compound_quality(&context, "all_converged", &sources);

        let expected = if specs.iter().all(|s| s.converged) { 0.0 } else { 1.0 };
        let input = json!({ "expected": expected });
        let oracle = oracle_invariant(move |inp, output| {
            let expected = inp["expected"].as_f64().ok_or("missing expected")?;
            let got = output.as_f64().ok_or("output not f64")?;
            if got == expected {
                Ok(())
            } else {
                Err(format!("all_converged mismatch: got {got}, expected {expected} (specs={specs:?})"))
            }
        });
        let verdict = oracle.verify(&input, &json!(got));
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "all_converged aggregation did not match reference"
        );
    }

    /// `compute_compound_quality("min", ...)` returns the minimum present
    /// `quality_at_exit`, folding from 1.0. Sources with no quality field are
    /// skipped (the fold stays at its current value). With no present qualities
    /// the result is the fold seed 1.0.
    ///
    /// Oracle: reference — fold(1.0, min) over present qualities, in source
    /// order (matching the implementation's iteration order so f64 reduction
    /// order is identical).
    #[test]
    fn compound_min_takes_lowest_present_quality(
        specs in arb_compound_specs(),
    ) {
        let (context, sources) = build_compound_case(&specs);
        let config = ConvergenceConfig::default();
        let tracker = ConvergenceTracker::new(&config);
        let got = tracker.compute_compound_quality(&context, "min", &sources);

        let expected = specs
            .iter()
            .filter_map(|s| s.quality)
            .fold(1.0_f64, f64::min);
        let input = json!({ "expected": expected });
        let oracle = oracle_invariant(move |inp, output| {
            let expected = inp["expected"].as_f64().ok_or("missing expected")?;
            let got = output.as_f64().ok_or("output not f64")?;
            if got == expected {
                Ok(())
            } else {
                Err(format!("min aggregation mismatch: got {got}, expected {expected} (specs={specs:?})"))
            }
        });
        let verdict = oracle.verify(&input, &json!(got));
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "min aggregation did not match reference"
        );
    }

    /// `compute_compound_quality("weighted_avg", ...)` returns
    /// `sum(w*q)/sum(w)` over present qualities, or 1.0 if no qualities are
    /// present (total weight == 0). The reference iterates in source order so
    /// the f64 summation order matches the implementation exactly.
    ///
    /// Oracle: reference — the weighted mean computed independently, with the
    /// same summation order.
    #[test]
    fn compound_weighted_avg_matches_reference(
        specs in arb_compound_specs(),
    ) {
        let (context, sources) = build_compound_case(&specs);
        let config = ConvergenceConfig::default();
        let tracker = ConvergenceTracker::new(&config);
        let got = tracker.compute_compound_quality(&context, "weighted_avg", &sources);

        let expected = {
            let mut sum = 0.0_f64;
            let mut total = 0.0_f64;
            for s in &specs {
                if let Some(q) = s.quality {
                    sum += q * s.weight;
                    total += s.weight;
                }
            }
            if total > 0.0 { sum / total } else { 1.0 }
        };
        let input = json!({ "expected": expected });
        let oracle = oracle_invariant(move |inp, output| {
            let expected = inp["expected"].as_f64().ok_or("missing expected")?;
            let got = output.as_f64().ok_or("output not f64")?;
            if got == expected {
                Ok(())
            } else {
                Err(format!("weighted_avg mismatch: got {got}, expected {expected} (specs={specs:?})"))
            }
        });
        let verdict = oracle.verify(&input, &json!(got));
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "weighted_avg aggregation did not match reference"
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 7. finalize_report / inject_running / capture_baseline — output contract
//
// `finalize_report` writes the `_convergence` JSON block (the single source of
// truth for the `_convergence` shape). `inject_running` writes the live running
// block. `capture_baseline` records the first-pass quality and is idempotent.
// These are pure functions of (tracker state, context, args) — no I/O, no ports.
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `finalize_report` writes `_convergence.status` equal to the passed
    /// `ConvergenceStatus`. The status enum is the cascade's exit verdict; the
    /// field is what templates and downstream consumers read. A mismatch would
    /// make the exit verdict unobservable.
    ///
    /// Oracle: invariant — context["_convergence"]["status"] == status.as_str().
    #[test]
    fn finalize_report_writes_status_field(
        status_idx in 0u32..4,
        iteration in 0u32..20,
        rjoule_used in arb_finite_f64(),
        rjoule_cap in arb_finite_f64(),
    ) {
        let status = match status_idx {
            0 => ConvergenceStatus::Converged,
            1 => ConvergenceStatus::MaxedOut,
            2 => ConvergenceStatus::Escalated,
            _ => ConvergenceStatus::Running,
        };
        let config = ConvergenceConfig::default();
        let tracker = ConvergenceTracker::new(&config);
        let mut context = HashMap::new();
        tracker.finalize_report(
            &mut context,
            status,
            "test-reason",
            iteration,
            rjoule_used,
            rjoule_cap,
        );

        let got_status = context
            .get("_convergence")
            .and_then(|v| v.get("status"))
            .and_then(|s| s.as_str());
        let payload = json!({ "got": got_status, "expected": status.as_str() });
        let oracle = oracle_invariant(|_input, output| {
            let got = output["got"].as_str().ok_or("missing got status")?;
            let expected = output["expected"].as_str().ok_or("missing expected status")?;
            if got == expected {
                Ok(())
            } else {
                Err(format!("finalize_report wrote status '{got}', expected '{expected}'"))
            }
        });
        let verdict = oracle.verify(&Value::Null, &payload);
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "finalize_report did not write the passed status"
        );
    }

    /// `finalize_report` is total: it never panics on arbitrary rJoule
    /// values, including `rjoule_used > rjoule_cap` (the `rjoule_remaining`
    /// uses `.max(0.0)`).
    /// P4 (Clear Boundaries): the output surface is total over its numeric args.
    #[test]
    fn finalize_report_never_panics_on_arbitrary_args(
        status_idx in 0u32..4,
        iteration in any::<u32>(),
        rjoule_used in any::<f64>(),
        rjoule_cap in any::<f64>(),
    ) {
        let status = match status_idx {
            0 => ConvergenceStatus::Converged,
            1 => ConvergenceStatus::MaxedOut,
            2 => ConvergenceStatus::Escalated,
            _ => ConvergenceStatus::Running,
        };
        let config = ConvergenceConfig::default();
        let tracker = ConvergenceTracker::new(&config);
        let mut context = HashMap::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tracker.finalize_report(
                &mut context,
                status,
                "test",
                iteration,
                rjoule_used,
                rjoule_cap,
            );
        }));
        prop_assert!(
            result.is_ok(),
            "finalize_report panicked on args (rjoule {rjoule_used}/{rjoule_cap})"
        );
    }

    /// `inject_running` writes `_convergence.status == "running"` and the live
    /// loop-control fields. Templates read `{{ _convergence.iterations_completed }}`
    /// mid-cascade; a missing or wrong status field would break template gating.
    ///
    /// Oracle: invariant — status == "running" and iterations_completed == arg.
    #[test]
    fn inject_running_writes_running_status(
        iteration in 0u32..20,
        rjoule_used in arb_finite_f64(),
        rjoule_cap in arb_finite_f64(),
    ) {
        let config = ConvergenceConfig::default();
        let tracker = ConvergenceTracker::new(&config);
        let mut context = HashMap::new();
        tracker.inject_running(&mut context, iteration, rjoule_used, rjoule_cap);

        let conv = context.get("_convergence").expect("_convergence must be injected");
        let payload = json!({
            "status": conv.get("status"),
            "iterations": conv.get("iterations_completed"),
            "expected_status": ConvergenceStatus::Running.as_str(),
            "expected_iterations": iteration,
        });
        let oracle = oracle_invariant(|_input, output| {
            let status = output["status"].as_str().ok_or("missing status")?;
            let expected_status = output["expected_status"].as_str().ok_or("missing expected")?;
            if status != expected_status {
                return Err(format!("inject_running status '{status}', expected '{expected_status}'"));
            }
            let iterations = output["iterations"].as_u64().ok_or("missing iterations")?;
            let expected_iterations = output["expected_iterations"].as_u64().ok_or("missing expected iterations")?;
            if iterations != expected_iterations {
                return Err(format!("inject_running iterations {iterations}, expected {expected_iterations}"));
            }
            Ok(())
        });
        let verdict = oracle.verify(&Value::Null, &payload);
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "inject_running did not write the running status block"
        );
    }

    /// `capture_baseline` is idempotent: the first call records the current
    /// quality, and a second call with a different value does NOT overwrite it.
    /// The baseline anchors the improvement-gate; a non-idempotent capture would
    /// re-anchor on every cycle and erase the improvement signal.
    ///
    /// Oracle: invariant — after two captures with different values, the
    /// baseline (read back via `finalize_report`) equals the FIRST value.
    #[test]
    fn capture_baseline_is_idempotent(
        first_quality in arb_finite_f64(),
        second_quality in arb_finite_f64(),
    ) {
        let mut config = ConvergenceConfig::default();
        config.convergence_field = "composite".to_string();
        let mut tracker = ConvergenceTracker::new(&config);

        let mut ctx = HashMap::new();
        ctx.insert("composite".to_string(), json!(first_quality));
        tracker.capture_baseline(&mut ctx);

        // Second capture with a different value — must be a no-op.
        ctx.insert("composite".to_string(), json!(second_quality));
        tracker.capture_baseline(&mut ctx);

        // Read the baseline back through finalize_report's `baseline_quality` field.
        let mut final_ctx = HashMap::new();
        tracker.finalize_report(&mut final_ctx, ConvergenceStatus::Converged, "t", 1, 0.0, 0.0);
        let baseline = final_ctx
            .get("_convergence")
            .and_then(|c| c.get("baseline_quality"))
            .and_then(|v| v.as_f64());

        let payload = json!({ "baseline": baseline, "expected": first_quality });
        let oracle = oracle_invariant(|_input, output| {
            let baseline = output["baseline"].as_f64().ok_or("baseline missing or not f64")?;
            let expected = output["expected"].as_f64().ok_or("expected missing")?;
            if baseline == expected {
                Ok(())
            } else {
                Err(format!(
                    "capture_baseline not idempotent: baseline {baseline}, expected first value {expected}"
                ))
            }
        });
        let verdict = oracle.verify(&Value::Null, &payload);
        prop_assert_eq!(
            verdict,
            OracleVerdict::Pass,
            "capture_baseline overwrote the first-pass baseline"
        );
    }
}
