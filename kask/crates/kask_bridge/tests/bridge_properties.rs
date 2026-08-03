//! Property tests for the inference IPC message types that `InferenceIpcServer`
//! serializes over the Unix socket.
//!
//! The deleted stub-based tests (`StubInferencePort`, `StubToolPort`,
//! `StubSkillExec`, `CountingMockPort`) exercised the async dispatch path of
//! `InferenceIpcServer::dispatch` and the prompt-length recall gate. Both need
//! a real port implementation to drive — constructing one would be a forbidden
//! stub. The genuinely pure, stub-free surface is the **serialization layer**:
//! `InferenceRequest` / `InferenceResponse` / `InferenceOutcome` round-trip
//! losslessly through `serde_json`, including the JSON-valued fields
//! (`tool_args`, `media_workflow`, and the `tool_result`/`media` outcome
//! payloads). The server does `serde_json::from_str::<InferenceRequest>(line)`
//! on read and `serde_json::to_string(&InferenceResponse)` on write, so
//! lossless roundtrip of these types is exactly the property the IPC layer
//! depends on.
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

use hkask_test_harness::{OracleVerdict, arb_json_value, oracle_invariant, oracle_reference};
use hkask_types::inference_ipc::{
    InferenceMethod, InferenceOutcome, InferenceParams, InferenceRequest, InferenceResponse,
};
use proptest::prelude::*;
use serde_json::{Value as JsonValue, json};

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
        media_object_description: None,
        media_language: None,
        media_workflow: None,
        tool_server: None,
        tool_name: None,
        tool_args: None,
        tool_allowlist: None,
        skill_name: None,
        skill_task: None,
    }
}

proptest! {
    /// An `InferenceRequest` carrying an arbitrary JSON `tool_args` payload
    /// round-trips losslessly: `to_string` → `from_str` → `to_string` yields
    /// the identical wire bytes. The IPC server reads requests with
    /// `serde_json::from_str`, so any drift here corrupts every tool dispatch.
    ///
    /// Oracle: [`oracle_reference`] — the reference is the identity function;
    /// the roundtripped wire form must equal the original wire form.
    #[test]
    fn inference_request_roundtrip_preserves_tool_args(
        id in any::<u64>(),
        tool_args in arb_json_value(),
    ) {
        let oracle = oracle_reference(|x: &JsonValue| x.clone());

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
        let original = serde_json::to_string(&request).expect("request serializes");
        let reparsed: InferenceRequest =
            serde_json::from_str(&original).expect("request re-parses");
        let roundtripped = serde_json::to_string(&reparsed).expect("reparsed serializes");

        let input = JsonValue::String(original);
        let output = JsonValue::String(roundtripped);
        prop_assert_eq!(
            oracle.verify(&input, &output),
            OracleVerdict::Pass,
            "InferenceRequest tool_args roundtrip is not lossless"
        );
    }

    /// An `InferenceRequest` carrying an arbitrary JSON `media_workflow`
    /// payload round-trips losslessly. (The `media_workflow` field is
    /// `serde_json::Value`, the DAG spec for `execute_workflow`.)
    ///
    /// Oracle: [`oracle_reference`] — identity reference.
    #[test]
    fn inference_request_roundtrip_preserves_media_workflow(
        id in any::<u64>(),
        media_workflow in arb_json_value(),
    ) {
        let oracle = oracle_reference(|x: &JsonValue| x.clone());

        let request = InferenceRequest {
            id,
            method: InferenceMethod::MediaGenerate,
            params: {
                let mut p = bare_params();
                p.media_op = Some("execute_workflow".to_string());
                p.media_workflow = Some(media_workflow);
                p
            },
        };
        let original = serde_json::to_string(&request).expect("request serializes");
        let reparsed: InferenceRequest =
            serde_json::from_str(&original).expect("request re-parses");
        let roundtripped = serde_json::to_string(&reparsed).expect("reparsed serializes");

        let input = JsonValue::String(original);
        let output = JsonValue::String(roundtripped);
        prop_assert_eq!(
            oracle.verify(&input, &output),
            OracleVerdict::Pass,
            "InferenceRequest media_workflow roundtrip is not lossless"
        );
    }

    /// An `InferenceResponse` whose outcome is `ToolResult { result: <json> }`
    /// round-trips losslessly: the deserialized outcome is still `ToolResult`
    /// (the `tool_result` key distinguishes it from the `Result` variant under
    /// `#[serde(untagged)]`) and the inner JSON is byte-identical.
    ///
    /// Oracle: [`oracle_invariant`] — variant key preserved AND inner JSON
    /// preserved.
    #[test]
    fn inference_response_tool_result_roundtrip_preserves_variant_and_json(
        id in any::<u64>(),
        result in arb_json_value(),
    ) {
        let oracle = oracle_invariant(|input: &JsonValue, output: &JsonValue| {
            if output["variant"] != "tool_result" {
                return Err(format!("outcome variant not preserved: expected tool_result, got {}", output["variant"]));
            }
            if output["result"] != *input {
                return Err(format!("inner JSON not preserved: {} != {}", output["result"], input));
            }
            Ok(())
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
    /// round-trips losslessly: the outcome stays `Media` and the inner JSON is
    /// byte-identical.
    ///
    /// Oracle: [`oracle_invariant`] — variant key preserved AND inner JSON
    /// preserved.
    #[test]
    fn inference_response_media_roundtrip_preserves_variant_and_json(
        id in any::<u64>(),
        media in arb_json_value(),
    ) {
        let oracle = oracle_invariant(|input: &JsonValue, output: &JsonValue| {
            if output["variant"] != "media" {
                return Err(format!("outcome variant not preserved: expected media, got {}", output["variant"]));
            }
            if output["result"] != *input {
                return Err(format!("inner JSON not preserved: {} != {}", output["result"], input));
            }
            Ok(())
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
