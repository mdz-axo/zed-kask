//! Schema-compliance tests for hkask-mcp-scenarios tool request structs.
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

use hkask_mcp_scenarios::{
    AssessRequest, BrainstormRequest, BuildEventsRequest, CalibrateRequest, CalibrationRequest,
    CompaniesBridgeRequest, CrossValidateRequest, FrameDocumentRequest, FrameRequest,
    FullPipelineRequest, QuantifyRequest, ResearchRequest, ScoreRequest, SensitivityRequest,
    StatusRequest, SynthesizeRequest, TriageRequest, UpdateRequest,
};
use hkask_mcp_server::find_boolean_schema_positions;
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

schema_clean_test!(status_request_schema, StatusRequest);
schema_clean_test!(full_pipeline_request_schema, FullPipelineRequest);
schema_clean_test!(companies_bridge_request_schema, CompaniesBridgeRequest);
schema_clean_test!(cross_validate_request_schema, CrossValidateRequest);
schema_clean_test!(frame_request_schema, FrameRequest);
schema_clean_test!(frame_document_request_schema, FrameDocumentRequest);
schema_clean_test!(brainstorm_request_schema, BrainstormRequest);
schema_clean_test!(build_events_request_schema, BuildEventsRequest);
schema_clean_test!(research_request_schema, ResearchRequest);
schema_clean_test!(quantify_request_schema, QuantifyRequest);
schema_clean_test!(update_request_schema, UpdateRequest);
schema_clean_test!(score_request_schema, ScoreRequest);
schema_clean_test!(calibrate_request_schema, CalibrateRequest);
schema_clean_test!(sensitivity_request_schema, SensitivityRequest);
schema_clean_test!(synthesize_request_schema, SynthesizeRequest);
schema_clean_test!(calibration_request_schema, CalibrationRequest);
schema_clean_test!(triage_request_schema, TriageRequest);
schema_clean_test!(assess_request_schema, AssessRequest);
