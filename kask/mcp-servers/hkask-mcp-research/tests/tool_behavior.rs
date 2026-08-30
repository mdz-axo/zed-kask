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
//!
//! Tools return `Result<String, McpToolError>`: Ok-path tests unwrap the
//! envelope string; error-path tests assert on the typed `McpToolError`
//! (`kind` + `message`) instead of parsing an in-band error envelope.

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
    ExtractedContent, FindSimilarRequest, ProviderFailureRecord, ProviderHealthEntry,
    ProviderRecommendation, RankedResult, RateLimiter, SearchQuery, SearchRequest, SearchStrategy,
    WebError,
};
use hkask_mcp_server::server::McpToolError;
use hkask_types::InferenceError;
use hkask_types::InferencePort;
use hkask_types::InferenceResult;
use hkask_types::McpErrorKind;
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

// ── Stub InferencePort ─────────────────────────────────────────────────────

/// Stub inference port that always fails — pins the degradation contract:
/// the deep strategy must surface the failure reason, never collapse it.
struct FailingInferencePort;

impl InferencePort for FailingInferencePort {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &hkask_types::template::LLMParameters,
        _tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        Box::pin(async {
            Err(InferenceError::Connection(
                "stub: inference bridge down".to_string(),
            ))
        })
    }

    fn rerank<'a>(
        &'a self,
        _model: &str,
        _query: &str,
        _documents: &[String],
    ) -> hkask_types::RerankFuture<'a> {
        Box::pin(async {
            Err(InferenceError::Connection(
                "stub: rerank bridge down".to_string(),
            ))
        })
    }
}

/// Stub inference port whose rerank scores each candidate by document
/// content — pins the success contract: the reranker's native scores reach
/// the caller and the output names `mode: "llm"` with no reason.
struct ScoringInferencePort;

impl InferencePort for ScoringInferencePort {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &hkask_types::template::LLMParameters,
        _tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        Box::pin(async {
            Err(InferenceError::Connection(
                "stub: generate unused in rerank tests".to_string(),
            ))
        })
    }

    fn rerank<'a>(
        &'a self,
        _model: &str,
        _query: &str,
        documents: &[String],
    ) -> hkask_types::RerankFuture<'a> {
        let scores: Vec<hkask_types::inference_ipc::RerankScoreEntry> = documents
            .iter()
            .enumerate()
            .map(|(index, document)| {
                let relevance_score = if document.contains("gamma") {
                    0.90
                } else if document.contains("alpha") {
                    0.50
                } else {
                    0.10
                };
                hkask_types::inference_ipc::RerankScoreEntry {
                    index,
                    relevance_score,
                }
            })
            .collect();
        Box::pin(async move { Ok(scores) })
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
        Arc::new(FailingInferencePort),
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
        Arc::new(FailingInferencePort),
    )
}

fn parse(out: &str) -> serde_json::Value {
    parse_tool_response(out).expect("tool output must be valid JSON")
}

/// Unwrap a successful tool call: the Ok payload is the `{"content": ...}`
/// envelope string.
fn ok(out: Result<String, McpToolError>) -> String {
    out.expect("tool ok")
}

/// Unwrap a failed tool call: the typed error carries `kind` + `message`.
fn err(out: Result<String, McpToolError>) -> McpToolError {
    out.expect_err("tool should fail")
}

