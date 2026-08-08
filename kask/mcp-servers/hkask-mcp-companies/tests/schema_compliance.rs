//! Schema-compliance tests for hkask-mcp-companies tool request structs.
//!
//! Layer-1 deterministic schema scan enforcing the `.rules` trap "kask MCP
//! tool inputs that accept arbitrary JSON use `AnyJsonValue`": `schemars`
//! renders `serde_json::Value` as the bare boolean `true` in schema-valued
//! positions, which strict-schema-decoding providers (Ollama, Gemini) reject
//! (`400 cannot unmarshal bool into api.ToolProperty`). One bare boolean in
//! any enabled tool's schema fails the whole chat-completion request.
//!
//! Layer 1 only — the `schema_clean_test!` macro asserts no request struct's
//! JSON schema has a bare-boolean schema-valued position. The 3 inline tests
//! in `src/types.rs` (ScreenerRequest, EquityDurationRequest,
//! CompanyTranscriptRequest) are retained; this file covers the remaining 32
//! so all 35 request types are pinned.

use hkask_mcp_companies::types::{
    AttributionRequest, CalibrateForecastRequest, CharacteristicsRequest, CompanyTranscriptRequest,
    ComparableAnalysisRequest, DcfValuationRequest, EpValuationRequest, EquityDurationRequest,
    ExpectationsGapRequest, FileAttachRequest, FileDeleteRequest, FileListRequest,
    ForecastGetRequest, ForecastListRequest, ForecastRecordRequest, HistoricalRequest,
    LedgerExportRequest, LedgerImportRequest, MonteCarloDcfRequest, NoteAddRequest,
    NoteDeleteRequest, NoteListRequest, PortfolioCompareRequest, PortfolioNameRequest,
    PortfolioReturnsRequest, ResearchSearchRequest, ResultFeedbackRequest, ReverseDcfRequest,
    ScenarioAnalysisRequest, ScreenerRequest, SearchRequest, SensitivityAnalysisRequest,
    SymbolLimitRequest, SymbolRequest, TransactionNoteRequest,
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

schema_clean_test!(symbol_request_schema, SymbolRequest);
schema_clean_test!(symbol_limit_request_schema, SymbolLimitRequest);
schema_clean_test!(historical_request_schema, HistoricalRequest);
schema_clean_test!(search_request_schema, SearchRequest);
schema_clean_test!(portfolio_name_request_schema, PortfolioNameRequest);
schema_clean_test!(transaction_note_request_schema, TransactionNoteRequest);
schema_clean_test!(ledger_import_request_schema, LedgerImportRequest);
schema_clean_test!(ledger_export_request_schema, LedgerExportRequest);
schema_clean_test!(portfolio_compare_request_schema, PortfolioCompareRequest);
schema_clean_test!(attribution_request_schema, AttributionRequest);
schema_clean_test!(characteristics_request_schema, CharacteristicsRequest);
schema_clean_test!(expectations_gap_request_schema, ExpectationsGapRequest);
schema_clean_test!(portfolio_returns_request_schema, PortfolioReturnsRequest);
schema_clean_test!(note_add_request_schema, NoteAddRequest);
schema_clean_test!(note_list_request_schema, NoteListRequest);
schema_clean_test!(note_delete_request_schema, NoteDeleteRequest);
schema_clean_test!(file_attach_request_schema, FileAttachRequest);
schema_clean_test!(file_list_request_schema, FileListRequest);
schema_clean_test!(file_delete_request_schema, FileDeleteRequest);
schema_clean_test!(result_feedback_request_schema, ResultFeedbackRequest);
schema_clean_test!(dcf_valuation_request_schema, DcfValuationRequest);
schema_clean_test!(equity_duration_request_schema, EquityDurationRequest);
schema_clean_test!(reverse_dcf_request_schema, ReverseDcfRequest);
schema_clean_test!(scenario_analysis_request_schema, ScenarioAnalysisRequest);
schema_clean_test!(calibrate_forecast_request_schema, CalibrateForecastRequest);
schema_clean_test!(forecast_get_request_schema, ForecastGetRequest);
schema_clean_test!(forecast_list_request_schema, ForecastListRequest);
schema_clean_test!(forecast_record_request_schema, ForecastRecordRequest);
schema_clean_test!(
    sensitivity_analysis_request_schema,
    SensitivityAnalysisRequest
);
schema_clean_test!(monte_carlo_dcf_request_schema, MonteCarloDcfRequest);
schema_clean_test!(
    comparable_analysis_request_schema,
    ComparableAnalysisRequest
);
schema_clean_test!(research_search_request_schema, ResearchSearchRequest);
schema_clean_test!(screener_request_schema, ScreenerRequest);
schema_clean_test!(company_transcript_request_schema, CompanyTranscriptRequest);
schema_clean_test!(ep_valuation_request_schema, EpValuationRequest);
