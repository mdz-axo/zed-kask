//! Schema-compliance tests for hkask-mcp-condenser tool request structs.
//!
//! Replaces the deleted stub-inference QA contract tests (`qa_contract.rs`,
//! 339 lines, which built `MockInference`/`FailingInference` to drive the live
//! server). The schema-compliance invariant those tests were protecting — the
//! `.rules` trap "kask MCP tool inputs that accept arbitrary JSON use
//! `AnyJsonValue`" and "schemars renders `serde_json::Value` as the bare
//! boolean `true`" — is a pure property of each tool's JSON schema and needs no
//! `InferencePort` and no running server.
//!
//! Three layers, all InferencePort-free:
//!
//! 1. **Deterministic schema scan** — for every `#[derive(JsonSchema)]` request
//!    struct, `find_boolean_schema_positions(schema).is_empty()`. A bare
//!    boolean in a schema-valued position is rejected by strict-schema-decoding
//!    providers (Ollama: `400 cannot unmarshal bool into api.ToolProperty`;
//!    Gemini's protobuf `Schema` likewise). `condenser_thread_summary`'s
//!    `messages` field is `Vec<AnyJsonValue>` precisely to avoid this — its
//!    schema must be the empty object `{}`, not `true`.
//!
//! 2. **Deserialization totality property** — arbitrary structured JSON (via
//!    `arb_json_value`) deserializes into every request struct without panicking
//!    (P4 clear boundaries). Verified with `oracle_invariant`.
//!
//! 3. **`AnyJsonValue` acceptance property** — arbitrary JSON placed in the
//!    `messages` field of `ThreadSummaryRequest` deserializes successfully
//!    (`Ok`), pinning that the field truly accepts any JSON value — the runtime
//!    property the schema invariant protects.

use hkask_condenser::types::{PersistRequest, ThreadSummaryRequest};
use hkask_mcp_condenser::SaliencyRequest;
use hkask_mcp_server::find_boolean_schema_positions;
use hkask_test_harness::{OracleVerdict, arb_json_value, oracle_invariant};
use proptest::prelude::*;
use schemars::schema_for;
use serde_json::Value as JsonValue;
use std::panic::catch_unwind;

// ── Layer 1: deterministic schema scan ─────────────────────────────────────

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

schema_clean_test!(saliency_request_schema, SaliencyRequest);
schema_clean_test!(persist_request_schema, PersistRequest);
schema_clean_test!(thread_summary_request_schema, ThreadSummaryRequest);

// `ThreadSummaryRequest.messages` is `Vec<AnyJsonValue>`, so its schema position
// must be an object (`{}`, array of `{}`) — never the bare boolean `true` that
// `serde_json::Value` would produce. This pin is the heart of the `.rules`
// invariant and is checked by the scanner above; this test additionally
// asserts the `messages` property entry is an object, so a regression to
// `serde_json::Value` is caught even before the scanner runs.
#[test]
fn thread_summary_messages_property_is_object_not_boolean() {
    let schema =
        serde_json::to_value(&schema_for!(ThreadSummaryRequest)).expect("schema serializes");
    let properties = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("schema has a properties object");
    let messages = properties
        .get("messages")
        .expect("messages property exists");
    assert!(
        messages.is_object(),
        "messages schema must be a JSON object (array of AnyJsonValue), got: {messages}"
    );
    assert!(
        !messages.is_boolean(),
        "messages schema must not be a bare boolean (serde_json::Value regression)"
    );
}

// ── Layer 2: deserialization totality property ─────────────────────────────

macro_rules! assert_deser_total {
    ($ty:ty, $value:expr, $panicked:expr) => {{
        let v = $value.clone();
        if catch_unwind(move || serde_json::from_value::<$ty>(v).is_ok()).is_err() {
            $panicked = true;
        }
    }};
}

proptest! {
    /// Every condenser request struct must deserialize arbitrary structured
    /// JSON without panicking. The oracle verifies the deserialization outcome
    /// is total (`panicked == false`) regardless of input shape.
    #[test]
    fn request_structs_deserialize_arbitrary_json_without_panicking(value in arb_json_value()) {
        let oracle = oracle_invariant(|_input: &JsonValue, output: &JsonValue| {
            match output.get("panicked").and_then(|v| v.as_bool()) {
                Some(false) => Ok(()),
                _ => Err("a condenser request struct panicked during deserialization".into()),
            }
        });

        let mut panicked = false;
        assert_deser_total!(SaliencyRequest, value, panicked);
        assert_deser_total!(PersistRequest, value, panicked);
        assert_deser_total!(ThreadSummaryRequest, value, panicked);

        let output = serde_json::json!({ "panicked": panicked });
        match oracle.verify(&value, &output) {
            OracleVerdict::Pass => {}
            OracleVerdict::Fail(msg) => prop_assert!(false, "{msg}"),
            OracleVerdict::Inconclusive => {}
        }
    }
}

// ── Layer 3: AnyJsonValue acceptance property ───────────────────────────────

proptest! {
    /// The `messages` field of `ThreadSummaryRequest` is `Vec<AnyJsonValue>`.
    /// Any JSON array element must deserialize successfully — the runtime
    /// property that the `AnyJsonValue` type (and its empty-object schema)
    /// exists to guarantee. A regression to `serde_json::Value` would keep the
    /// wire format permissive but break strict-provider tool-schema decoding;
    /// this proptest pins the deserialize-accepts-anything half of the
    /// contract. The schema scan in Layer 1 pins the schema half.
    #[test]
    fn thread_summary_messages_accepts_arbitrary_json_elements(message in arb_json_value()) {
        let oracle = oracle_invariant(|_input: &JsonValue, output: &JsonValue| {
            match output.get("ok").and_then(|v| v.as_bool()) {
                Some(true) => Ok(()),
                _ => Err("ThreadSummaryRequest rejected arbitrary JSON for the AnyJsonValue messages field".into()),
            }
        });

        let input = serde_json::json!({
            "messages": [message.clone()],
            "current_query": "summarize",
        });
        let ok = serde_json::from_value::<ThreadSummaryRequest>(input.clone()).is_ok();
        let output = serde_json::json!({ "ok": ok });
        match oracle.verify(&input, &output) {
            OracleVerdict::Pass => {}
            OracleVerdict::Fail(msg) => prop_assert!(false, "{msg}"),
            OracleVerdict::Inconclusive => {}
        }
    }
}
