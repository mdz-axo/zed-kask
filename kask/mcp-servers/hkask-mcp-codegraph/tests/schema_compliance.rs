//! Schema-compliance tests for hkask-mcp-codegraph tool request structs.
//!
//! Replaces the deleted stub-`InferencePort` QA contract tests (`qa_contract.rs`,
//! 661 lines). Those tests built a `StubInferencePort` to drive the live server
//! and assert tool-output contracts. The schema-compliance invariant they were
//! protecting — the `.rules` trap "kask MCP tool inputs that accept arbitrary
//! JSON use `AnyJsonValue`" and "schemars renders `serde_json::Value` as the
//! bare boolean `true`" — is a pure property of each tool's JSON schema and
//! needs no `InferencePort` and no running server.
//!
//! Two layers, both InferencePort-free:
//!
//! 1. **Deterministic schema scan** — for every `#[derive(JsonSchema)]` request
//!    struct, `find_boolean_schema_positions(schema).is_empty()`. A bare boolean
//!    in a schema-valued position (`properties.*`, `items`, `anyOf.*`, …) is
//!    valid JSON Schema but rejected by strict-schema-decoding providers
//!    (Ollama: `400 cannot unmarshal bool into api.ToolProperty`; Gemini's
//!    protobuf `Schema` likewise). One such boolean in any enabled tool's schema
//!    fails the whole chat-completion request.
//!
//! 2. **Property test** — arbitrary structured JSON (via `arb_json_value`)
//!    deserializes into every request struct without panicking (P4 clear
//!    boundaries: input surfaces reject invalid input gracefully, never panic).
//!    Verified with the `oracle_invariant` oracle from `hkask-test-harness`.

use hkask_mcp_codegraph::{
    AnalysisRequest, ContextRequest, EmbedIndexRequest, ImpactRequest, QueryRequest, StatsRequest,
    StructureRequest, TraverseRequest,
};
use hkask_mcp_server::find_boolean_schema_positions;
use hkask_test_harness::{OracleVerdict, arb_json_value, oracle_invariant};
use proptest::prelude::*;
use schemars::schema_for;
use serde_json::Value as JsonValue;
use std::panic::catch_unwind;

// ── Layer 1: deterministic schema scan ─────────────────────────────────────

/// Generates a `#[test]` that builds the request struct's JSON schema and
/// asserts it contains no bare-boolean schema-valued positions — the
/// strict-provider (Ollama/Gemini) rejection class. One test per struct keeps
/// CI output granular: a violation names the exact struct.
macro_rules! schema_clean_test {
    ($test_name:ident, $ty:ty) => {
        #[test]
        fn $test_name() {
            let schema = serde_json::to_value(&schema_for!($ty)).expect("schema serializes");
            let violations = find_boolean_schema_positions(&schema);
            assert!(
                violations.is_empty(),
                "{} schema has bare-boolean schema positions (Ollama/Gemini would reject): {violations:?}",
                stringify!($ty),
            );
        }
    };
}

schema_clean_test!(query_request_schema, QueryRequest);
schema_clean_test!(embed_index_request_schema, EmbedIndexRequest);
schema_clean_test!(traverse_request_schema, TraverseRequest);
schema_clean_test!(impact_request_schema, ImpactRequest);
schema_clean_test!(context_request_schema, ContextRequest);
schema_clean_test!(analysis_request_schema, AnalysisRequest);
schema_clean_test!(structure_request_schema, StructureRequest);
schema_clean_test!(stats_request_schema, StatsRequest);

// ── Layer 2: deserialization totality property ─────────────────────────────

/// Tries to deserialize `value` into `$ty` inside `catch_unwind`; sets
/// `$panicked` to `true` if deserialization panics. P4 clear boundaries:
/// a tool input surface must reject arbitrary JSON with `Err`, never a panic.
macro_rules! assert_deser_total {
    ($ty:ty, $value:expr, $panicked:expr) => {{
        let v = $value.clone();
        if catch_unwind(move || serde_json::from_value::<$ty>(v).is_ok()).is_err() {
            $panicked = true;
        }
    }};
}

proptest! {
    /// Every codegraph request struct must deserialize arbitrary structured
    /// JSON without panicking. The oracle verifies the deserialization outcome
    /// is total (`panicked == false`) regardless of input shape — the input
    /// surface degrades to `Err`, never a panic.
    #[test]
    fn request_structs_deserialize_arbitrary_json_without_panicking(value in arb_json_value()) {
        let oracle = oracle_invariant(|_input: &JsonValue, output: &JsonValue| {
            match output.get("panicked").and_then(|v| v.as_bool()) {
                Some(false) => Ok(()),
                _ => Err("a codegraph request struct panicked during deserialization".into()),
            }
        });

        let mut panicked = false;
        assert_deser_total!(QueryRequest, value, panicked);
        assert_deser_total!(EmbedIndexRequest, value, panicked);
        assert_deser_total!(TraverseRequest, value, panicked);
        assert_deser_total!(ImpactRequest, value, panicked);
        assert_deser_total!(ContextRequest, value, panicked);
        assert_deser_total!(AnalysisRequest, value, panicked);
        assert_deser_total!(StructureRequest, value, panicked);
        assert_deser_total!(StatsRequest, value, panicked);

        let output = serde_json::json!({ "panicked": panicked });
        match oracle.verify(&value, &output) {
            OracleVerdict::Pass => {}
            OracleVerdict::Fail(msg) => prop_assert!(false, "{msg}"),
            OracleVerdict::Inconclusive => {}
        }
    }
}
