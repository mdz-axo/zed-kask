//! Tool-behavior contract tests for hkask-mcp-research.
//!
//! Drives the real `Parameters<T>` seam on `ResearchServer`. Covers:
//! - `web_ping` happy path (in-process, stub pool).
//! - `web_search` invalid-argument paths (empty query, oversized query,
//!   unknown strategy, unknown freshness) — checked before any HTTP call.
//! - `web_search` / `web_find_similar` / `web_extract` / `web_browse`
//!   credential-missing paths — a stub `WebSearchPort` returns
//!   `NoProviderConfigured`, which maps to `permission_denied`. This pins the
//!   `.rules` rule: a missing credential must surface as a structured error,
//!   not a silent fallback or empty result.
//! - `web_extract` / `web_browse` invalid-argument paths (oversized URL,
//!   oversized json_prompt / instruction) — checked before URL validation.
//! - RSS tools without a DB → `permission_denied` (the `require_rss_db!` gate).
//! - RSS tools with an in-memory DB → happy path (empty list, zero unread)
//!   and invalid-argument (malformed continuation token).

#![forbid(unsafe_code)]

use hkask_mcp_research::ResearchServer;
use hkask_mcp_research::research::cache::ResponseCache;
use hkask_mcp_research::research::db::RSS_SCHEMA_DDL;
use hkask_mcp_research::research::providers::{ProviderSearchOutput, WebSearchPort};
use hkask_mcp_research::research::rss_types::{
    GetEntriesRequest, ListSubscriptionsRequest, MarkReadRequest, UnreadCountRequest,
    UnsubscribeRequest,
};
use hkask_mcp_research::research::types::{
    BrowseRequest, BrowseResult, CompoundSearchResult, ExtractOptions, ExtractRequest,
    ExtractedContent, FindSimilarRequest, ProviderHealthEntry, ProviderRecommendation, RateLimiter,
    SearchQuery, SearchRequest, SearchStrategy, WebError,
};
use hkask_types::WebID;
use hkask_types::tool_response::parse_tool_response;
use rmcp::handler::server::wrapper::Parameters;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

// ── Stub WebSearchPort ─────────────────────────────────────────────────────

/// Stub that simulates "no credentials configured" for all HTTP provider
/// calls. The tool handler maps `WebError::NoProviderConfigured` to
/// `McpToolError::permission_denied` — the test asserts that mapping,
/// which pins the broken-feedback-loop rule: a missing credential must
/// surface as a structured error, not an empty result or silent no-op.
struct NoCredentialsPool;

#[async_trait]
impl WebSearchPort for NoCredentialsPool {
    async fn search(
        &self,
        _query: &SearchQuery,
        _strategy: SearchStrategy,
        _provider: Option<&str>,
    ) -> Result<CompoundSearchResult, WebError> {
        Err(WebError::NoProviderConfigured(
            "No search provider configured. Set HKASK_BRAVE_API_KEY or HKASK_TAVILY_API_KEY."
                .to_string(),
        ))
    }

    async fn find_similar(
        &self,
        _url: &str,
        _num_results: u32,
    ) -> Result<ProviderSearchOutput, WebError> {
        Err(WebError::NoProviderConfigured(
            "Exa provider not configured. Set HKASK_EXA_API_KEY.".to_string(),
        ))
    }

    async fn extract(
        &self,
        _url: &str,
        _opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        Err(WebError::NoProviderConfigured(
            "No extract provider configured.".to_string(),
        ))
    }

    async fn browse(
        &self,
        _url: &str,
        _instruction: &str,
        _timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        Err(WebError::NoProviderConfigured(
            "No browse provider configured. Set HKASK_FIRECRAWL_API_KEY.".to_string(),
        ))
    }

    async fn health_check(&self) -> Vec<ProviderHealthEntry> {
        vec![ProviderHealthEntry {
            kind: "stub".to_string(),
            healthy: true,
            error: None,
        }]
    }

    fn provider_fingerprint(&self) -> String {
        "stub-no-credentials".to_string()
    }

    fn provider_kinds(&self) -> Vec<String> {
        Vec::new()
    }

