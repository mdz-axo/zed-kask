//! Schema-compliance tests for hkask-mcp-research tool request structs.
//!
//! Layer-1 deterministic schema scan enforcing the `.rules` trap "kask MCP
//! tool inputs that accept arbitrary JSON use `AnyJsonValue`": `schemars`
//! renders `serde_json::Value` as the bare boolean `true` in schema-valued
//! positions, which strict-schema-decoding providers (Ollama, Gemini) reject
//! (`400 cannot unmarshal bool into api.ToolProperty`). One bare boolean in
//! any enabled tool's schema fails the whole chat-completion request.
//!
//! Layer 1 only — the `schema_clean_test!` macro asserts no request struct's
//! JSON schema has a bare-boolean schema-valued position. The 3 inline checks
//! in `src/research/types/mod.rs` (ExtractRequest, CitationGetRequest,
//! CitationListRequest) are retained; this file pins all 17 request types
//! (11 in `research::rss_types`, 6 in `research::types`) so the 14
//! previously-uncovered are caught and the full surface stays guarded in one
//! place. Note: `SearchRequest` exists in both modules — they are distinct
//! tool inputs and both are pinned.

use hkask_mcp_research::research::rss_types::{
    DiscoverRequest as RssDiscoverRequest, EditTagRequest as RssEditTagRequest,
    FetchRequest as RssFetchRequest, GetEntriesRequest as RssGetEntriesRequest,
    ImportOpmlRequest as RssImportOpmlRequest,
    ListSubscriptionsRequest as RssListSubscriptionsRequest, MarkReadRequest as RssMarkReadRequest,
    SearchRequest as RssSearchRequest, SubscribeRequest as RssSubscribeRequest,
    UnreadCountRequest as RssUnreadCountRequest, UnsubscribeRequest as RssUnsubscribeRequest,
};
use hkask_mcp_research::research::types::{
    BrowseRequest, CitationGetRequest, CitationListRequest, ExtractRequest, FindSimilarRequest,
    SearchRequest,
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

// ── research::rss_types (11) ──
schema_clean_test!(rss_subscribe_request_schema, RssSubscribeRequest);
schema_clean_test!(rss_unsubscribe_request_schema, RssUnsubscribeRequest);
schema_clean_test!(
    rss_list_subscriptions_request_schema,
    RssListSubscriptionsRequest
);
schema_clean_test!(rss_fetch_request_schema, RssFetchRequest);
schema_clean_test!(rss_get_entries_request_schema, RssGetEntriesRequest);
schema_clean_test!(rss_mark_read_request_schema, RssMarkReadRequest);
schema_clean_test!(rss_unread_count_request_schema, RssUnreadCountRequest);
schema_clean_test!(rss_search_request_schema, RssSearchRequest);
schema_clean_test!(rss_import_opml_request_schema, RssImportOpmlRequest);
schema_clean_test!(rss_discover_request_schema, RssDiscoverRequest);
schema_clean_test!(rss_edit_tag_request_schema, RssEditTagRequest);

// ── research::types (6) ──
schema_clean_test!(types_search_request_schema, SearchRequest);
schema_clean_test!(types_find_similar_request_schema, FindSimilarRequest);
schema_clean_test!(types_extract_request_schema, ExtractRequest);
schema_clean_test!(types_citation_get_request_schema, CitationGetRequest);
schema_clean_test!(types_citation_list_request_schema, CitationListRequest);
schema_clean_test!(types_browse_request_schema, BrowseRequest);
