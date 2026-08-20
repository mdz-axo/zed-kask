//! Property tests for the inference IPC message types that `InferenceIpcServer`
//! serializes over the Unix socket.
//!
//! The deleted stub-based tests (`StubInferencePort`, `StubToolPort`,
//! `StubSkillExec`, `CountingMockPort`) exercised the async dispatch path of
//! `InferenceIpcServer::dispatch` and the prompt-length recall gate. Both need
//! a real port implementation to drive — constructing one would be a forbidden
//! stub. The genuinely pure, stub-free surface is the **serialization layer**:
//! `InferenceRequest` / `InferenceResponse` / `InferenceOutcome` round-trip
//! through `serde_json` preserving every field, variant, and nested JSON
//! payload. The server does `serde_json::from_str::<InferenceRequest>(line)` on
//! read and `serde_json::to_string(&InferenceResponse)` on write, so lossless
//! roundtrip of these types is exactly the property the IPC layer depends on.
//!
//! ## Why a float-tolerant invariant, not `oracle_reference` identity
//!
//! `serde_json`'s float parser is not correctly-rounded for every finite `f64`
//! (it can be off by 1–2 ULP for extreme magnitudes), so
//! `to_string(from_str(to_string(x)))` is not bit-identical to `to_string(x)`
//! for arbitrary `f64` leaves. That is a `serde_json` property the IPC layer
//! inherits, not a bug in the IPC types. The tests therefore assert
//! **float-tolerant structural roundtrip**: every field, variant key, array
//! length, and object key is preserved exactly, and numbers are preserved
//! within a tolerance far tighter than any structural change (~1e-9 relative,
//! vs. serde_json's ~1e-16 ULP noise). A real serialization bug (a dropped
//! field, a mangled variant, a lost array element) differs by O(1) and fails;
//! only serde_json's float-parser noise is absorbed.
//!
//! # Gaps (logic that is NOT testable from `tests/`)
//!
//! - `context_injector::BridgeContextInjector::should_recall` is the
//!   prompt-length threshold gate (≥20 chars AND ≥3 words). It is a private
//!   associated fn, so it is unreachable from an integration test. The only
//!   public entry that exercises it, `inject_context`, requires a
//!   `MemoryPort` — constructing one would be a stub. The same applies to the
//!   curator injector's `should_recall` and the
//!   `(recall_min_confidence + 0.1).min(1.0)` clamp in `inject_static_context`.
//! - `InferenceIpcServer::dispatch` (the routing the stub ports drove) is async
//!   and takes live `InferencePort`/`ToolPort`/`SkillExecPort` trait objects.

use hkask_types::inference_ipc::{
    InferenceMethod, InferenceOutcome, InferenceParams, InferenceRequest, InferenceResponse,
};
use proptest::prelude::*;
use serde_json::{Value as JsonValue, json};

/// Minimal inline port of the deleted `hkask-test-harness` Oracle surface —
/// only what these property tests use.
#[derive(Debug, Clone, PartialEq, Eq)]
enum OracleVerdict {
    Pass,
    Fail(String),
}

trait Oracle: Send + Sync {
    fn verify(&self, input: &JsonValue, output: &JsonValue) -> OracleVerdict;
}

fn oracle_invariant<F>(check: F) -> Box<dyn Oracle>
where
    F: Fn(&JsonValue, &JsonValue) -> Result<(), String> + Send + Sync + 'static,
{
    struct InvariantOracle<F>(F);
    impl<F> Oracle for InvariantOracle<F>
    where
        F: Fn(&JsonValue, &JsonValue) -> Result<(), String> + Send + Sync,
    {
        fn verify(&self, input: &JsonValue, output: &JsonValue) -> OracleVerdict {
            match (self.0)(input, output) {
                Ok(()) => OracleVerdict::Pass,
                Err(msg) => OracleVerdict::Fail(msg),
            }
        }
    }
    Box::new(InvariantOracle(check))
}

/// Structured-JSON strategy (inlined from the deleted harness).
fn arb_json_value() -> BoxedStrategy<JsonValue> {
    let leaf = prop_oneof![
        Just(JsonValue::Null),
        any::<bool>().prop_map(JsonValue::Bool),
        any::<i64>().prop_map(|n| serde_json::json!(n)),
        any::<u64>().prop_map(|n| serde_json::json!(n)),
        any::<f64>()
            .prop_filter("must be finite", |f| f.is_finite())
            .prop_map(|n| serde_json::json!(n)),
        any::<String>().prop_map(JsonValue::String),
    ];
    leaf.prop_recursive(
        4,
        64,
        8,
        |element| {
            prop_oneof![
                prop::collection::vec(element.clone(), 0..8).prop_map(JsonValue::Array),
                prop::collection::vec((any::<String>(), element), 0..8).prop_map(|pairs| {
                    let mut map = serde_json::Map::new();
                    for (k, v) in pairs {
                        map.insert(k, v);
                    }
                    JsonValue::Object(map)
                }),
            ]
            .boxed()
        },
    )
    .boxed()
}

