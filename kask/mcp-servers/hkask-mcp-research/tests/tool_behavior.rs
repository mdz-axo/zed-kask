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
//! - RSS tools without a DB → `permission_denied` (the `require_research_db!` gate).
//! - RSS tools with an in-memory DB → happy path (empty list, zero unread)
//!   and invalid-argument (malformed continuation token).
//!
//! Tools return `Result<String, McpToolError>`: Ok-path tests unwrap the
//! envelope string; error-path tests assert on the typed `McpToolError`
//! (`kind` + `message`) instead of parsing an in-band error envelope.

#![forbid(unsafe_code)]

use hkask_mcp_research::ResearchServer;
use hkask_mcp_research::research::cache::ResponseCache;
use hkask_mcp_research::research::db::RESEARCH_SCHEMA_DDL;
use hkask_mcp_research::research::providers::{ProviderSearchOutput, WebSearchPort};
use hkask_mcp_research::research::rss_types::{
    DiscoverRequest, GetEntriesRequest, ListSubscriptionsRequest, MarkReadRequest,
    UnreadCountRequest, UnsubscribeRequest,
};
use hkask_mcp_research::research::types::{
    AnnotateResearchRunRequest, BeginResearchRunRequest, BrowseRequest, BrowseResult,
    CompoundSearchResult, EvaluateArtifact, EvaluateEvidenceRequest, ExtractOptions,
    ExtractRequest, ExtractedContent, FindSimilarRequest, GetResearchRunRequest, LatencyTier,
    ProviderFailureRecord, ProviderHealthEntry, ProviderInfo, ProviderRecommendation, RankedResult,
    RateLimiter, ResolvePaperRequest, SearchQuery, SearchRequest, SearchStrategy, WebError,
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
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        Arc::new(FailingInferencePort),
        None,
        None,
    )
}

fn research_db_pool() -> r2d2::Pool<hkask_storage::SqliteConnectionManager> {
    let manager = hkask_storage::SqliteConnectionManager::memory();
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .build(manager)
        .expect("r2d2 pool build");
    {
        let connection = pool.get().expect("r2d2 pool get");
        connection
            .execute_batch(RESEARCH_SCHEMA_DDL)
            .expect("research schema init");
    }
    pool
}

