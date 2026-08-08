//! Schema-compliance tests for hkask-mcp-prediction-markets tool request structs.
//!
//! Layer-1 deterministic schema scan enforcing the `.rules` trap "kask MCP
//! tool inputs that accept arbitrary JSON use `AnyJsonValue`": `schemars`
//! renders `serde_json::Value` as the bare boolean `true` in schema-valued
//! positions, which strict-schema-decoding providers (Ollama, Gemini) reject
//! (`400 cannot unmarshal bool into api.ToolProperty`). One bare boolean in
//! any enabled tool's schema fails the whole chat-completion request.
//!
//! Layer 1 only — the `schema_clean_test!` macro asserts no request struct's
//! JSON schema has a bare-boolean schema-valued position. The 14 inline /
//! per-test-file checks (StatusRequest, MarketLookupRequest,
//! MarketOntologyMapRequest, MarketRecordResolutionRequest,
//! MarketCmpIndexRequest, MarketCmpIndexStoreRequest,
//! MarketCmpPortfolioStoreRequest, MarketCmpContextSuggestRequest,
//! MarketVolatilityRequest, MarketCmpRequest, MarketHistoryRequest,
//! MarketCheckResolutionsRequest, MarketCalibrationRequest,
//! MarketMatchRequest) are retained; this file pins all 17 so the 3
//! previously-uncovered (MarketSubscribeRequest, MarketResidualRequest,
//! MarketLadderRequest) are caught and the full surface stays guarded in
//! one place.

use hkask_mcp_prediction_markets::{
    MarketCalibrationRequest, MarketCheckResolutionsRequest, MarketCmpContextSuggestRequest,
    MarketCmpIndexRequest, MarketCmpIndexStoreRequest, MarketCmpPortfolioStoreRequest,
    MarketCmpRequest, MarketHistoryRequest, MarketLadderRequest, MarketLookupRequest,
    MarketMatchRequest, MarketOntologyMapRequest, MarketRecordResolutionRequest,
    MarketResidualRequest, MarketSubscribeRequest, MarketVolatilityRequest, StatusRequest,
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
schema_clean_test!(market_ontology_map_request_schema, MarketOntologyMapRequest);
schema_clean_test!(market_lookup_request_schema, MarketLookupRequest);
schema_clean_test!(
    market_record_resolution_request_schema,
    MarketRecordResolutionRequest
);
schema_clean_test!(market_subscribe_request_schema, MarketSubscribeRequest);
schema_clean_test!(market_residual_request_schema, MarketResidualRequest);
schema_clean_test!(market_cmp_index_request_schema, MarketCmpIndexRequest);
schema_clean_test!(
    market_cmp_index_store_request_schema,
    MarketCmpIndexStoreRequest
);
schema_clean_test!(
    market_cmp_portfolio_store_request_schema,
    MarketCmpPortfolioStoreRequest
);
schema_clean_test!(
    market_cmp_context_suggest_request_schema,
    MarketCmpContextSuggestRequest
);
schema_clean_test!(market_volatility_request_schema, MarketVolatilityRequest);
schema_clean_test!(market_cmp_request_schema, MarketCmpRequest);
schema_clean_test!(market_history_request_schema, MarketHistoryRequest);
schema_clean_test!(
    market_check_resolutions_request_schema,
    MarketCheckResolutionsRequest
);
schema_clean_test!(market_calibration_request_schema, MarketCalibrationRequest);
schema_clean_test!(market_match_request_schema, MarketMatchRequest);
schema_clean_test!(market_ladder_request_schema, MarketLadderRequest);