/// Build an `InferenceParams` with every field `None`/default except the one
/// under test. Keeps the roundtrip focused on the JSON-valued field.
fn bare_params() -> InferenceParams {
    InferenceParams {
        prompt: None,
        messages: None,
        images: None,
        parameters: Default::default(),
        model_override: None,
        tools: None,
        embed_model: None,
        embed_texts: None,
        media_op: None,
        media_prompt: None,
        media_image_url: None,
        media_audio_url: None,
        media_text: None,
        media_voice: None,
        media_size: None,
        media_count: None,
        media_strength: None,
        media_scale: None,
        media_duration: None,
        media_language: None,
        tool_server: None,
        tool_name: None,
        tool_args: None,
        tool_allowlist: None,
        skill_name: None,
        skill_task: None,
        worktree_prompt: None,
        worktree_title: None,
        worktree_name: None,
        worktree_base_ref: None,
    }
}

/// Float-tolerant structural JSON equality. Booleans, strings, null, object
/// keys, and array lengths are compared exactly; integers exactly; floats with
/// a tolerance that absorbs `serde_json`'s 1–2 ULP parser noise while still
/// rejecting any structural change.
///
/// Returns `Err(message)` on the first mismatch (for use with `oracle_invariant`).
fn json_approx_eq(a: &JsonValue, b: &JsonValue) -> Result<(), String> {
    match (a, b) {
        (JsonValue::Null, JsonValue::Null) => Ok(()),
        (JsonValue::Bool(x), JsonValue::Bool(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(format!("bool mismatch: {x} vs {y}"))
            }
        }
        (JsonValue::String(x), JsonValue::String(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(format!("string mismatch: {x:?} vs {y:?}"))
            }
        }
        (JsonValue::Number(x), JsonValue::Number(y)) => {
            if x == y {
                return Ok(());
            }
            // Both must be representable as f64 (JSON numbers always are in serde_json::Value).
            match (x.as_f64(), y.as_f64()) {
                (Some(xf), Some(yf)) => {
                    let abs_err = (xf - yf).abs();
                    // Relative tolerance vs the larger magnitude; absolute floor for near-zero.
                    let scale = xf.abs().max(yf.abs());
                    if abs_err <= 1e-12 || (scale > 0.0 && abs_err / scale <= 1e-9) {
                        Ok(())
                    } else {
                        Err(format!("float mismatch: {xf} vs {yf} (abs_err {abs_err})"))
                    }
                }
                _ => Err(format!("number mismatch: {x} vs {y}")),
            }
        }
        (JsonValue::Array(x), JsonValue::Array(y)) => {
            if x.len() != y.len() {
                return Err(format!("array length mismatch: {} vs {}", x.len(), y.len()));
            }
            for (xi, yi) in x.iter().zip(y.iter()) {
                json_approx_eq(xi, yi)?;
            }
            Ok(())
        }
        (JsonValue::Object(x), JsonValue::Object(y)) => {
            if x.len() != y.len() {
                return Err(format!("object size mismatch: {} vs {}", x.len(), y.len()));
            }
            for (k, v) in x {
                match y.get(k) {
                    Some(yv) => json_approx_eq(v, yv)?,
                    None => return Err(format!("missing key {k:?} in roundtripped object")),
                }
            }
            Ok(())
        }
        _ => Err(format!(
            "type mismatch: {a} (kind {}) vs {b} (kind {})",
            kind(a),
            kind(b)
        )),
    }
}

