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
use hkask_mcp_research::research::providers::{
    CrossrefCandidate, ProviderSearchOutput, WebSearchPort,
};
use hkask_mcp_research::research::rss_types::{
    DiscoverRequest, GetEntriesRequest, ListSubscriptionsRequest, MarkReadRequest,
    UnreadCountRequest, UnsubscribeRequest,
};
use hkask_mcp_research::research::types::{
    AnnotateResearchRunRequest, BeginResearchRunRequest, BrowseRequest, BrowseResult,
    CompoundSearchResult, EvaluateArtifact, EvaluateEvidenceRequest, ExtractOptions,
    ExtractRequest, ExtractedContent, FindSimilarRequest, FinishResearchRunRequest,
    GetResearchRunRequest, LatencyTier, ProviderFailureRecord, ProviderHealthEntry, ProviderInfo,
    ProviderRecommendation, RankedResult, RateLimiter, ResolvePaperRequest, SearchQuery,
    SearchRequest, SearchStrategy, WebError,
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

/// The credential-missing arm every stub surface shares: a stub without a
/// provider returns the structured `NoProviderConfigured` error — the
/// mapping the tool-behavior tests pin (a missing credential surfaces as
/// a structured error, never an empty result or silent fallback).
macro_rules! no_provider {
    ($msg:literal) => {
        Err(WebError::NoProviderConfigured($msg.to_string()))
    };
}

/// Generate the `WebSearchPort` impl for a test stub pool. The trait's
/// method signatures, the `provider_fingerprint` wrapping, and the
/// per-stub bodies are single-sourced here: each invocation supplies only
/// the bodies that make its stub distinct. `resolve_title` is emitted only
/// when supplied — otherwise the stub keeps the trait default
/// (`WebError::NoProvider`).
///
/// `search` and `score_providers` take their parameter names from the
/// invocation — macro hygiene: a body pasted from the call site can only
/// reference bindings the call site named, so a body that uses the query
/// or the provider passes the name it wants to bind. The other methods'
/// signatures are fixed because no stub body references their parameters.
macro_rules! stub_web_search_port {
    (
        $name:ident, $fingerprint:literal,
        search($self:ident, $query:ident, $strategy:ident, $provider:ident): $search:block,
        find_similar: $find_similar:block,
        extract: $extract:block,
        browse: $browse:block,
        health: $health:block,
        kinds: $kinds:block,
        score($score_self:ident, $score_query:ident, $score_intent:ident): $score:block
        $(, resolve_title: $resolve:block)?
        $(,)?
    ) => {
        #[async_trait]
        impl WebSearchPort for $name {
            async fn search(
                &$self,
                $query: &SearchQuery,
                $strategy: SearchStrategy,
                $provider: Option<&str>,
            ) -> Result<CompoundSearchResult, WebError> $search

            async fn find_similar(
                &self,
                _url: &str,
                _num_results: u32,
            ) -> Result<ProviderSearchOutput, WebError> $find_similar

            async fn extract(
                &self,
                _url: &str,
                _opts: &ExtractOptions,
            ) -> Result<ExtractedContent, WebError> $extract

            async fn browse(
                &self,
                _url: &str,
                _instruction: &str,
                _timeout: Duration,
            ) -> Result<BrowseResult, WebError> $browse

            async fn health_check(&self) -> Vec<ProviderHealthEntry> $health

            fn provider_fingerprint(&self) -> String {
                $fingerprint.to_string()
            }

            fn provider_kinds(&self) -> Vec<String> $kinds

            fn score_providers(
                &$score_self,
                $score_query: &str,
                $score_intent: Option<&str>,
            ) -> Vec<ProviderRecommendation> $score

            $(async fn resolve_title(
                &self,
                _title: &str,
                _rows: u32,
            ) -> Result<Vec<CrossrefCandidate>, WebError> $resolve)?
        }
    };
}

/// Stub that simulates "no credentials configured" for all HTTP provider
/// calls. The tool handler maps `WebError::NoProviderConfigured` to
/// `McpToolError::permission_denied` — the test asserts that mapping,
/// which pins the broken-feedback-loop rule: a missing credential must
/// surface as a structured error, not an empty result or silent no-op.
struct NoCredentialsPool;

stub_web_search_port! {
    NoCredentialsPool, "stub-no-credentials",
    search(self, _query, _strategy, _provider): {
        no_provider!(
            "No search provider configured. Set HKASK_BRAVE_API_KEY or HKASK_TAVILY_API_KEY."
        )
    },
    find_similar: {
        no_provider!("Exa provider not configured. Set HKASK_EXA_API_KEY.")
    },
    extract: {
        no_provider!("No extract provider configured.")
    },
    browse: {
        no_provider!("No browse provider configured. Set HKASK_FIRECRAWL_API_KEY.")
    },
    health: {
        vec![ProviderHealthEntry {
            kind: "stub".to_string(),
            surface: "search".to_string(),
            healthy: true,
            error: None,
        }]
    },
    kinds: { Vec::new() },
    score(self, _query, _intent): { Vec::new() }
}

/// Stub whose `extract` returns a near-empty body — the JS-shell / bot-block
/// shape observed on Cloudflare-gated publisher pages (2026-09-30 zk-reference
/// lesson L1). Pins the degradation contract: a successful HTTP fetch with a
/// near-empty body surfaces a note naming the likely cause, never a bare
/// success, and is not cached.
struct ChromeShellPool;

stub_web_search_port! {
    ChromeShellPool, "stub-chrome-shell",
    search(self, _query, _strategy, _provider): { no_provider!("No search provider configured.") },
    find_similar: {
        no_provider!("Exa provider not configured. Set HKASK_EXA_API_KEY.")
    },
    extract: {
        Ok(ExtractedContent {
            url: "https://example.com/js-shell".to_string(),
            content: "[Skip to content] × Copy link ✓".to_string(),
            format: "markdown".to_string(),
            metadata: Some(serde_json::json!({"title": "A JS shell", "statusCode": 200})),
        })
    },
    browse: { no_provider!("No browse provider configured.") },
    health: { Vec::new() },
    kinds: { Vec::new() },
    score(self, _query, _intent): { Vec::new() }
}

/// Stub whose extraction returns a thin body with a non-2xx fetch status —
/// pins P2 (2026-10-01): the degradation note names the observed status
/// (a gone/moved URL) instead of guessing the JS-shell cause list.
struct GoneOriginPool;

stub_web_search_port! {
    GoneOriginPool, "stub-gone-origin",
    search(self, _query, _strategy, _provider): { no_provider!("No search provider configured.") },
    find_similar: {
        no_provider!("Exa provider not configured. Set HKASK_EXA_API_KEY.")
    },
    extract: {
        Ok(ExtractedContent {
            url: "https://example.com/gone".to_string(),
            content: "# 404 Not Found\n\n* * *\n\nnginx/1.24.0".to_string(),
            format: "markdown".to_string(),
            metadata: Some(serde_json::json!({"statusCode": 404, "error": "Not Found"})),
        })
    },
    browse: { no_provider!("No browse provider configured.") },
    health: { Vec::new() },
    kinds: { Vec::new() },
    score(self, _query, _intent): { Vec::new() }
}

/// One Crossref candidate for a `resolve_title` stub body — the repeated
/// construction collapses to one line per candidate.
fn crossref_candidate(
    doi: &str,
    title: &str,
    publication_year: u64,
    venue: &str,
    first_author: Option<&str>,
) -> CrossrefCandidate {
    CrossrefCandidate {
        doi: doi.to_string(),
        title: title.to_string(),
        publication_year: Some(publication_year),
        venue: Some(venue.to_string()),
        first_author: first_author.map(str::to_string),
    }
}

/// Stub whose `resolve_title` returns fixed Crossref candidates — pins the
/// bibliographic title mode (2026-09-30 zk-reference lesson L4): candidates
/// are surfaced in full, the top candidate resolves to the typed identity.
struct TitleResolvePool;

stub_web_search_port! {
    TitleResolvePool, "stub-title-resolve",
    search(self, _query, _strategy, _provider): { no_provider!("No search provider configured.") },
    find_similar: {
        no_provider!("Exa provider not configured. Set HKASK_EXA_API_KEY.")
    },
    extract: {
        no_provider!("No extract provider configured.")
    },
    browse: { no_provider!("No browse provider configured.") },
    health: { Vec::new() },
    kinds: { Vec::new() },
    score(self, _query, _intent): { Vec::new() },
    resolve_title: {
        Ok(vec![
            crossref_candidate(
                "10.18653/v1/2022.acl-short.94",
                "A Recipe For Arbitrary Text Style Transfer with Large Language Models",
                2022,
                "ACL 2022",
                Some("Reif"),
            ),
            crossref_candidate(
                "10.18653/v1/2022.acl-long.285",
                "Zero-Shot Cross-lingual Semantic Parsing",
                2022,
                "ACL 2022",
                Some("Sherborne"),
            ),
        ])
    }
}

/// Stub whose `resolve_title` returns the live-observed superstring-first
/// candidate list — pins P1 (2026-10-01): exact-title matches rank first,
/// stable within groups.
struct SuperstringFirstTitlePool;

stub_web_search_port! {
    SuperstringFirstTitlePool, "stub-superstring-first",
    search(self, _query, _strategy, _provider): { no_provider!("No search provider configured.") },
    find_similar: {
        no_provider!("Exa provider not configured. Set HKASK_EXA_API_KEY.")
    },
    extract: {
        no_provider!("No extract provider configured.")
    },
    browse: { no_provider!("No browse provider configured.") },
    health: { Vec::new() },
    kinds: { Vec::new() },
    score(self, _query, _intent): { Vec::new() },
    resolve_title: {
        // The live 2026-10-01 observation: Crossref's relevance put the
        // superstring "Computing Machinery and Intelligence Amplification"
        // first, with Turing's exact-titled work (in two case variants)
        // behind it.
        Ok(vec![
            crossref_candidate(
                "10.1109/9780470544297.ch3",
                "Computing Machinery and Intelligence Amplification",
                2009,
                "Computational Intelligence",
                None,
            ),
            crossref_candidate(
                "10.7551/mitpress/4626.003.0002",
                "Computing Machinery and Intelligence",
                1997,
                "Mind Design II",
                Some("Turing"),
            ),
            crossref_candidate(
                "10.1016/b978-1-4832-1446-7.50006-6",
                "COMPUTING MACHINERY AND INTELLIGENCE",
                1988,
                "Readings in Cognitive Science",
                Some("TURING"),
            ),
        ])
    }
}

/// Generate the `InferencePort` impl for a test stub. `generate` is the
/// surface these tests never exercise — one structured `Connection` error
/// carrying the stub's own message, single-sourced here. `rerank` (and
/// the optional `embed`) is the surface under test and is supplied per
/// stub, with its parameter names taken from the invocation (macro
/// hygiene — see `stub_web_search_port`); omitting `embed` keeps the
/// trait default.
macro_rules! stub_inference_port {
    (
        $name:ident,
        generate_msg: $generate_msg:literal,
        rerank($self:ident, $model:ident, $query:ident, $documents:ident): $rerank:block
        $(, embed($embed_self:ident, $embed_model:ident, $embed_texts:ident): $embed:block)?
        $(,)?
    ) => {
        impl InferencePort for $name {
            fn generate(
                &self,
                _prompt: &str,
                _parameters: &hkask_types::template::LLMParameters,
                _tools: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<InferenceResult, InferenceError>>
                        + Send
                        + '_,
                >,
            > {
                Box::pin(async {
                    Err(InferenceError::Connection($generate_msg.to_string()))
                })
            }

            fn rerank<'a>(
                &'a $self,
                $model: &str,
                $query: &str,
                $documents: &[String],
            ) -> hkask_types::RerankFuture<'a> $rerank

            $(fn embed<'a>(
                &'a $embed_self,
                $embed_model: &str,
                $embed_texts: &[String],
            ) -> hkask_types::EmbedFuture<'a> $embed)?
        }
    };
}

/// Stub inference port that always fails — pins the degradation contract:
/// the deep strategy must surface the failure reason, never collapse it.
struct FailingInferencePort;

stub_inference_port! {
    FailingInferencePort,
    generate_msg: "stub: inference bridge down",
    rerank(self, _model, _query, _documents): {
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

stub_inference_port! {
    ScoringInferencePort,
    generate_msg: "stub: generate unused in rerank tests",
    rerank(self, _model, _query, _documents): {
        let scores: Vec<hkask_types::inference_ipc::RerankScoreEntry> = _documents
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

/// One constructor for every test server: the shared scaffolding —
/// identity, cache, limiter, HTTP clients, and the default failing
/// inference port — lives here once; each call site names only the parts
/// its test exercises.
fn test_server(
    pool: Arc<dyn WebSearchPort>,
    cache_capacity: usize,
    database: Option<r2d2::Pool<hkask_storage::SqliteConnectionManager>>,
    inference_port: Option<Arc<dyn InferencePort>>,
    rerank_model: Option<&str>,
    embedding_model: Option<&str>,
) -> ResearchServer {
    ResearchServer::new(
        WebID::new(),
        pool,
        Arc::new(ResponseCache::new(cache_capacity, Duration::from_secs(60))),
        RateLimiter::new(10000, 60),
        database,
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        reqwest::Client::builder()
            .build()
            .expect("reqwest client build"),
        inference_port.unwrap_or_else(|| Arc::new(FailingInferencePort)),
        rerank_model.map(str::to_string),
        embedding_model.map(str::to_string),
    )
}

fn make_server_without_db() -> ResearchServer {
    test_server(Arc::new(NoCredentialsPool), 10, None, None, None, None)
}

fn make_server_with_pool(pool: Arc<dyn WebSearchPort>) -> ResearchServer {
    test_server(pool, 10, None, None, None, None)
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
    test_server(
        Arc::new(NoCredentialsPool),
        10,
        Some(research_db_pool()),
        None,
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

stub_web_search_port! {
    FixedResultsPool, "stub-fixed-results",
    search(self, _query, _strategy, _provider): {
        let result = |title: &str, url: &str| RankedResult {
            title: title.to_string(),
            url: url.to_string(),
            description: None,
            source: None,
            oa_pdf_url: None,
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
            query: _query.query.clone(),
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
    },
    find_similar: { no_provider!("stub") },
    extract: { no_provider!("stub") },
    browse: { no_provider!("stub") },
    health: { Vec::new() },
    kinds: { Vec::new() },
    score(self, _query, _intent): { Vec::new() }
}

fn make_server_with_pool_and_port(
    pool: Arc<dyn WebSearchPort>,
    inference_port: Arc<dyn InferencePort>,
    rerank_model: Option<&str>,
) -> ResearchServer {
    test_server(pool, 10, None, Some(inference_port), rerank_model, None)
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

/// The run-scoped stub search every run-ledger test issues — one request
/// shape, varying only the strategy and the run it belongs to.
fn run_scoped_stub_search(strategy: Option<&str>, run_id: &str) -> SearchRequest {
    SearchRequest {
        query: "stub query".to_string(),
        num_results: Some(10),
        include_domains: None,
        exclude_domains: None,
        freshness: None,
        strategy: strategy.map(|s| s.to_string()),
        intent: None,
        provider: None,
        run_id: Some(run_id.to_string()),
    }
}

/// expect: A provider that ignores the requested domain filters cannot return
/// off-domain search hits to the primary-disclosure consumer.
#[tokio::test]
async fn web_search_enforces_domain_filters_after_provider_results() {
    let server = make_server_with_pool_and_port(
        Arc::new(FixedResultsPool),
        Arc::new(FailingInferencePort),
        None,
    );
    let mut request = deep_search_request();
    request.strategy = Some("quick".to_string());
    request.include_domains = Some(vec!["amf-france.org".to_string()]);
    let blocked = parse(&ok(server.web_search(Parameters(request)).await));
    assert_eq!(blocked["count"], 0);
    assert_eq!(blocked["results"], serde_json::json!([]));

    let mut request = deep_search_request();
    request.strategy = Some("quick".to_string());
    request.include_domains = Some(vec!["example.com".to_string()]);
    let included = parse(&ok(server.web_search(Parameters(request)).await));
    assert_eq!(included["count"], 3);

    let mut request = deep_search_request();
    request.strategy = Some("quick".to_string());
    request.exclude_domains = Some(vec!["example.com".to_string()]);
    let excluded = parse(&ok(server.web_search(Parameters(request)).await));
    assert_eq!(excluded["count"], 0);
    assert_eq!(excluded["results"], serde_json::json!([]));
}

/// expect: A run-scoped official-domain search never records provider hits
/// excluded from the response as server-observed primary sources.
#[tokio::test]
async fn filtered_search_does_not_ledger_off_domain_hits() {
    let server = make_server_with_pool_and_db(Arc::new(FixedResultsPool), Some(research_db_pool()));
    let begun = parse(&ok(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "official-domain boundary".to_string(),
        }))
        .await));
    let run_id = begun["run_id"].as_str().expect("run id").to_string();
    let mut request = deep_search_request();
    request.strategy = Some("quick".to_string());
    request.include_domains = Some(vec!["amf-france.org".to_string()]);
    request.run_id = Some(run_id.clone());
    let output = parse(&ok(server.web_search(Parameters(request)).await));
    assert_eq!(output["count"], 0);
    assert_eq!(output["domain_filter_removed"], 3);
    assert_eq!(output["run_ledger"]["recorded"], 0);
    let manifest = parse(&ok(server
        .get_research_run(Parameters(GetResearchRunRequest { run_id }))
        .await));
    assert_eq!(manifest["sources"], serde_json::json!([]));
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

stub_web_search_port! {
    FailThenSucceedPool, "stub-fail-then-succeed",
    search(self, _query, _strategy, _provider): {
        let mut failed_once = self
            .failed_once
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if *failed_once {
            return Ok(CompoundSearchResult {
                query: _query.query.clone(),
                strategy: "quick".to_string(),
                results: vec![RankedResult {
                    title: "Recovered".to_string(),
                    url: "https://example.com/recovered".to_string(),
                    description: None,
                    oa_pdf_url: None,
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
            query: _query.query.clone(),
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
    },
    find_similar: { no_provider!("stub") },
    extract: { no_provider!("stub") },
    browse: { no_provider!("stub") },
    health: { Vec::new() },
    kinds: { Vec::new() },
    score(self, _query, _intent): { Vec::new() }
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

stub_web_search_port! {
    IntentSelectionPool, "intent-selection-stub",
    search(self, _query, _strategy, _provider): {
        self.selected_provider
            .lock()
            .expect("selected provider lock")
            .push(_provider.map(str::to_string));
        Ok(CompoundSearchResult {
            query: _query.query.clone(),
            strategy: "quick".to_string(),
            results: Vec::new(),
            providers_queried: vec![ProviderInfo {
                kind: _provider.unwrap_or("default").to_string(),
                capabilities: Vec::new(),
            }],
            providers_succeeded: vec![_provider.unwrap_or("default").to_string()],
            providers_failed: Vec::new(),
            answer_box: None,
            related_questions: Vec::new(),
            total_before_dedup: 0,
            duplicates_removed: 0,
        })
    },
    find_similar: { no_provider!("not configured") },
    extract: { no_provider!("not configured") },
    browse: { no_provider!("not configured") },
    health: { Vec::new() },
    kinds: { vec!["arxiv".to_string()] },
    score(self, _query, _intent): {
        vec![
            ProviderRecommendation {
                kind: "arxiv".to_string(),
                score: 1.0,
                rationale: format!("best for {} intent", _intent.unwrap_or("general")),
                cost_per_call_usd: 0.0,
                latency_tier: LatencyTier::Fast,
                strengths: Vec::new(),
                weaknesses: Vec::new(),
                best_for: Vec::new(),
                configured: true,
                live_success_rate: None,
                live_p50_latency_ms: None,
                live_sample_count: None,
                live_stats_degraded: false,
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
                live_stats_degraded: false,
            },
        ]
    }
}

#[tokio::test]
async fn web_search_intent_selects_top_configured_provider_and_surfaces_ranking() {
    let server = test_server(
        Arc::new(IntentSelectionPool {
            selected_provider: std::sync::Mutex::new(Vec::new()),
        }),
        0,
        None,
        None,
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
    test_server(pool, 10, database, None, None, None)
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
        .web_search(Parameters(run_scoped_stub_search(Some("deep"), &run_id)))
        .await));
    assert_eq!(
        output["run_ledger"]["recorded"].as_u64(),
        Some(3),
        "three stub results recorded: {output}"
    );

    assert_eq!(output["run_ledger"]["already_recorded"].as_u64(), Some(0));

    // A repeat call in the same run keeps the first observation: the note
    // names the held rows instead of reporting a bare zero.
    let repeat = parse(&ok(server
        .web_search(Parameters(run_scoped_stub_search(Some("deep"), &run_id)))
        .await));
    assert_eq!(
        repeat["run_ledger"]["recorded"].as_u64(),
        Some(0),
        "{repeat}"
    );
    assert_eq!(
        repeat["run_ledger"]["already_recorded"].as_u64(),
        Some(3),
        "{repeat}"
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
        .web_search(Parameters(run_scoped_stub_search(None, "deadbeefdeadbeef")))
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
        .web_search(Parameters(run_scoped_stub_search(None, &run_id)))
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
        .web_search(Parameters(run_scoped_stub_search(None, &run_id)))
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
            query: Some("https://doi.org/10.1038/s41586-024-00000-x".to_string()),
            title: None,
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
            query: Some("not a paper".to_string()),
            title: None,
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
            query: Some("doi:10.1038/s41586-024-00000-x".to_string()),
            title: None,
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

#[tokio::test]
async fn resolve_paper_title_mode_surfaces_candidates_and_resolves_the_top() {
    // Bibliographic mode (2026-09-30 zk-reference lesson L4): a title query
    // resolves via Crossref candidates — every candidate is surfaced so the
    // caller can verify the match (a title search can hit a different work
    // than intended), and the top candidate resolves to the typed identity.
    let server = make_server_with_pool(Arc::new(TitleResolvePool));
    let json = parse(&ok(server
        .resolve_paper(Parameters(ResolvePaperRequest {
            query: None,
            title: Some(
                "A Recipe For Arbitrary Text Style Transfer with Large Language Models".to_string(),
            ),
            run_id: None,
        }))
        .await));
    assert_eq!(json["mode"].as_str(), Some("bibliographic"));
    assert_eq!(json["identifier"]["kind"].as_str(), Some("doi"));
    assert_eq!(
        json["identifier"]["value"].as_str(),
        Some("10.18653/v1/2022.acl-short.94")
    );
    let candidates = json["candidates"].as_array().expect("candidates surfaced");
    assert_eq!(candidates.len(), 2, "every candidate surfaced: {json}");
    assert_eq!(
        candidates[0]["title"].as_str(),
        Some("A Recipe For Arbitrary Text Style Transfer with Large Language Models")
    );
    assert_eq!(
        candidates[1]["doi"].as_str(),
        Some("10.18653/v1/2022.acl-long.285")
    );
    // The openalex block degrades with a surfaced note on the stub pool —
    // the identity is the deterministic floor.
    assert!(
        json["openalex"].is_object(),
        "openalex block present: {json}"
    );
}

#[tokio::test]
async fn resolve_paper_title_mode_ranks_exact_title_matches_first() {
    // P1 (2026-10-01): Crossref's relevance put the superstring
    // "Computing Machinery and Intelligence Amplification" above
    // Turing's exact-titled work (observed live); the exact-first sort
    // promotes the exact matches — stable within the group (the 1997
    // reprint stays ahead of the 1988 one, and the uppercase variant
    // exercises the normalization) — and the top candidate's resolution
    // follows the ranking.
    let server = make_server_with_pool(Arc::new(SuperstringFirstTitlePool));
    let json = parse(&ok(server
        .resolve_paper(Parameters(ResolvePaperRequest {
            query: None,
            title: Some("Computing Machinery and Intelligence".to_string()),
            run_id: None,
        }))
        .await));
    let candidates = json["candidates"].as_array().expect("candidates surfaced");
    assert_eq!(candidates.len(), 3, "every candidate surfaced: {json}");
    assert_eq!(
        candidates[0]["doi"].as_str(),
        Some("10.7551/mitpress/4626.003.0002"),
        "exact match ranks first: {json}"
    );
    assert_eq!(
        candidates[1]["doi"].as_str(),
        Some("10.1016/b978-1-4832-1446-7.50006-6"),
        "case-normalized exact match ranks second (stable within the group): {json}"
    );
    assert_eq!(
        candidates[2]["doi"].as_str(),
        Some("10.1109/9780470544297.ch3"),
        "the superstring title ranks last: {json}"
    );
    assert_eq!(
        json["identifier"]["value"].as_str(),
        Some("10.7551/mitpress/4626.003.0002"),
        "the top candidate's resolution follows the exact-first ranking: {json}"
    );
}

#[tokio::test]
async fn resolve_paper_title_mode_rejects_both_and_neither_with_typed_errors() {
    let server = make_server_without_db();
    // Both → ambiguous input, rejected with what was expected.
    let error = err(server
        .resolve_paper(Parameters(ResolvePaperRequest {
            query: Some("doi:10.1/x".to_string()),
            title: Some("A Title".to_string()),
            run_id: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("either query"),
        "rejection names the contract: {}",
        error.message
    );
    // Neither → empty input, rejected with what was expected.
    let error = err(server
        .resolve_paper(Parameters(ResolvePaperRequest {
            query: None,
            title: None,
            run_id: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("either query"),
        "rejection names the contract: {}",
        error.message
    );
}

#[tokio::test]
async fn web_extract_surfaces_near_empty_body_as_a_degradation_note() {
    // L1 (2026-09-30 zk-reference lesson): a successful HTTP fetch whose
    // captured body is near-empty (a JS shell / bot-block page) surfaces a
    // note naming the likely cause and the alternative route — never a
    // bare success. The degraded extraction is not cached: a second call
    // re-fetches instead of replaying the shell.
    let server = make_server_with_pool(Arc::new(ChromeShellPool));
    let json = parse(&ok(server
        .web_extract(Parameters(ExtractRequest {
            url: "https://example.com/js-shell".to_string(),
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
            run_id: None,
        }))
        .await));
    let note = json["note"].as_str().expect("degradation note present");
    assert!(
        note.contains("JS shell") && note.contains("Wayback"),
        "note names the likely cause and the alternative route: {note}"
    );
    // The degraded extraction is not cached: the second call re-fetches
    // (the stub returns the same shell, but through a fresh fetch — the
    // cache would have replayed the first response unchanged, which the
    // note's presence on the second call also demonstrates).
    let json2 = parse(&ok(server
        .web_extract(Parameters(ExtractRequest {
            url: "https://example.com/js-shell".to_string(),
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
            run_id: None,
        }))
        .await));
    assert!(
        json2["note"].is_string(),
        "degraded extraction not cached as a clean hit: {json2}"
    );
}

#[tokio::test]
async fn web_extract_degradation_note_names_a_non_2xx_origin_status() {
    // P2 (2026-10-01): a thin body from a non-2xx origin names the
    // observed status in the note — a 404's cause is a gone/moved URL,
    // not a JS shell (observed live: the httpstat.us probe surfaced the
    // JS-shell cause list while the metadata held statusCode 404). The
    // ChromeShellPool test above pins the boundary: a 2xx status keeps
    // the JS-shell cause list.
    let server = make_server_with_pool(Arc::new(GoneOriginPool));
    let json = parse(&ok(server
        .web_extract(Parameters(ExtractRequest {
            url: "https://example.com/gone".to_string(),
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
            run_id: None,
        }))
        .await));
    let note = json["note"].as_str().expect("degradation note present");
    assert!(
        note.contains("HTTP 404") && note.contains("gone or moved"),
        "note names the observed status and its cause: {note}"
    );
    assert!(
        note.contains("Wayback"),
        "the alternative route stays named: {note}"
    );
    assert!(
        !note.contains("JS shell"),
        "the JS-shell cause list is not guessed when the status is known: {note}"
    );
}

#[tokio::test]
async fn web_ping_carries_the_static_provider_profiles() {
    // L5 (2026-09-30 zk-reference lesson): the static profile table moved
    // from every web_search response (~1KB of repeated context per call) to
    // web_ping — the per-server-version metacognitive lookup lives where a
    // health-check consumer reads it.
    let server = make_server_without_db();
    let json = parse(&ok(server.web_ping().await));
    assert!(
        json["provider_profiles"].is_array(),
        "ping carries the profile table (possibly empty on a stub pool): {json}"
    );
}

// ── Semantic duplication tier (Commit 6) ───────────────────────────────────

/// Stub inference port whose embed returns one fixed vector per input text —
/// every content-bearing artifact embeds parallel, so the semantic tier
/// clusters them all.
struct EmbeddingInferencePort;

stub_inference_port! {
    EmbeddingInferencePort,
    generate_msg: "stub: inference bridge down",
    rerank(self, _model, _query, _documents): {
        Box::pin(async {
            Err(InferenceError::Connection(
                "stub: rerank bridge down".to_string(),
            ))
        })
    },
    embed(self, _model, _texts): {
        // Collect before the async block so the future captures owned data
        // only — the borrowed `texts` cannot outlive the call.
        let vectors: Vec<Vec<f32>> = _texts.iter().map(|_| vec![1.0_f32, 0.0]).collect();
        Box::pin(async move { Ok(vectors) })
    }
}

fn make_server_with_embedding(
    inference_port: Arc<dyn InferencePort>,
    embedding_model: Option<&str>,
) -> ResearchServer {
    test_server(
        Arc::new(NoCredentialsPool),
        10,
        None,
        Some(inference_port),
        None,
        embedding_model,
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

#[tokio::test]
async fn finish_research_run_transitions_and_preserves_the_sources_ledger() {
    let server = make_server_with_research_db();
    let begun = parse(&ok(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "is the wire story corroborated?".to_string(),
        }))
        .await));
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    // completed/partial require a server-recorded source — a fresh run has none.
    let refused = err(server
        .finish_research_run(Parameters(FinishResearchRunRequest {
            run_id: run_id.clone(),
            status: "completed".to_string(),
            note: None,
        }))
        .await);
    assert_error_kind(&refused, McpErrorKind::FailedPrecondition);
    assert!(
        refused.message.contains("server-recorded source"),
        "message names the rule: {}",
        refused.message
    );

    // blocked is a legitimate finish without sources.
    let finished = parse(&ok(server
        .finish_research_run(Parameters(FinishResearchRunRequest {
            run_id: run_id.clone(),
            status: "blocked".to_string(),
            note: Some("no sources reachable".to_string()),
        }))
        .await));
    assert_eq!(finished["status"].as_str(), Some("blocked"));
    assert_eq!(finished["from_status"].as_str(), Some("planned"));
    assert_eq!(finished["note"].as_str(), Some("no sources reachable"));

    // The manifest reflects the terminal status AND its transition history;
    // the sources ledger is untouched.
    let manifest = parse(&ok(server
        .get_research_run(Parameters(GetResearchRunRequest {
            run_id: run_id.clone(),
        }))
        .await));
    assert_eq!(manifest["status"].as_str(), Some("blocked"));
    let history = manifest["status_history"]
        .as_array()
        .expect("status_history present");
    assert_eq!(history.len(), 1, "one journaled transition: {manifest}");
    assert_eq!(history[0]["from_status"].as_str(), Some("planned"));
    assert_eq!(history[0]["to_status"].as_str(), Some("blocked"));
    assert_eq!(history[0]["note"].as_str(), Some("no sources reachable"));
    assert!(
        manifest["sources"].as_array().is_some_and(|s| s.is_empty()),
        "finishing never appends or mutates sources: {manifest}"
    );
}

#[tokio::test]
async fn finish_research_run_rejects_unknown_run_and_nonterminal_status() {
    let server = make_server_with_research_db();
    let missing = err(server
        .finish_research_run(Parameters(FinishResearchRunRequest {
            run_id: "deadbeefdeadbeef".to_string(),
            status: "completed".to_string(),
            note: None,
        }))
        .await);
    assert_error_kind(&missing, McpErrorKind::NotFound);

    let begun = parse(&ok(server
        .begin_research_run(Parameters(BeginResearchRunRequest {
            question: "another question".to_string(),
        }))
        .await));
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();
    let nonterminal = err(server
        .finish_research_run(Parameters(FinishResearchRunRequest {
            run_id,
            status: "running".to_string(),
            note: None,
        }))
        .await);
    assert_error_kind(&nonterminal, McpErrorKind::InvalidArgument);
    assert!(
        nonterminal
            .message
            .contains("completed|partial|blocked|failed"),
        "message names the terminal vocabulary: {}",
        nonterminal.message
    );
}
