//! Schema-compliance tests for hkask-mcp-curator tool request structs.
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

use hkask_mcp_curator::types::{
    AlgedonicLogRequest, CuratorConsultRequest, EscalationDismissRequest, EscalationResolveRequest,
    MemoryRecallRequest, PingRequest, RegQueryRequest, ReportSkillUseIssueRequest,
    SemanticSearchRequest,
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

schema_clean_test!(ping_request_schema, PingRequest);
schema_clean_test!(escalation_resolve_request_schema, EscalationResolveRequest);
schema_clean_test!(escalation_dismiss_request_schema, EscalationDismissRequest);
schema_clean_test!(semantic_search_request_schema, SemanticSearchRequest);
schema_clean_test!(memory_recall_request_schema, MemoryRecallRequest);
schema_clean_test!(algedonic_log_request_schema, AlgedonicLogRequest);
schema_clean_test!(reg_query_request_schema, RegQueryRequest);
schema_clean_test!(curator_consult_request_schema, CuratorConsultRequest);
schema_clean_test!(
    report_skill_use_issue_request_schema,
    ReportSkillUseIssueRequest
);