fn kind(v: &JsonValue) -> &'static str {
    match v {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

/// The shared oracle for the request roundtrips: the roundtripped `Value` must
/// be float-tolerant-equal to the original `Value`.
fn roundtrip_oracle() -> Box<dyn Oracle> {
    oracle_invariant(|input: &JsonValue, output: &JsonValue| json_approx_eq(input, output))
}

proptest! {
    /// An `InferenceRequest` carrying an arbitrary JSON `tool_args` payload
    /// round-trips through `serde_json` preserving every field. The IPC server
    /// reads requests with `serde_json::from_str`, so any drift here corrupts
    /// every tool dispatch.
    ///
    /// Oracle: [`oracle_invariant`] — float-tolerant structural equality (see
    /// module docs for why not bit-exact `oracle_reference`).
    #[test]
    fn inference_request_roundtrip_preserves_tool_args(
        id in any::<u64>(),
        tool_args in arb_json_value(),
    ) {
        let oracle = roundtrip_oracle();

        let request = InferenceRequest {
            id,
            method: InferenceMethod::ToolInvoke,
            params: {
                let mut p = bare_params();
                p.tool_server = Some("codegraph".to_string());
                p.tool_name = Some("list_nodes".to_string());
                p.tool_args = Some(tool_args);
                p.tool_allowlist = Some(vec!["codegraph/list_nodes".to_string()]);
                p
            },
        };
        let original = serde_json::to_value(&request).expect("request -> Value");
        let wire = serde_json::to_string(&request).expect("request serializes");
        let reparsed: InferenceRequest =
            serde_json::from_str(&wire).expect("request re-parses");
        let roundtripped = serde_json::to_value(&reparsed).expect("reparsed -> Value");

        prop_assert_eq!(
            oracle.verify(&original, &roundtripped),
            OracleVerdict::Pass,
            "InferenceRequest tool_args roundtrip lost structure for wire: {}",
            wire
        );
    }

    /// An `InferenceResponse` whose outcome is `ToolResult { result: <json> }`
    /// round-trips preserving the outcome variant (the `tool_result` key
    /// distinguishes it from the `Result` variant under `#[serde(untagged)]`)
    /// and the inner JSON payload.
    ///
    /// Oracle: [`oracle_invariant`] — variant key preserved exactly AND inner
    /// JSON float-tolerant-equal to the original.
    #[test]
    fn inference_response_tool_result_roundtrip_preserves_variant_and_json(
        id in any::<u64>(),
        result in arb_json_value(),
    ) {
        let oracle = oracle_invariant(|input: &JsonValue, output: &JsonValue| {
            if output["variant"] != "tool_result" {
                return Err(format!(
                    "outcome variant not preserved: expected tool_result, got {}",
                    output["variant"]
                ));
            }
            json_approx_eq(input, &output["result"])
        });

        let response = InferenceResponse {
            id,
            outcome: InferenceOutcome::ToolResult { result: result.clone() },
        };
        let wire = serde_json::to_string(&response).expect("response serializes");
        let reparsed: InferenceResponse =
            serde_json::from_str(&wire).expect("response re-parses");

        prop_assert!(
            matches!(&reparsed.outcome, InferenceOutcome::ToolResult { .. }),
            "deserialized outcome was {:?}, expected ToolResult",
            reparsed.outcome
        );
        let InferenceOutcome::ToolResult { result: inner } = reparsed.outcome else {
            unreachable!();
        };
        let output = json!({ "variant": "tool_result", "result": inner });

        prop_assert_eq!(
            oracle.verify(&result, &output),
            OracleVerdict::Pass,
            "ToolResult outcome roundtrip lost data for wire: {}",
            wire
        );
    }

    /// An `InferenceResponse` whose outcome is `Media { media: <json> }`
    /// round-trips preserving the `Media` variant and the inner JSON payload.
    ///
    /// Oracle: [`oracle_invariant`] — variant key preserved exactly AND inner
    /// JSON float-tolerant-equal to the original.
    #[test]
    fn inference_response_media_roundtrip_preserves_variant_and_json(
        id in any::<u64>(),
        media in arb_json_value(),
    ) {
        let oracle = oracle_invariant(|input: &JsonValue, output: &JsonValue| {
            if output["variant"] != "media" {
                return Err(format!(
                    "outcome variant not preserved: expected media, got {}",
                    output["variant"]
                ));
            }
            json_approx_eq(input, &output["result"])
        });

        let response = InferenceResponse {
            id,
            outcome: InferenceOutcome::Media { media: media.clone() },
        };
        let wire = serde_json::to_string(&response).expect("response serializes");
        let reparsed: InferenceResponse =
            serde_json::from_str(&wire).expect("response re-parses");

        prop_assert!(
            matches!(&reparsed.outcome, InferenceOutcome::Media { .. }),
            "deserialized outcome was {:?}, expected Media",
            reparsed.outcome
        );
        let InferenceOutcome::Media { media: inner } = reparsed.outcome else {
            unreachable!();
        };
        let output = json!({ "variant": "media", "result": inner });

        prop_assert_eq!(
            oracle.verify(&media, &output),
            OracleVerdict::Pass,
            "Media outcome roundtrip lost data for wire: {}",
            wire
        );
    }
}
