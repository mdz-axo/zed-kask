//! Schema-compliance tests for hkask-mcp-swarm tool request structs.
//!
//! Layer-1 deterministic schema scan enforcing the `.rules` trap "kask MCP tool
//! inputs that accept arbitrary JSON use `AnyJsonValue`": schemars renders
//! `serde_json::Value` as the bare boolean `true` in schema-valued positions,
//! which strict-schema-decoding providers (Ollama, Gemini) reject
//! (`400 cannot unmarshal bool into api.ToolProperty`). One bare boolean in
//! any enabled tool schema fails the whole chat-completion request.
//!
//! Layer 1 only (the `schema_clean_test!` macro asserts no request struct
//! schema has a bare-boolean schema-valued position). Layer 2 (a `proptest!`
//! deserialization-totality property) is intentionally omitted: it needs
//! `proptest` + `hkask-test-harness` dev-deps to guard a different invariant
//! (P4 deserialization totality) that is out of scope here. The request types
//! live in the public `request_types` module (widened from `pub(crate)` so an
//! integration test can name them).

use hkask_mcp_server::find_boolean_schema_positions;
use hkask_mcp_swarm::request_types::*;
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

schema_clean_test!(a2a_card_request_schema, A2aCardRequest);
schema_clean_test!(a2a_send_request_schema, A2aSendRequest);
schema_clean_test!(
    add_agent_to_local_swarm_request_schema,
    AddAgentToLocalSwarmRequest
);
schema_clean_test!(authorize_session_request_schema, AuthorizeSessionRequest);
schema_clean_test!(balance_local_request_schema, BalanceLocalRequest);
schema_clean_test!(clone_to_local_request_schema, CloneToLocalRequest);
schema_clean_test!(create_agent_request_schema, CreateAgentRequest);
schema_clean_test!(create_app_request_schema, CreateAppRequest);
schema_clean_test!(create_local_agent_request_schema, CreateLocalAgentRequest);
schema_clean_test!(create_local_swarm_request_schema, CreateLocalSwarmRequest);
schema_clean_test!(create_swarm_request_schema, CreateSwarmRequest);
schema_clean_test!(delegate_and_wait_request_schema, DelegateAndWaitRequest);
schema_clean_test!(delegate_local_request_schema, DelegateLocalRequest);
schema_clean_test!(delegate_request_schema, DelegateRequest);
schema_clean_test!(delete_agent_request_schema, DeleteAgentRequest);
schema_clean_test!(delete_local_swarm_request_schema, DeleteLocalSwarmRequest);
schema_clean_test!(delete_swarm_request_schema, DeleteSwarmRequest);
schema_clean_test!(execute_agent_request_schema, ExecuteAgentRequest);
schema_clean_test!(fanout_abw_entry_schema, FanoutAbwEntry);
schema_clean_test!(fanout_entry_schema, FanoutEntry);
schema_clean_test!(fanout_local_request_schema, FanoutLocalRequest);
schema_clean_test!(fanout_request_schema, FanoutRequest);
schema_clean_test!(fire_request_schema, FireRequest);
schema_clean_test!(fork_agent_request_schema, ForkAgentRequest);
schema_clean_test!(fund_local_request_schema, FundLocalRequest);
schema_clean_test!(
    generate_ontology_local_request_schema,
    GenerateOntologyLocalRequest
);
schema_clean_test!(generate_ontology_request_schema, GenerateOntologyRequest);
schema_clean_test!(
    generate_prompt_local_request_schema,
    GeneratePromptLocalRequest
);
schema_clean_test!(generate_prompt_request_schema, GeneratePromptRequest);
schema_clean_test!(get_agent_request_schema, GetAgentRequest);
schema_clean_test!(get_local_swarm_request_schema, GetLocalSwarmRequest);
schema_clean_test!(get_swarm_request_schema, GetSwarmRequest);
schema_clean_test!(hire_cost_request_schema, HireCostRequest);
schema_clean_test!(hire_request_schema, HireRequest);
schema_clean_test!(list_agents_request_schema, ListAgentsRequest);
schema_clean_test!(list_apps_request_schema, ListAppsRequest);
schema_clean_test!(list_local_agents_request_schema, ListLocalAgentsRequest);
schema_clean_test!(list_local_swarms_request_schema, ListLocalSwarmsRequest);
schema_clean_test!(local_history_request_schema, LocalHistoryRequest);
schema_clean_test!(ontology_templates_request_schema, OntologyTemplatesRequest);
schema_clean_test!(pipeline_local_request_schema, PipelineLocalRequest);
schema_clean_test!(pipeline_step_schema, PipelineStep);
schema_clean_test!(publish_agent_request_schema, PublishAgentRequest);
schema_clean_test!(publish_checks_request_schema, PublishChecksRequest);
schema_clean_test!(push_to_cloud_request_schema, PushToCloudRequest);
schema_clean_test!(
    reconfigure_local_agent_request_schema,
    ReconfigureLocalAgentRequest
);
schema_clean_test!(
    remove_agent_from_local_swarm_request_schema,
    RemoveAgentFromLocalSwarmRequest
);
schema_clean_test!(remove_local_request_schema, RemoveLocalRequest);
schema_clean_test!(request_consent_request_schema, RequestConsentRequest);
schema_clean_test!(
    search_knowledge_local_request_schema,
    SearchKnowledgeLocalRequest
);
schema_clean_test!(search_knowledge_request_schema, SearchKnowledgeRequest);
schema_clean_test!(swarm_run_request_schema, SwarmRunRequest);
schema_clean_test!(xaman_request_schema, XamanRequest);
