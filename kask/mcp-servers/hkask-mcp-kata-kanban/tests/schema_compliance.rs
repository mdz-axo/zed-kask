//! Schema-compliance tests for hkask-mcp-kata-kanban tool request structs.
//!
//! Replaces the deleted stub-inference service-integration tests
//! (`service_integration.rs`, 359 lines, which built `MockInference` to drive
//! the live server). The schema-compliance invariant those tests were
//! protecting — the `.rules` trap "kask MCP tool inputs that accept arbitrary
//! JSON use `AnyJsonValue`" and "schemars renders `serde_json::Value` as the
//! bare boolean `true`" — is a pure property of each tool's JSON schema and
//! needs no `InferencePort` and no running server.
//!
//! Three layers, all InferencePort-free:
//!
//! 1. **Deterministic schema scan** — for every `#[derive(JsonSchema)]` request
//!    struct, `find_boolean_schema_positions(schema).is_empty()`. A bare
//!    boolean in a schema-valued position is rejected by strict-schema-decoding
//!    providers (Ollama: `400 cannot unmarshal bool into api.ToolProperty`;
//!    Gemini's protobuf `Schema` likewise). `contract_propose_expect`'s
//!    `proposals` field is `AnyJsonValue` precisely to avoid this — its schema
//!    must be the empty object `{}`, not `true`.
//!
//! 2. **Deserialization totality property** — arbitrary structured JSON (via
//!    `arb_json_value`) deserializes into every request struct without panicking
//!    (P4 clear boundaries). Verified with `oracle_invariant`.
//!
//! 3. **`AnyJsonValue` acceptance property** — arbitrary JSON placed in the
//!    `proposals` field of `ContractProposeExpect` deserializes successfully
//!    (`Ok`), pinning that the field truly accepts any JSON value — the runtime
//!    property the schema invariant protects.

use hkask_mcp_kata_kanban::types::{
    BoardCreateRequest, BoardListRequest, ColumnDefInput, ContractProposeExpect,
    TaskAddDeliverableRequest, TaskAddRjoulesRequest, TaskAssignRequest, TaskCommentRequest,
    TaskCommentsSinceRequest, TaskCreateRequest, TaskKataCoachingRequest,
    TaskKataImprovementRequest, TaskKataPracticeRequest, TaskListRequest, TaskMoveRequest,
    TaskReopenRequest, TaskSpawnRequest, TaskVerifyRequest,
};
use hkask_mcp_server::find_boolean_schema_positions;
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

schema_clean_test!(board_create_request_schema, BoardCreateRequest);
schema_clean_test!(column_def_input_schema, ColumnDefInput);
schema_clean_test!(board_list_request_schema, BoardListRequest);
schema_clean_test!(task_create_request_schema, TaskCreateRequest);
schema_clean_test!(task_list_request_schema, TaskListRequest);
schema_clean_test!(task_move_request_schema, TaskMoveRequest);
schema_clean_test!(task_assign_request_schema, TaskAssignRequest);
schema_clean_test!(task_verify_request_schema, TaskVerifyRequest);
schema_clean_test!(task_add_rjoules_request_schema, TaskAddRjoulesRequest);
schema_clean_test!(task_comment_request_schema, TaskCommentRequest);
schema_clean_test!(task_comments_since_request_schema, TaskCommentsSinceRequest);
schema_clean_test!(
    task_add_deliverable_request_schema,
    TaskAddDeliverableRequest
);
schema_clean_test!(task_reopen_request_schema, TaskReopenRequest);
schema_clean_test!(contract_propose_expect_schema, ContractProposeExpect);
schema_clean_test!(task_kata_coaching_request_schema, TaskKataCoachingRequest);
schema_clean_test!(
    task_kata_improvement_request_schema,
    TaskKataImprovementRequest
);
schema_clean_test!(task_kata_practice_request_schema, TaskKataPracticeRequest);
schema_clean_test!(task_spawn_request_schema, TaskSpawnRequest);