    fn score_providers(&self, _query: &str, _intent: Option<&str>) -> Vec<ProviderRecommendation> {
        Vec::new()
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn make_server_without_db() -> ResearchServer {
    ResearchServer::new(
        WebID::new(),
        Arc::new(NoCredentialsPool),
        Arc::new(ResponseCache::new(10, Duration::from_secs(60))),
        RateLimiter::new(10000, 60),
        None,
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
    )
}

fn make_server_with_rss_db() -> ResearchServer {
    let manager = r2d2_sqlite::SqliteConnectionManager::memory();
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .build(manager)
        .expect("r2d2 pool build");
    {
        let connection = pool.get().expect("r2d2 pool get");
        connection
            .execute_batch(RSS_SCHEMA_DDL)
            .expect("RSS schema init");
    }
    ResearchServer::new(
        WebID::new(),
        Arc::new(NoCredentialsPool),
        Arc::new(ResponseCache::new(10, Duration::from_secs(60))),
        RateLimiter::new(10000, 60),
        Some(pool),
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
    )
}

fn parse(out: &str) -> serde_json::Value {
    parse_tool_response(out).expect("tool output must be valid JSON")
}

fn assert_error_kind(json: &serde_json::Value, expected_kind: &str) {
    let kind = json
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or_else(|| panic!("expected 'kind' field, got: {json}"));
    assert_eq!(
        kind, expected_kind,
        "expected kind '{expected_kind}' but got '{kind}'; full response: {json}"
    );
    assert!(
        json.get("error")
            .is_some_and(|e| e.as_str().is_some_and(|s| !s.is_empty())),
        "expected non-empty 'error' field, got: {json}"
    );
}

/// A literal public IP URL that passes `validate_tool_url_with_dns` without
/// DNS resolution (literal IPs skip the `lookup_host` call). Used for
/// credential-missing tests on URL-accepting tools.
const LITERAL_IP_URL: &str = "http://1.2.3.4/path";

// ── web_ping (happy path) ──────────────────────────────────────────────────

#[tokio::test]
async fn web_ping_returns_ok_with_provider_health() {
    let server = make_server_without_db();
    let out = server.web_ping().await;
    let json = parse(&out);
    assert_eq!(
        json.get("status").and_then(|status| status.as_str()),
        Some("ok"),
        "web_ping should return status ok; got: {json}"
    );
    assert!(
        json.get("providers")
            .is_some_and(|providers| providers.is_array()),
        "web_ping should return a providers array; got: {json}"
    );
}

// ── web_search invalid-argument paths ──────────────────────────────────────

#[tokio::test]
async fn web_search_rejects_empty_query() {
    let server = make_server_without_db();
    let out = server
        .web_search(Parameters(SearchRequest {
            query: String::new(),
            num_results: None,
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            provider: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "invalid_argument");
    assert!(
        json.get("error").is_some_and(|error| error
            .as_str()
            .is_some_and(|message| message.contains("empty"))),
        "error should mention empty query; got: {json}"
    );
}

#[tokio::test]
async fn web_search_rejects_oversized_query() {
    let server = make_server_without_db();
    let out = server
        .web_search(Parameters(SearchRequest {
            query: "x".repeat(500),
            num_results: None,
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            provider: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "invalid_argument");
    assert!(
        json.get("error").is_some_and(|error| {
            error
                .as_str()
                .is_some_and(|message| message.contains("maximum length"))
        }),
        "error should mention maximum length; got: {json}"
    );
}

#[tokio::test]
async fn web_search_rejects_unknown_strategy() {
    let server = make_server_without_db();
    let out = server
        .web_search(Parameters(SearchRequest {
            query: "test".to_string(),
            num_results: None,
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: Some("bogus".to_string()),
            provider: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "invalid_argument");
}

#[tokio::test]
async fn web_search_rejects_unknown_freshness() {
    let server = make_server_without_db();
    let out = server
        .web_search(Parameters(SearchRequest {
            query: "test".to_string(),
            num_results: None,
            include_domains: None,
            exclude_domains: None,
            freshness: Some("bogus".to_string()),
            strategy: None,
            provider: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "invalid_argument");
}

// ── web_search credential-missing path ─────────────────────────────────────

#[tokio::test]
async fn web_search_surfaces_missing_credentials_as_permission_denied() {
    let server = make_server_without_db();
    let out = server
        .web_search(Parameters(SearchRequest {
            query: "test".to_string(),
            num_results: None,
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            provider: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "permission_denied");
}

// ── web_extract invalid-argument paths ─────────────────────────────────────

#[tokio::test]
async fn web_extract_rejects_oversized_url() {
    let server = make_server_without_db();
    let oversized_url = format!("http://1.2.3.4/{}", "x".repeat(2100));
    let out = server
        .web_extract(Parameters(ExtractRequest {
            url: oversized_url,
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "invalid_argument");
    assert!(
        json.get("error").is_some_and(|error| error
            .as_str()
            .is_some_and(|message| message.contains("url"))),
        "error should mention url; got: {json}"
    );
}

#[tokio::test]
async fn web_extract_rejects_oversized_json_prompt() {
    let server = make_server_without_db();
    let out = server
        .web_extract(Parameters(ExtractRequest {
            url: LITERAL_IP_URL.to_string(),
            format: None,
            json_prompt: Some("x".repeat(5000)),
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "invalid_argument");
    assert!(
        json.get("error").is_some_and(|error| {
            error
                .as_str()
                .is_some_and(|message| message.contains("json_prompt"))
        }),
        "error should mention json_prompt; got: {json}"
    );
}

// ── web_extract credential-missing path ────────────────────────────────────

#[tokio::test]
async fn web_extract_surfaces_missing_credentials_as_permission_denied() {
    let server = make_server_without_db();
    let out = server
        .web_extract(Parameters(ExtractRequest {
            url: LITERAL_IP_URL.to_string(),
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "permission_denied");
}

// ── web_find_similar credential-missing path ───────────────────────────────

#[tokio::test]
async fn web_find_similar_surfaces_missing_credentials_as_permission_denied() {
    let server = make_server_without_db();
    let out = server
        .web_find_similar(Parameters(FindSimilarRequest {
            url: LITERAL_IP_URL.to_string(),
            num_results: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "permission_denied");
}

// ── web_browse invalid-argument paths ──────────────────────────────────────

#[tokio::test]
async fn web_browse_rejects_oversized_url() {
    let server = make_server_without_db();
    let oversized_url = format!("http://1.2.3.4/{}", "x".repeat(2100));
    let out = server
        .web_browse(Parameters(BrowseRequest {
            url: oversized_url,
            instruction: None,
            timeout_secs: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "invalid_argument");
}

#[tokio::test]
async fn web_browse_rejects_oversized_instruction() {
    let server = make_server_without_db();
    let out = server
        .web_browse(Parameters(BrowseRequest {
            url: LITERAL_IP_URL.to_string(),
            instruction: Some("x".repeat(3000)),
            timeout_secs: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "invalid_argument");
    assert!(
        json.get("error").is_some_and(|error| {
            error
                .as_str()
                .is_some_and(|message| message.contains("instruction"))
        }),
        "error should mention instruction; got: {json}"
    );
}

// ── web_browse credential-missing path ─────────────────────────────────────

#[tokio::test]
async fn web_browse_surfaces_missing_credentials_as_permission_denied() {
    let server = make_server_without_db();
    let out = server
        .web_browse(Parameters(BrowseRequest {
            url: LITERAL_IP_URL.to_string(),
            instruction: None,
            timeout_secs: None,
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "permission_denied");
}

// ── RSS tools without DB (permission_denied) ───────────────────────────────

#[tokio::test]
async fn rss_list_subscriptions_without_db_returns_permission_denied() {
    let server = make_server_without_db();
    let out = server
        .rss_list_subscriptions(Parameters(ListSubscriptionsRequest { folder: None }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "permission_denied");
}

#[tokio::test]
async fn rss_get_unread_count_without_db_returns_permission_denied() {
    let server = make_server_without_db();
    let out = server
        .rss_get_unread_count(Parameters(UnreadCountRequest {
            stream_id: "feed/test".to_string(),
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "permission_denied");
}

#[tokio::test]
async fn rss_export_opml_without_db_returns_permission_denied() {
    let server = make_server_without_db();
    let out = server.rss_export_opml().await;
    let json = parse(&out);
    assert_error_kind(&json, "permission_denied");
}

#[tokio::test]
async fn rss_unsubscribe_without_db_returns_permission_denied() {
    let server = make_server_without_db();
    let out = server
        .rss_unsubscribe(Parameters(UnsubscribeRequest {
            stream_id: "feed/test".to_string(),
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "permission_denied");
}

#[tokio::test]
async fn rss_mark_all_read_without_db_returns_permission_denied() {
    let server = make_server_without_db();
    let out = server
        .rss_mark_all_read(Parameters(MarkReadRequest {
            stream_id: "feed/test".to_string(),
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "permission_denied");
}

// ── RSS tools with DB (happy path) ─────────────────────────────────────────

#[tokio::test]
async fn rss_list_subscriptions_with_empty_db_returns_zero_count() {
    let server = make_server_with_rss_db();
    let out = server
        .rss_list_subscriptions(Parameters(ListSubscriptionsRequest { folder: None }))
        .await;
    let json = parse(&out);
    assert_eq!(
        json.get("count").and_then(|count| count.as_u64()),
        Some(0),
        "empty DB should have 0 subscriptions; got: {json}"
    );
}

#[tokio::test]
async fn rss_get_unread_count_with_empty_db_returns_zero() {
    let server = make_server_with_rss_db();
    let out = server
        .rss_get_unread_count(Parameters(UnreadCountRequest {
            stream_id: "feed/test".to_string(),
        }))
        .await;
    let json = parse(&out);
    assert_eq!(
        json.get("unread_count").and_then(|count| count.as_u64()),
        Some(0),
        "empty DB should have 0 unread; got: {json}"
    );
}

// ── RSS tools with DB (invalid argument) ───────────────────────────────────

#[tokio::test]
async fn rss_get_entries_rejects_non_base64_continuation_token() {
    let server = make_server_with_rss_db();
    let out = server
        .rss_get_entries(Parameters(GetEntriesRequest {
            stream_id: "feed/test".to_string(),
            unread_only: None,
            starred_only: None,
            count: None,
            continuation_token: Some("!!!not-base64!!!".to_string()),
        }))
        .await;
    let json = parse(&out);
    assert_error_kind(&json, "invalid_argument");
    assert!(
        json.get("error").is_some_and(|error| {
            error
                .as_str()
                .is_some_and(|message| message.contains("base64"))
        }),
        "error should mention base64; got: {json}"
    );
}