fn make_server_with_research_db() -> ResearchServer {
    ResearchServer::new(
        WebID::new(),
        Arc::new(NoCredentialsPool),
        Arc::new(ResponseCache::new(10, Duration::from_secs(60))),
        RateLimiter::new(10000, 60),
        Some(research_db_pool()),
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        Arc::new(FailingInferencePort),
        None,
        None,
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
            run_id: None,
            intent: None,
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
            run_id: None,
            intent: None,
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
            run_id: None,
            intent: None,
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
            run_id: None,
            intent: None,
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
            run_id: None,
            intent: None,
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
            run_id: None,
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
            run_id: None,
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
            run_id: None,
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
            run_id: None,
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

// ── web_extract / web_browse SSRF destination gate ─────────────────────

/// Control: the pre-existing strict gate refuses a loopback destination at
/// the tool layer, before any provider is consulted.
#[tokio::test]
async fn web_extract_rejects_loopback_destination() {
    let server = make_server_without_db();
    let error = err(server
        .web_extract(Parameters(ExtractRequest {
            url: "http://127.0.0.1:9/path".to_string(),
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
            run_id: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("Loopback"),
        "error must name the loopback rejection: {}",
        error.message
    );
}

/// expect: "An URL that connects to loopback by spelling 'this network'
/// must be rejected." [P4]
/// On Linux, connecting to 0.0.0.0 routes to loopback, so it must not pass
/// the destination gate (the pre-fix validator admitted it).
#[tokio::test]
async fn web_extract_rejects_unspecified_destination() {
    let server = make_server_without_db();
    let error = err(server
        .web_extract(Parameters(ExtractRequest {
            url: "http://0.0.0.0:6379/".to_string(),
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
            run_id: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("Unspecified"),
        "error must name the unspecified rejection: {}",
        error.message
    );
}

/// expect: "Neither address family can re-spell a forbidden IPv4
/// destination." [P4]
/// 64:ff9b::7f00:1 is 127.0.0.1 under the NAT64 well-known prefix.
#[tokio::test]
async fn web_extract_rejects_nat64_loopback_destination() {
    let server = make_server_without_db();
    let error = err(server
        .web_extract(Parameters(ExtractRequest {
            url: "http://[64:ff9b::7f00:1]:9/".to_string(),
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
            run_id: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("Loopback"),
        "error must name the loopback rejection: {}",
        error.message
    );
}

#[tokio::test]
async fn web_browse_rejects_unspecified_destination() {
    let server = make_server_without_db();
    let error = err(server
        .web_browse(Parameters(BrowseRequest {
            url: "http://0.0.0.0:6379/".to_string(),
            instruction: None,
            timeout_secs: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("Unspecified"),
        "error must name the unspecified rejection: {}",
        error.message
    );
}

/// Pins the strict destination gate for the discover tool: its initial URL is
/// user-supplied (unlike user-curated RSS feeds), so it goes through the same
/// strict validation as `web_extract` — including the unspecified-address
/// class that connects to loopback on Linux.
#[tokio::test]
async fn rss_discover_feeds_rejects_unspecified_destination() {
    let server = make_server_without_db();
    let error = err(server
        .rss_discover_feeds(Parameters(DiscoverRequest {
            url: "http://0.0.0.0:6379/".to_string(),
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("Unspecified"),
        "error must name the unspecified rejection: {}",
        error.message
    );
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
    let server = make_server_with_research_db();
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
    let server = make_server_with_research_db();
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
    let server = make_server_with_research_db();
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
    rerank_model: Option<&str>,
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
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        inference_port,
        rerank_model.map(str::to_string),
        None,
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
        intent: None,
        provider: None,
        run_id: None,
    }
}

/// Success contract: the LLM's per-candidate scores reach the caller
/// (descending score order) and the output names `mode: "llm"` with no
/// reason.
#[tokio::test]
async fn deep_search_llm_rerank_reorders_results() {
    let server = make_server_with_pool_and_port(
        Arc::new(FixedResultsPool),
        Arc::new(ScoringInferencePort),
        Some("test-rerank-model"),
    );
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
    let server = make_server_with_pool_and_port(
        Arc::new(FixedResultsPool),
        Arc::new(FailingInferencePort),
        Some("test-rerank-model"),
    );
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
    let server = make_server_with_pool_and_port(
        Arc::new(FixedResultsPool),
        Arc::new(FailingInferencePort),
        Some("test-rerank-model"),
    );
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
        None,
    );
    let make_request = || SearchRequest {
        query: "cache gate".to_string(),
        num_results: Some(5),
        include_domains: None,
        exclude_domains: None,
        freshness: None,
        strategy: Some("quick".to_string()),
        intent: None,
        provider: None,
        run_id: None,
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

// ── web_search intent-driven provider selection ─────────────────────────────

/// A pool fake with one configured recommendation: records the provider the
/// tool selected, and returns a minimal successful compound. Pins the
/// folded-in deliberate-selection path (the former web_recommend_provider +
/// web_search(provider) two-step): intent set, provider unset → the top
/// configured recommendation is queried and the ranking is surfaced.
struct IntentSelectionPool {
    selected_provider: std::sync::Mutex<Vec<Option<String>>>,
}

#[async_trait]
impl WebSearchPort for IntentSelectionPool {
    async fn search(
        &self,
        _query: &SearchQuery,
        _strategy: SearchStrategy,
        provider: Option<&str>,
    ) -> Result<CompoundSearchResult, WebError> {
        self.selected_provider
            .lock()
            .unwrap()
            .push(provider.map(str::to_string));
        Ok(CompoundSearchResult {
            query: _query.query.clone(),
            strategy: "quick".to_string(),
            results: Vec::new(),
            providers_queried: vec![ProviderInfo {
                kind: provider.unwrap_or("default").to_string(),
                capabilities: Vec::new(),
            }],
            providers_succeeded: vec![provider.unwrap_or("default").to_string()],
            providers_failed: Vec::new(),
            answer_box: None,
            related_questions: Vec::new(),
            total_before_dedup: 0,
            duplicates_removed: 0,
        })
    }

    async fn find_similar(
        &self,
        _url: &str,
        _num_results: u32,
    ) -> Result<ProviderSearchOutput, WebError> {
        Err(WebError::NoProviderConfigured("not configured".to_string()))
    }

    async fn extract(
        &self,
        _url: &str,
        _opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        Err(WebError::NoProviderConfigured("not configured".to_string()))
    }

    async fn browse(
        &self,
        _url: &str,
        _instruction: &str,
        _timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        Err(WebError::NoProviderConfigured("not configured".to_string()))
    }

    async fn health_check(&self) -> Vec<ProviderHealthEntry> {
        Vec::new()
    }

    fn provider_fingerprint(&self) -> String {
        "intent-selection-stub".to_string()
    }

    fn provider_kinds(&self) -> Vec<String> {
        vec!["arxiv".to_string()]
    }

    fn score_providers(&self, _query: &str, intent: Option<&str>) -> Vec<ProviderRecommendation> {
        vec![
            ProviderRecommendation {
                kind: "arxiv".to_string(),
                score: 1.0,
                rationale: format!("best for {} intent", intent.unwrap_or("general")),
                cost_per_call_usd: 0.0,
                latency_tier: LatencyTier::Fast,
                strengths: Vec::new(),
                weaknesses: Vec::new(),
                best_for: Vec::new(),
                configured: true,
                live_success_rate: None,
                live_p50_latency_ms: None,
                live_sample_count: None,
            },
            ProviderRecommendation {
                kind: "tavily".to_string(),
                score: 5.0,
                rationale: "not configured".to_string(),
                cost_per_call_usd: 0.01,
                latency_tier: LatencyTier::Fast,
                strengths: Vec::new(),
                weaknesses: Vec::new(),
                best_for: Vec::new(),
                configured: false,
                live_success_rate: None,
                live_p50_latency_ms: None,
                live_sample_count: None,
            },
        ]
    }
}

#[tokio::test]
async fn web_search_intent_selects_top_configured_provider_and_surfaces_ranking() {
    let server = ResearchServer::new(
        WebID::new(),
        Arc::new(IntentSelectionPool {
            selected_provider: std::sync::Mutex::new(Vec::new()),
        }),
        Arc::new(ResponseCache::new(0, Duration::from_secs(60))),
        RateLimiter::new(10000, 60),
        None,
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        Arc::new(FailingInferencePort),
        None,
        None,
    );
    let output = server
        .web_search(Parameters(SearchRequest {
            query: "rust async runtime benchmarks".to_string(),
            num_results: Some(5),
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            intent: Some("academic".to_string()),
            provider: None,
            run_id: None,
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);

    // The ranking is surfaced — the choice is auditable.
    let recommendations = parsed["provider_recommendations"]
        .as_array()
        .expect("provider_recommendations array");
    assert_eq!(recommendations.len(), 2, "both recommendations surfaced");
    assert_eq!(recommendations[0]["kind"].as_str(), Some("arxiv"));

    // The top CONFIGURED provider was queried (not the unconfigured tavily)
    // and surfaced as selected_provider.
    assert_eq!(
        parsed["selected_provider"].as_str(),
        Some("arxiv"),
        "the top configured recommendation is the selected provider, got: {parsed}"
    );
}

// ── evaluate_evidence (signal model) ───────────────────────────────────────

fn evidence_artifact(
    url: &str,
    source: Option<&str>,
    published: Option<&str>,
    content: Option<&str>,
) -> EvaluateArtifact {
    EvaluateArtifact {
        url: url.to_string(),
        title: Some("title".to_string()),
        source: source.map(str::to_string),
        published: published.map(str::to_string),
        content: content.map(str::to_string),
    }
}

#[tokio::test]
async fn evaluate_evidence_rejects_empty_question() {
    let server = make_server_without_db();
    let error = err(server
        .evaluate_evidence(Parameters(EvaluateEvidenceRequest {
            question: "  ".to_string(),
            duplication: None,
            artifacts: vec![evidence_artifact(
                "https://a.example/1",
                Some("a.example"),
                None,
                None,
            )],
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
}

#[tokio::test]
async fn evaluate_evidence_rejects_empty_artifacts() {
    let server = make_server_without_db();
    let error = err(server
        .evaluate_evidence(Parameters(EvaluateEvidenceRequest {
            question: "what is the evidence?".to_string(),
            duplication: None,
            artifacts: Vec::new(),
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
}

#[tokio::test]
async fn evaluate_evidence_emits_signal_model() {
    let server = make_server_without_db();
    let out = ok(server
        .evaluate_evidence(Parameters(EvaluateEvidenceRequest {
            question: "is the claim corroborated?".to_string(),
            duplication: None,
            artifacts: vec![
                evidence_artifact(
                    "https://a.example/1",
                    Some("a.example"),
                    Some("not a date"),
                    Some("alpha one two three four"),
                ),
                evidence_artifact("https://b.example/1", Some("b.example"), None, None),
            ],
        }))
        .await);
    let json = parse(&out);

    let artifacts = json["artifacts"].as_array().expect("artifacts array");
    assert_eq!(artifacts.len(), 2);

    // The signal trail: four components with earned values and basis strings.
    let signals = artifacts[0]["signals"].as_array().expect("signals array");
    assert_eq!(signals.len(), 4);
    let components: Vec<&str> = signals
        .iter()
        .filter_map(|signal| signal["component"].as_str())
        .collect();
    assert_eq!(components, ["base", "corroboration", "recency", "content"]);
    for signal in signals {
        assert!(
            signal["basis"]
                .as_str()
                .is_some_and(|basis| !basis.is_empty()),
            "basis string required: {signal}"
        );
    }

    // The deleted flat booleans are gone — the signals carry the information.
    assert!(
        artifacts[0].get("has_published_date").is_none(),
        "has_published_date deleted: {json}"
    );
    assert!(
        artifacts[0].get("has_content").is_none(),
        "has_content deleted: {json}"
    );

    // An unparseable date is stated in the recency basis, not a
    // dual-meaning flag.
    let recency = signals
        .iter()
        .find(|signal| signal["component"].as_str() == Some("recency"))
        .expect("recency signal");
    assert!(
        recency["basis"]
            .as_str()
            .is_some_and(|basis| basis.contains("unparseable")),
        "basis: {recency}"
    );

    // The set block: domains, clusters, sensitivity, duplication mode.
    let set = &json["set"];
    assert_eq!(set["distinct_domains"].as_u64(), Some(2));
    assert_eq!(set["sourced_count"].as_u64(), Some(2));
    assert!(
        set["content_clusters"]
            .as_array()
            .is_some_and(|clusters| !clusters.is_empty()),
        "content_clusters: {json}"
    );
    assert_eq!(set["duplication_mode"].as_str(), Some("shingles"));
    assert!(
        set["sensitivity"].get("status").is_some(),
        "sensitivity status: {json}"
    );

    // The ontology labeling contract is retained (fixture-guarded keys).
    assert!(
        artifacts[0].get("SEPIO:0000167").is_some(),
        "SEPIO confidence key: {json}"
    );
    assert!(
        artifacts[0].get("SEPIO:0000440").is_some(),
        "SEPIO supporting-evidence key: {json}"
    );
    assert_eq!(
        json.get("pko:StepVerification")
            .and_then(|value| value.as_str()),
        Some("evidence_quality_assessed")
    );
}

#[tokio::test]
async fn evaluate_evidence_syndication_visible_in_clusters() {
    let server = make_server_without_db();
    let out = ok(server
        .evaluate_evidence(Parameters(EvaluateEvidenceRequest {
            question: "did the wire story spread?".to_string(),
            duplication: None,
            artifacts: vec![
                evidence_artifact(
                    "https://a.example/1",
                    Some("a.example"),
                    None,
                    Some("wire story body text alpha beta gamma delta"),
                ),
                evidence_artifact(
                    "https://b.example/1",
                    Some("b.example"),
                    None,
                    Some("wire story body text alpha beta gamma delta"),
                ),
                evidence_artifact(
                    "https://c.example/1",
                    Some("c.example"),
                    None,
                    Some("wire story body text alpha beta gamma delta"),
                ),
            ],
        }))
        .await);
    let json = parse(&out);
    let artifacts = json["artifacts"].as_array().expect("artifacts array");
    // One syndicated story: one unit, not three corroborations.
    assert_eq!(artifacts[0]["corroboration_count"].as_u64(), Some(1));
    let clusters = json["set"]["content_clusters"]
        .as_array()
        .expect("content_clusters");
    assert_eq!(clusters.len(), 1);
    assert_eq!(
        clusters[0]["domains"]
            .as_array()
            .map(|domains| domains.len()),
        Some(3)
    );
    assert_eq!(
        clusters[0]["artifact_urls"]
            .as_array()
            .map(|urls| urls.len()),
        Some(3)
    );
}

// ── Research-run ledger schema ─────────────────────────────────────────────

#[test]
fn research_schema_creates_run_tables() {
    // The run ledger lives in the same encrypted DB as the feed substrate
    // — one DB, one passphrase, one pool (essentialist G3). The run tables
    // must exist after the schema DDL runs.
    let manager = hkask_storage::SqliteConnectionManager::memory();
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .build(manager)
        .expect("r2d2 pool build");
    let connection = pool.get().expect("r2d2 pool get");
    connection
        .execute_batch(RESEARCH_SCHEMA_DDL)
        .expect("schema init");

    for table in ["research_runs", "run_sources"] {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("sqlite_master query");
        assert_eq!(count, 1, "table {table} must exist after the DDL");
    }

    // run_sources carries the audit copy (excerpt — what the server
    // actually returned, capped) and the corpus composition seam
    // (corpus_ref — the agent's entity_ref for the durable recall copy).
    let mut statement = connection
        .prepare("PRAGMA table_info(run_sources)")
        .expect("pragma table_info");
    let columns: Vec<String> = statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query_map columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("column rows");
    for column in [
        "run_id",
        "url",
        "provider",
        "title",
        "published",
        "source",
        "excerpt",
        "corpus_ref",
        "recorded_by",
        "verification_state",
        "verification_basis",
        "recorded_at",
    ] {
        assert!(
            columns.contains(&column.to_string()),
            "column {column} missing: {columns:?}"
        );
    }

    // Cross-run audit queries ("was this URL ever consulted?") need only
    // the index now; a lookup tool waits for a named consumer.
    let index_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_run_sources_url'",
            [],
            |row| row.get(0),
        )
        .expect("index query");
    assert_eq!(index_count, 1, "idx_run_sources_url must exist");
}

// ── Research-run ledger tools ──────────────────────────────────────────────

fn make_server_with_pool_and_db(
    pool: Arc<dyn WebSearchPort>,
    database: Option<r2d2::Pool<hkask_storage::SqliteConnectionManager>>,
) -> ResearchServer {
    ResearchServer::new(
        WebID::new(),
        pool,
        Arc::new(ResponseCache::new(10, Duration::from_secs(60))),
        RateLimiter::new(10000, 60),
        database,
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        Arc::new(FailingInferencePort),
        None,
        None,
    )
}

#[tokio::test]
async fn begin_research_run_requires_db() {
    let server = make_server_without_db();
    let error = err(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "what supports the claim?".to_string(),
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::PermissionDenied);
    assert!(
        error.message.contains("HKASK_RESEARCH_DB"),
        "message names the env var: {}",
        error.message
    );
}

#[tokio::test]
async fn begin_research_run_rejects_empty_question() {
    let server = make_server_with_research_db();
    let error = err(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "   ".to_string(),
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
}

#[tokio::test]
async fn begin_and_get_research_run_roundtrip() {
    let server = make_server_with_research_db();
    let begun = parse(&ok(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "is the wire story corroborated?".to_string(),
        }))
        .await));
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();
    assert_eq!(begun["status"].as_str(), Some("planned"));
    assert_eq!(run_id.len(), 16, "run_id is blake3[..16] hex: {run_id}");

    let manifest = parse(&ok(server
        .get_research_run(Parameters(GetResearchRunRequest {
            run_id: run_id.clone(),
        }))
        .await));
    assert_eq!(manifest["run_id"].as_str(), Some(run_id.as_str()));
    assert_eq!(
        manifest["question"].as_str(),
        Some("is the wire story corroborated?")
    );
    assert_eq!(manifest["status"].as_str(), Some("planned"));
    assert!(
        manifest["sources"]
            .as_array()
            .is_some_and(|sources| sources.is_empty()),
        "a fresh run has no sources: {manifest}"
    );
}

#[tokio::test]
async fn get_research_run_unknown_id_is_not_found() {
    let server = make_server_with_research_db();
    let error = err(server
        .get_research_run(Parameters(GetResearchRunRequest {
            run_id: "deadbeefdeadbeef".to_string(),
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::NotFound);
}

#[tokio::test]
async fn web_search_with_run_id_records_sources_server_side() {
    // The non-repudiation path: a run-scoped search records what the tool
    // actually returned (recorded_by='server'), visibly in the search
    // output (run_ledger note) and in the manifest (with server-side
    // recomputed confidence).
    let server = make_server_with_pool_and_db(Arc::new(FixedResultsPool), Some(research_db_pool()));

    let begun = parse(&ok(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "stub results".to_string(),
        }))
        .await));
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    let output = parse(&ok(server
        .web_search(Parameters(SearchRequest {
            query: "stub query".to_string(),
            num_results: Some(10),
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: Some("deep".to_string()),
            intent: None,
            provider: None,
            run_id: Some(run_id.clone()),
        }))
        .await));
    assert_eq!(
        output["run_ledger"]["recorded"].as_u64(),
        Some(3),
        "three stub results recorded: {output}"
    );

    let manifest = parse(&ok(server
        .get_research_run(Parameters(GetResearchRunRequest { run_id }))
        .await));
    let sources = manifest["sources"].as_array().expect("sources array");
    assert_eq!(sources.len(), 3);
    for source in sources {
        assert_eq!(source["recorded_by"].as_str(), Some("server"));
        assert!(
            source["url"]
                .as_str()
                .is_some_and(|url| url.contains("example.com")),
            "stub url recorded: {source}"
        );
        assert!(
            source["confidence"].as_f64().is_some(),
            "server-side recomputed confidence present: {source}"
        );
    }
}

#[tokio::test]
async fn web_search_with_unknown_run_id_surfaces_ledger_note() {
    // A run_id the ledger does not know fails the append — surfaced as a
    // run_ledger note in the output (the search itself succeeded), never
    // swallowed and never an error return.
    let server = make_server_with_pool_and_db(Arc::new(FixedResultsPool), Some(research_db_pool()));

    let output = parse(&ok(server
        .web_search(Parameters(SearchRequest {
            query: "stub query".to_string(),
            num_results: Some(10),
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            intent: None,
            provider: None,
            run_id: Some("deadbeefdeadbeef".to_string()),
        }))
        .await));
    let note = &output["run_ledger"];
    assert_eq!(note["recorded"].as_u64(), Some(0), "note: {note}");
    assert!(
        note["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty()),
        "failure surfaced: {note}"
    );
}

// ── Research-run annotation and validation gate ────────────────────────────

#[tokio::test]
async fn annotate_verified_requires_server_recorded_source() {
    // The fail-closed gate (Decision 4, strict): an annotation about a
    // source the server never served under this run is invalid_argument —
    // the server refuses verification claims about its own output it
    // cannot check.
    let server = make_server_with_research_db();
    let begun = parse(&ok(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "never-served sources".to_string(),
        }))
        .await));
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    let error = err(server
        .annotate_research_run(Parameters(AnnotateResearchRunRequest {
            run_id,
            url: "https://never-served.example/1".to_string(),
            verification_state: "verified".to_string(),
            basis: Some("I checked it elsewhere".to_string()),
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("server"),
        "message names the violated rule: {}",
        error.message
    );
}

#[tokio::test]
async fn annotate_verified_without_basis_is_invalid_argument() {
    let server = make_server_with_pool_and_db(Arc::new(FixedResultsPool), Some(research_db_pool()));
    let begun = parse(&ok(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "basis required".to_string(),
        }))
        .await));
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    // Record a source the server actually served.
    let search = parse(&ok(server
        .web_search(Parameters(SearchRequest {
            query: "stub query".to_string(),
            num_results: Some(10),
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            intent: None,
            provider: None,
            run_id: Some(run_id.clone()),
        }))
        .await));
    let served_url = search["results"][0]["url"]
        .as_str()
        .expect("stub result url")
        .to_string();

    let error = err(server
        .annotate_research_run(Parameters(AnnotateResearchRunRequest {
            run_id: run_id.clone(),
            url: served_url,
            verification_state: "verified".to_string(),
            basis: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("basis"),
        "message names the basis requirement: {}",
        error.message
    );
}

#[tokio::test]
async fn annotate_verified_on_server_recorded_source_roundtrips() {
    let server = make_server_with_pool_and_db(Arc::new(FixedResultsPool), Some(research_db_pool()));
    let begun = parse(&ok(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "roundtrip".to_string(),
        }))
        .await));
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    let search = parse(&ok(server
        .web_search(Parameters(SearchRequest {
            query: "stub query".to_string(),
            num_results: Some(10),
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            intent: None,
            provider: None,
            run_id: Some(run_id.clone()),
        }))
        .await));
    let served_url = search["results"][0]["url"]
        .as_str()
        .expect("stub result url")
        .to_string();

    // Annotating twice is idempotent (PRIMARY KEY (run_id, url) upsert —
    // re-annotating after a crash is safe).
    for _ in 0..2 {
        let annotated = parse(&ok(server
            .annotate_research_run(Parameters(AnnotateResearchRunRequest {
                run_id: run_id.clone(),
                url: served_url.clone(),
                verification_state: "verified".to_string(),
                basis: Some("cross-checked against the primary source".to_string()),
            }))
            .await));
        assert_eq!(annotated["run_id"].as_str(), Some(run_id.as_str()));
    }

    let manifest = parse(&ok(server
        .get_research_run(Parameters(GetResearchRunRequest {
            run_id: run_id.clone(),
        }))
        .await));
    let sources = manifest["sources"].as_array().expect("sources");
    assert_eq!(
        sources.len(),
        3,
        "idempotent — no duplicate rows: {manifest}"
    );
    let annotated_row = sources
        .iter()
        .find(|source| source["url"].as_str() == Some(served_url.as_str()))
        .expect("annotated row");
    assert_eq!(
        annotated_row["verification_state"].as_str(),
        Some("verified")
    );
    assert_eq!(
        annotated_row["verification_basis"].as_str(),
        Some("cross-checked against the primary source")
    );
    // The validation block: a manifest with a legitimate verified
    // annotation is valid.
    assert_eq!(
        manifest["validation"]["valid"].as_bool(),
        Some(true),
        "{manifest}"
    );
}

#[tokio::test]
async fn annotate_unknown_run_is_not_found() {
    let server = make_server_with_research_db();
    let error = err(server
        .annotate_research_run(Parameters(AnnotateResearchRunRequest {
            run_id: "deadbeefdeadbeef".to_string(),
            url: "https://a.example/1".to_string(),
            verification_state: "inferred".to_string(),
            basis: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::NotFound);
}

#[tokio::test]
async fn annotate_external_source_as_agent_declared_row() {
    // An agent may declare an EXTERNAL source (one the server never
    // served) with a non-verified state — recorded recorded_by='agent',
    // excluded from verified eligibility, visible in the manifest.
    let server = make_server_with_research_db();
    let begun = parse(&ok(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "external sources".to_string(),
        }))
        .await));
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    let annotated = parse(&ok(server
        .annotate_research_run(Parameters(AnnotateResearchRunRequest {
            run_id: run_id.clone(),
            url: "https://external.example/1".to_string(),
            verification_state: "inferred".to_string(),
            basis: Some("prior knowledge".to_string()),
        }))
        .await));
    assert_eq!(annotated["recorded_by"].as_str(), Some("agent"));

    let manifest = parse(&ok(server
        .get_research_run(Parameters(GetResearchRunRequest {
            run_id: run_id.clone(),
        }))
        .await));
    let sources = manifest["sources"].as_array().expect("sources");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["recorded_by"].as_str(), Some("agent"));
    assert_eq!(sources[0]["verification_state"].as_str(), Some("inferred"));
    // An agent-declared inferred row does not violate the manifest.
    assert_eq!(manifest["validation"]["valid"].as_bool(), Some(true));
}

#[tokio::test]
async fn annotate_rejects_unknown_verification_state() {
    let server = make_server_with_research_db();
    let begun = parse(&ok(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "enum check".to_string(),
        }))
        .await));
    let error = err(server
        .annotate_research_run(Parameters(AnnotateResearchRunRequest {
            run_id: begun["run_id"].as_str().expect("run_id").to_string(),
            url: "https://a.example/1".to_string(),
            verification_state: "double-checked".to_string(),
            basis: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("verification_state"),
        "message names the enum: {}",
        error.message
    );
}

// ── Paper resolution (identity) ────────────────────────────────────────────

#[tokio::test]
async fn resolve_paper_canonicalizes_any_identifier_form() {
    let server = make_server_without_db();
    let out = ok(server
        .resolve_paper(Parameters(ResolvePaperRequest {
            query: "https://doi.org/10.1038/s41586-024-00000-x".to_string(),
            run_id: None,
        }))
        .await);
    let json = parse(&out);
    assert_eq!(json["identifier"]["kind"].as_str(), Some("doi"));
    assert_eq!(
        json["identifier"]["value"].as_str(),
        Some("10.1038/s41586-024-00000-x")
    );
    assert_eq!(
        json["canonical_url"].as_str(),
        Some("https://doi.org/10.1038/s41586-024-00000-x")
    );
    assert_eq!(
        json["stable_key"].as_str(),
        Some("doi:10.1038/s41586-024-00000-x")
    );
}

#[tokio::test]
async fn resolve_paper_rejects_garbage_with_typed_error() {
    let server = make_server_without_db();
    let error = err(server
        .resolve_paper(Parameters(ResolvePaperRequest {
            query: "not a paper".to_string(),
            run_id: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("expected"),
        "rejection names what was expected: {}",
        error.message
    );
}

#[tokio::test]
async fn resolve_paper_with_run_id_records_the_canonical_url() {
    // The stub pool has no OpenAlex provider, so the enrichment degrades
    // with a surfaced note — but the identity and the run-ledger recording
    // happen regardless: the deterministic floor, never blocked by the
    // enrichment tier.
    let server = make_server_with_research_db();
    let begun = parse(&ok(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "paper identity".to_string(),
        }))
        .await));
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    let json = parse(&ok(server
        .resolve_paper(Parameters(ResolvePaperRequest {
            query: "doi:10.1038/s41586-024-00000-x".to_string(),
            run_id: Some(run_id.clone()),
        }))
        .await));
    assert_eq!(json["identifier"]["kind"].as_str(), Some("doi"));
    assert_eq!(
        json["canonical_url"].as_str(),
        Some("https://doi.org/10.1038/s41586-024-00000-x")
    );
    // The enrichment outcome is surfaced either way — metadata or a note,
    // never silent.
    assert!(
        json["openalex"].is_object(),
        "openalex block present (metadata or degradation): {json}"
    );
    assert_eq!(
        json["run_ledger"]["recorded"].as_u64(),
        Some(1),
        "canonical URL recorded: {json}"
    );

    let manifest = parse(&ok(server
        .get_research_run(Parameters(GetResearchRunRequest { run_id }))
        .await));
    let sources = manifest["sources"].as_array().expect("sources");
    assert_eq!(sources.len(), 1);
    assert_eq!(
        sources[0]["url"].as_str(),
        Some("https://doi.org/10.1038/s41586-024-00000-x")
    );
    assert_eq!(sources[0]["recorded_by"].as_str(), Some("server"));
    assert_eq!(sources[0]["provider"].as_str(), Some("openalex"));
}

// ── Semantic duplication tier (Commit 6) ───────────────────────────────────

/// Stub inference port whose embed returns one fixed vector per input text —
/// every content-bearing artifact embeds parallel, so the semantic tier
/// clusters them all.
struct EmbeddingInferencePort;

impl InferencePort for EmbeddingInferencePort {
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

    fn embed<'a>(&'a self, _model: &str, texts: &[String]) -> hkask_types::EmbedFuture<'a> {
        // Collect before the async block so the future captures owned data
        // only — the borrowed `texts` cannot outlive the call.
        let vectors: Vec<Vec<f32>> = texts.iter().map(|_| vec![1.0_f32, 0.0]).collect();
        Box::pin(async move { Ok(vectors) })
    }
}

fn make_server_with_embedding(
    inference_port: Arc<dyn InferencePort>,
    embedding_model: Option<&str>,
) -> ResearchServer {
    ResearchServer::new(
        WebID::new(),
        Arc::new(NoCredentialsPool),
        Arc::new(ResponseCache::new(10, Duration::from_secs(60))),
        RateLimiter::new(10000, 60),
        None,
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        inference_port,
        None,
        embedding_model.map(str::to_string),
    )
}

fn semantic_request(duplication: Option<&str>) -> EvaluateEvidenceRequest {
    EvaluateEvidenceRequest {
        question: "is it duplicated?".to_string(),
        artifacts: vec![
            evidence_artifact(
                "https://a.example/1",
                Some("a.example"),
                None,
                Some("completely different words entirely here now"),
            ),
            evidence_artifact(
                "https://b.example/1",
                Some("b.example"),
                None,
                Some("totally unlike those other ones entirely"),
            ),
        ],
        duplication: duplication.map(str::to_string),
    }
}

#[tokio::test]
async fn evaluate_evidence_semantic_without_model_degrades_to_shingles_with_reason() {
    // No embedding model configured: the tier degrades to the deterministic
    // floor with a reason naming the setting — never silent, never an error.
    let server = make_server_with_embedding(Arc::new(EmbeddingInferencePort), None);
    let json = parse(&ok(server
        .evaluate_evidence(Parameters(semantic_request(Some("semantic"))))
        .await));
    assert_eq!(json["set"]["duplication_mode"].as_str(), Some("shingles"));
    let reason = json["set"]["duplication_reason"]
        .as_str()
        .expect("degradation reason surfaced");
    assert!(
        reason.contains("HKASK_EMBEDDING_MODEL"),
        "reason names the setting: {reason}"
    );
}

#[tokio::test]
async fn evaluate_evidence_semantic_with_embed_failure_degrades_with_reason() {
    // Model configured but the embed call fails: the floor runs, the
    // failure is surfaced.
    let server = make_server_with_embedding(Arc::new(FailingInferencePort), Some("test-embed"));
    let json = parse(&ok(server
        .evaluate_evidence(Parameters(semantic_request(Some("semantic"))))
        .await));
    assert_eq!(json["set"]["duplication_mode"].as_str(), Some("shingles"));
    let reason = json["set"]["duplication_reason"]
        .as_str()
        .expect("degradation reason surfaced");
    assert!(
        reason.contains("embedding tier failed"),
        "reason names the failure: {reason}"
    );
}

#[tokio::test]
async fn evaluate_evidence_semantic_success_clusters_by_cosine() {
    // Model configured, embed succeeds: the semantic tier clusters the two
    // paraphrased artifacts the shingle floor cannot (different words,
    // parallel vectors) — one unit, one cluster, mode "semantic", no reason.
    let server = make_server_with_embedding(Arc::new(EmbeddingInferencePort), Some("test-embed"));
    let json = parse(&ok(server
        .evaluate_evidence(Parameters(semantic_request(Some("semantic"))))
        .await));
    assert_eq!(json["set"]["duplication_mode"].as_str(), Some("semantic"));
    assert!(
        json["set"].get("duplication_reason").is_none(),
        "no degradation: {json}"
    );
    let artifacts = json["artifacts"].as_array().expect("artifacts");
    assert_eq!(artifacts[0]["corroboration_count"].as_u64(), Some(1));
    let clusters = json["set"]["content_clusters"]
        .as_array()
        .expect("clusters");
    assert_eq!(clusters.len(), 1);
}

#[tokio::test]
async fn evaluate_evidence_rejects_unknown_duplication_mode() {
    let server = make_server_with_embedding(Arc::new(EmbeddingInferencePort), None);
    let error = err(server
        .evaluate_evidence(Parameters(semantic_request(Some("fuzzy"))))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("duplication"),
        "message names the field: {}",
        error.message
    );
}
