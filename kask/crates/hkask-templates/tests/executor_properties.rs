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

use hkask_templates::load_manifest_from_yaml;
use hkask_test_harness::{OracleVerdict, arb_json_value, oracle_invariant, oracle_reference};
use hkask_types::tool_response::{parse_tool_response, unwrap_tool_envelope};
use proptest::prelude::*;
use serde_json::Value;

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
        let unwrapped = unwrap_tool_envelope(bare.clone());

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