// `ContractProposeExpect.proposals` is `AnyJsonValue`, so its schema position
// must be the empty object `{}` — never the bare boolean `true` that
// `serde_json::Value` would produce. The scanner above catches this; this test
// additionally asserts the `proposals` property entry is an object, so a
// regression to `serde_json::Value` is caught with a precise message.
#[test]
fn contract_propose_expect_proposals_property_is_object_not_boolean() {
    let schema =
        serde_json::to_value(&schema_for!(ContractProposeExpect)).expect("schema serializes");
    let properties = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("schema has a properties object");
    let proposals = properties
        .get("proposals")
        .expect("proposals property exists");
    assert!(
        proposals.is_object(),
        "proposals schema must be a JSON object (AnyJsonValue), got: {proposals}"
    );
    assert!(
        !proposals.is_boolean(),
        "proposals schema must not be a bare boolean (serde_json::Value regression)"
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
    /// Every kata-kanban request struct must deserialize arbitrary structured
    /// JSON without panicking. The oracle verifies the deserialization outcome
    /// is total (`panicked == false`) regardless of input shape.
    #[test]
    fn request_structs_deserialize_arbitrary_json_without_panicking(value in arb_json_value()) {
        let oracle = oracle_invariant(|_input: &JsonValue, output: &JsonValue| {
            match output.get("panicked").and_then(|v| v.as_bool()) {
                Some(false) => Ok(()),
                _ => Err("a kata-kanban request struct panicked during deserialization".into()),
            }
        });

        let mut panicked = false;
        assert_deser_total!(BoardCreateRequest, value, panicked);
        assert_deser_total!(ColumnDefInput, value, panicked);
        assert_deser_total!(BoardListRequest, value, panicked);
        assert_deser_total!(TaskCreateRequest, value, panicked);
        assert_deser_total!(TaskListRequest, value, panicked);
        assert_deser_total!(TaskMoveRequest, value, panicked);
        assert_deser_total!(TaskAssignRequest, value, panicked);
        assert_deser_total!(TaskVerifyRequest, value, panicked);
        assert_deser_total!(TaskAddRjoulesRequest, value, panicked);
        assert_deser_total!(TaskCommentRequest, value, panicked);
        assert_deser_total!(TaskCommentsSinceRequest, value, panicked);
        assert_deser_total!(TaskAddDeliverableRequest, value, panicked);
        assert_deser_total!(TaskReopenRequest, value, panicked);
        assert_deser_total!(ContractProposeExpect, value, panicked);
        assert_deser_total!(TaskKataCoachingRequest, value, panicked);
        assert_deser_total!(TaskKataImprovementRequest, value, panicked);
        assert_deser_total!(TaskKataPracticeRequest, value, panicked);
        assert_deser_total!(TaskSpawnRequest, value, panicked);

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
    /// The `proposals` field of `ContractProposeExpect` is `AnyJsonValue`. Any
    /// JSON value must deserialize successfully — the runtime property that
    /// `AnyJsonValue` (and its empty-object schema) exists to guarantee. A
    /// regression to `serde_json::Value` would keep the wire format permissive
    /// but break strict-provider tool-schema decoding; this proptest pins the
    /// deserialize-accepts-anything half of the contract. The schema scan in
    /// Layer 1 pins the schema half.
    #[test]
    fn contract_propose_expect_proposals_accepts_arbitrary_json(proposals in arb_json_value()) {
        let oracle = oracle_invariant(|_input: &JsonValue, output: &JsonValue| {
            match output.get("ok").and_then(|v| v.as_bool()) {
                Some(true) => Ok(()),
                _ => Err("ContractProposeExpect rejected arbitrary JSON for the AnyJsonValue proposals field".into()),
            }
        });

        let input = serde_json::json!({
            "board_id": "b1",
            "proposals": proposals,
        });
        let ok = serde_json::from_value::<ContractProposeExpect>(input.clone()).is_ok();
        let output = serde_json::json!({ "ok": ok });
        match oracle.verify(&input, &output) {
            OracleVerdict::Pass => {}
            OracleVerdict::Fail(msg) => prop_assert!(false, "{msg}"),
            OracleVerdict::Inconclusive => {}
        }
    }
}
