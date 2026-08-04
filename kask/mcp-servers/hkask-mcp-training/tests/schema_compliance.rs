//! Schema-compliance tests for hkask-mcp-training tool request structs.
//!
//! Layer-1 deterministic schema scan enforcing the `.rules` trap "kask MCP
//! tool inputs that accept arbitrary JSON use `AnyJsonValue`": `schemars`
//! renders `serde_json::Value` as the bare boolean `true` in schema-valued
//! positions, which strict-schema-decoding providers (Ollama, Gemini) reject
//! (`400 cannot unmarshal bool into api.ToolProperty`). One bare boolean in
//! any enabled tool's schema fails the whole chat-completion request.
//!
//! Layer 1 only — the `schema_clean_test!` macro asserts no request struct's
//! JSON schema has a bare-boolean schema-valued position. Layer 2 (a
//! `proptest!` deserialization-totality property) is intentionally omitted: it
//! needs `proptest` + `hkask-test-harness` dev-deps to guard a different
//! invariant (P4 deserialization totality) that is out of scope here.

use hkask_mcp_server::find_boolean_schema_positions;
use hkask_mcp_training::types::{
    AssembleDatasetRequest, IngestQaRequest, TrainCancelRequest, TrainEvaluateRequest,
    TrainIngestDatasetRequest, TrainStatusRequest, TrainSubmitRequest, TrainValidateConfigRequest,
};
use schemars::schema_for;

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

schema_clean_test!(train_cancel_request_schema, TrainCancelRequest);
schema_clean_test!(ingest_qa_request_schema, IngestQaRequest);
schema_clean_test!(assemble_dataset_request_schema, AssembleDatasetRequest);
schema_clean_test!(
    train_ingest_dataset_request_schema,
    TrainIngestDatasetRequest
);
schema_clean_test!(train_evaluate_request_schema, TrainEvaluateRequest);
schema_clean_test!(train_status_request_schema, TrainStatusRequest);
schema_clean_test!(train_submit_request_schema, TrainSubmitRequest);
schema_clean_test!(
    train_validate_config_request_schema,
    TrainValidateConfigRequest
);