fn assert_error_kind(error: &McpToolError, expected_kind: McpErrorKind) {
    assert_eq!(
        error.kind, expected_kind,
        "expected kind '{expected_kind}' but got '{}'; message: {}",
        error.kind, error.message
    );
    assert!(
        !error.message.is_empty(),
        "expected non-empty error message, got: {error:?}"
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
    let out = ok(server.web_ping().await);
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
    let error = err(server
        .web_search(Parameters(SearchRequest {
            query: String::new(),
            num_results: None,
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            provider: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("empty"),
        "error should mention empty query; got: {}",
        error.message
    );
}

#[tokio::test]
async fn web_search_rejects_oversized_query() {
    let server = make_server_without_db();
    let error = err(server
        .web_search(Parameters(SearchRequest {
            query: "x".repeat(500),
            num_results: None,
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            provider: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("maximum length"),
        "error should mention maximum length; got: {}",
        error.message
    );
}

#[tokio::test]
async fn web_search_rejects_unknown_strategy() {
    let server = make_server_without_db();
    let error = err(server
        .web_search(Parameters(SearchRequest {
            query: "test".to_string(),
            num_results: None,
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: Some("bogus".to_string()),
            provider: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
}

#[tokio::test]
async fn web_search_rejects_unknown_freshness() {
    let server = make_server_without_db();
    let error = err(server
        .web_search(Parameters(SearchRequest {
            query: "test".to_string(),
            num_results: None,
            include_domains: None,
            exclude_domains: None,
            freshness: Some("bogus".to_string()),
            strategy: None,
            provider: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
}

// ── web_search credential-missing path ─────────────────────────────────────

#[tokio::test]
async fn web_search_surfaces_missing_credentials_as_permission_denied() {
    let server = make_server_without_db();
    let error = err(server
        .web_search(Parameters(SearchRequest {
            query: "test".to_string(),
            num_results: None,
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            provider: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::PermissionDenied);
}

// ── web_extract invalid-argument paths ─────────────────────────────────────

#[tokio::test]
async fn web_extract_rejects_oversized_url() {
    let server = make_server_without_db();
    let oversized_url = format!("http://1.2.3.4/{}", "x".repeat(2100));
    let error = err(server
        .web_extract(Parameters(ExtractRequest {
            url: oversized_url,
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("url"),
        "error should mention url; got: {}",
        error.message
    );
}

#[tokio::test]
async fn web_extract_rejects_oversized_json_prompt() {
    let server = make_server_without_db();
    let error = err(server
        .web_extract(Parameters(ExtractRequest {
            url: LITERAL_IP_URL.to_string(),
            format: None,
            json_prompt: Some("x".repeat(5000)),
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("json_prompt"),
        "error should mention json_prompt; got: {}",
        error.message
    );
}

// ── web_extract credential-missing path ────────────────────────────────────

#[tokio::test]
async fn web_extract_surfaces_missing_credentials_as_permission_denied() {
    let server = make_server_without_db();
    let error = err(server
        .web_extract(Parameters(ExtractRequest {
            url: LITERAL_IP_URL.to_string(),
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::PermissionDenied);
}

// ── web_find_similar credential-missing path ───────────────────────────────

#[tokio::test]
async fn web_find_similar_surfaces_missing_credentials_as_permission_denied() {
    let server = make_server_without_db();
    let error = err(server
        .web_find_similar(Parameters(FindSimilarRequest {
            url: LITERAL_IP_URL.to_string(),
            num_results: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::PermissionDenied);
}

// ── web_browse invalid-argument paths ──────────────────────────────────────

#[tokio::test]
async fn web_browse_rejects_oversized_url() {
    let server = make_server_without_db();
    let oversized_url = format!("http://1.2.3.4/{}", "x".repeat(2100));
    let error = err(server
        .web_browse(Parameters(BrowseRequest {
            url: oversized_url,
            instruction: None,
            timeout_secs: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
}

#[tokio::test]
async fn web_browse_rejects_oversized_instruction() {
    let server = make_server_without_db();
    let error = err(server
        .web_browse(Parameters(BrowseRequest {
            url: LITERAL_IP_URL.to_string(),
            instruction: Some("x".repeat(3000)),
            timeout_secs: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("instruction"),
        "error should mention instruction; got: {}",
        error.message
    );
}

// ── web_browse credential-missing path ─────────────────────────────────────

#[tokio::test]
async fn web_browse_surfaces_missing_credentials_as_permission_denied() {
    let server = make_server_without_db();
    let error = err(server
        .web_browse(Parameters(BrowseRequest {
            url: LITERAL_IP_URL.to_string(),
            instruction: None,
            timeout_secs: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::PermissionDenied);
}

// ── RSS tools without DB (permission_denied) ───────────────────────────────

#[tokio::test]
async fn rss_list_subscriptions_without_db_returns_permission_denied() {
    let server = make_server_without_db();
    let error = err(server
        .rss_list_subscriptions(Parameters(ListSubscriptionsRequest { folder: None }))
        .await);
    assert_error_kind(&error, McpErrorKind::PermissionDenied);
}

#[tokio::test]
async fn rss_get_unread_count_without_db_returns_permission_denied() {
    let server = make_server_without_db();
    let error = err(server
        .rss_get_unread_count(Parameters(UnreadCountRequest {
            stream_id: "feed/test".to_string(),
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::PermissionDenied);
}

#[tokio::test]
async fn rss_export_opml_without_db_returns_permission_denied() {
    let server = make_server_without_db();
    let error = err(server.rss_export_opml().await);
    assert_error_kind(&error, McpErrorKind::PermissionDenied);
}

#[tokio::test]
async fn rss_unsubscribe_without_db_returns_permission_denied() {
    let server = make_server_without_db();
    let error = err(server
        .rss_unsubscribe(Parameters(UnsubscribeRequest {
            stream_id: "feed/test".to_string(),
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::PermissionDenied);
}

#[tokio::test]
async fn rss_mark_all_read_without_db_returns_permission_denied() {
    let server = make_server_without_db();
    let error = err(server
        .rss_mark_all_read(Parameters(MarkReadRequest {
            stream_id: "feed/test".to_string(),
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::PermissionDenied);
}

// ── RSS tools with DB (happy path) ─────────────────────────────────────────

#[tokio::test]
async fn rss_list_subscriptions_with_empty_db_returns_zero_count() {
    let server = make_server_with_rss_db();
    let out = ok(server
        .rss_list_subscriptions(Parameters(ListSubscriptionsRequest { folder: None }))
        .await);
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
    let out = ok(server
        .rss_get_unread_count(Parameters(UnreadCountRequest {
            stream_id: "feed/test".to_string(),
        }))
        .await);
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
    let error = err(server
        .rss_get_entries(Parameters(GetEntriesRequest {
            stream_id: "feed/test".to_string(),
            unread_only: None,
            starred_only: None,
            count: None,
            continuation_token: Some("!!!not-base64!!!".to_string()),
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("base64"),
        "error should mention base64; got: {}",
        error.message
    );
}

// ── web_search deep-strategy LLM rerank ────────────────────────────────────

/// Stub pool that returns three fixed results for any search — lets the
/// deep-strategy rerank stage run against a stub inference port without
/// touching the network.
struct FixedResultsPool;

#[async_trait]
impl WebSearchPort for FixedResultsPool {
    async fn search(
        &self,
        query: &SearchQuery,
        _strategy: SearchStrategy,
        _provider: Option<&str>,
    ) -> Result<CompoundSearchResult, WebError> {
        let result = |title: &str, url: &str| RankedResult {
            title: title.to_string(),
            url: url.to_string(),
            description: None,
            source: None,
            published: None,
            rrf_score: 1.0,
            provider_count: 1,
            providers: vec!["stub".to_string()],
            best_rank: None,
            content_preview: None,
            semantic_score: None,
            extracted_content: None,
        };
        Ok(CompoundSearchResult {
            query: query.query.clone(),
            strategy: "deep".to_string(),
            results: vec![
                result("Alpha", "https://example.com/alpha"),
                result("Beta", "https://example.com/beta"),
                result("Gamma", "https://example.com/gamma"),
            ],
            answer_box: None,
            related_questions: Vec::new(),
            providers_queried: Vec::new(),
            providers_succeeded: vec!["stub".to_string()],
            providers_failed: Vec::new(),
            total_before_dedup: 3,
            duplicates_removed: 0,
        })
    }

    async fn find_similar(
        &self,
        _url: &str,
        _num_results: u32,
    ) -> Result<ProviderSearchOutput, WebError> {
        Err(WebError::NoProviderConfigured("stub".to_string()))
    }

    async fn extract(
        &self,
        _url: &str,
        _opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        Err(WebError::NoProviderConfigured("stub".to_string()))
    }

    async fn browse(
        &self,
        _url: &str,
        _instruction: &str,
        _timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        Err(WebError::NoProviderConfigured("stub".to_string()))
    }

    async fn health_check(&self) -> Vec<ProviderHealthEntry> {
        Vec::new()
    }

    fn provider_fingerprint(&self) -> String {
        "stub-fixed-results".to_string()
    }

    fn provider_kinds(&self) -> Vec<String> {
        Vec::new()
    }

    fn score_providers(&self, _query: &str, _intent: Option<&str>) -> Vec<ProviderRecommendation> {
        Vec::new()
    }
}

fn make_server_with_pool_and_port(
    pool: Arc<dyn WebSearchPort>,
    inference_port: Arc<dyn InferencePort>,
) -> ResearchServer {
    ResearchServer::new(
        WebID::new(),
        pool,
        Arc::new(ResponseCache::new(10, Duration::from_secs(60))),
        RateLimiter::new(10000, 60),
        None,
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        inference_port,
    )
}

fn deep_search_request() -> SearchRequest {
    SearchRequest {
        query: "test query".to_string(),
        num_results: Some(10),
        include_domains: None,
        exclude_domains: None,
        freshness: None,
        strategy: Some("deep".to_string()),
        provider: None,
    }
}

/// Success contract: the LLM's per-candidate scores reach the caller
/// (descending score order) and the output names `mode: "llm"` with no
/// reason.
#[tokio::test]
async fn deep_search_llm_rerank_reorders_results() {
    let server =
        make_server_with_pool_and_port(Arc::new(FixedResultsPool), Arc::new(ScoringInferencePort));
    let output = parse(&ok(server
        .web_search(Parameters(deep_search_request()))
        .await));

    let rerank = output
        .get("rerank")
        .expect("deep strategy must surface rerank info");
    assert_eq!(rerank.get("mode").and_then(|m| m.as_str()), Some("llm"));
    assert!(
        rerank.get("reason").is_none(),
        "fully successful rerank must not claim a degradation"
    );
    let urls: Vec<&str> = output["results"]
        .as_array()
        .expect("results array")
        .iter()
        .map(|r| r["url"].as_str().expect("url"))
        .collect();
    assert_eq!(
        urls,
        vec![
            "https://example.com/gamma",
            "https://example.com/alpha",
            "https://example.com/beta",
        ],
        "descending score order (gamma 90, alpha 50, beta 10) must reach the caller"
    );
}

/// Degradation contract: when the inference bridge is down, the heuristic
/// order is kept AND the failure reason is surfaced — never a silent
/// fallback.
#[tokio::test]
async fn deep_search_llm_rerank_failure_surfaces_heuristic_mode() {
    let server =
        make_server_with_pool_and_port(Arc::new(FixedResultsPool), Arc::new(FailingInferencePort));
    let output = parse(&ok(server
        .web_search(Parameters(deep_search_request()))
        .await));

    let rerank = output
        .get("rerank")
        .expect("deep strategy must surface rerank info even on failure");
    assert_eq!(
        rerank.get("mode").and_then(|m| m.as_str()),
        Some("heuristic")
    );
    let reason = rerank
        .get("reason")
        .and_then(|r| r.as_str())
        .expect("heuristic fallback must name the cause");
    assert!(
        reason.contains("inference"),
        "reason should name the inference failure; got: {reason}"
    );
    let urls: Vec<&str> = output["results"]
        .as_array()
        .expect("results array")
        .iter()
        .map(|r| r["url"].as_str().expect("url"))
        .collect();
    assert_eq!(
        urls,
        vec![
            "https://example.com/alpha",
            "https://example.com/beta",
            "https://example.com/gamma",
        ],
        "heuristic order must be kept on rerank failure"
    );
}

/// Non-deep strategies do not rerank — no rerank field in the output.
#[tokio::test]
async fn quick_search_has_no_rerank_field() {
    let server =
        make_server_with_pool_and_port(Arc::new(FixedResultsPool), Arc::new(FailingInferencePort));
    let mut request = deep_search_request();
    request.strategy = Some("quick".to_string());
    let output = parse(&ok(server.web_search(Parameters(request)).await));
    assert!(
        output.get("rerank").is_none(),
        "quick strategy must not claim a rerank stage"
    );
}

/// A pool whose first search carries a provider failure (as single-provider
/// mode maps an Err into an Ok compound with `providers_failed`), then
/// succeeds. Pins the cache gate: the failure must NOT be cached — the second
/// identical call must see the recovered results, not a replayed empty
/// "success". Before the gate, a transient provider failure was replayed as
/// a successful empty result for the full cache TTL.
struct FailThenSucceedPool {
    failed_once: std::sync::Mutex<bool>,
}

#[async_trait]
impl WebSearchPort for FailThenSucceedPool {
    async fn search(
        &self,
        query: &SearchQuery,
        _strategy: SearchStrategy,
        _provider: Option<&str>,
    ) -> Result<CompoundSearchResult, WebError> {
        let mut failed_once = self.failed_once.lock().unwrap_or_else(|e| e.into_inner());
        if *failed_once {
            return Ok(CompoundSearchResult {
                query: query.query.clone(),
                strategy: "quick".to_string(),
                results: vec![RankedResult {
                    title: "Recovered".to_string(),
                    url: "https://example.com/recovered".to_string(),
                    description: None,
                    source: None,
                    published: None,
                    rrf_score: 1.0,
                    provider_count: 1,
                    providers: vec!["stub".to_string()],
                    best_rank: None,
                    content_preview: None,
                    semantic_score: None,
                    extracted_content: None,
                }],
                answer_box: None,
                related_questions: Vec::new(),
                providers_queried: Vec::new(),
                providers_succeeded: vec!["stub".to_string()],
                providers_failed: Vec::new(),
                total_before_dedup: 1,
                duplicates_removed: 0,
            });
        }
        *failed_once = true;
        Ok(CompoundSearchResult {
            query: query.query.clone(),
            strategy: "quick".to_string(),
            results: Vec::new(),
            answer_box: None,
            related_questions: Vec::new(),
            providers_queried: Vec::new(),
            providers_succeeded: Vec::new(),
            providers_failed: vec![ProviderFailureRecord {
                kind: "stub".to_string(),
                error: "transient failure".to_string(),
            }],
            total_before_dedup: 0,
            duplicates_removed: 0,
        })
    }

    async fn find_similar(
        &self,
        _url: &str,
        _num_results: u32,
    ) -> Result<ProviderSearchOutput, WebError> {
        Err(WebError::NoProviderConfigured("stub".to_string()))
    }

    async fn extract(
        &self,
        _url: &str,
        _opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        Err(WebError::NoProviderConfigured("stub".to_string()))
    }

    async fn browse(
        &self,
        _url: &str,
        _instruction: &str,
        _timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        Err(WebError::NoProviderConfigured("stub".to_string()))
    }

    async fn health_check(&self) -> Vec<ProviderHealthEntry> {
        Vec::new()
    }

    fn provider_fingerprint(&self) -> String {
        "stub-fail-then-succeed".to_string()
    }

    fn provider_kinds(&self) -> Vec<String> {
        Vec::new()
    }

    fn score_providers(&self, _query: &str, _intent: Option<&str>) -> Vec<ProviderRecommendation> {
        Vec::new()
    }
}

#[tokio::test]
async fn web_search_does_not_cache_provider_failures() {
    let server = make_server_with_pool_and_port(
        Arc::new(FailThenSucceedPool {
            failed_once: std::sync::Mutex::new(false),
        }),
        Arc::new(FailingInferencePort),
    );
    let make_request = || SearchRequest {
        query: "cache gate".to_string(),
        num_results: Some(5),
        include_domains: None,
        exclude_domains: None,
        freshness: None,
        strategy: Some("quick".to_string()),
        provider: None,
    };

    // First call: the failure is surfaced (degradation contract)…
    let first = parse(&ok(server.web_search(Parameters(make_request())).await));
    assert!(
        first["providers_failed"]
            .as_array()
            .is_some_and(|f| !f.is_empty()),
        "the transient failure must be surfaced in the first response"
    );

    // …and must NOT be replayed from cache on the identical second call.
    let second = parse(&ok(server.web_search(Parameters(make_request())).await));
    assert_eq!(
        second["count"], 1,
        "the recovered result must reach the caller — a cached failure would return count 0"
    );
    assert!(
        second["providers_failed"]
            .as_array()
            .is_some_and(|f| f.is_empty()),
        "the second response must carry no failure record"
    );
}
