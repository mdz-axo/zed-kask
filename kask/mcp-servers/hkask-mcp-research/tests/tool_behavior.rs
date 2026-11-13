//! Tool-behavior contract tests for hkask-mcp-research.
//!
//! Drives the real `Parameters<T>` seam on `ResearchServer`. Covers:
//! - `web_ping` happy path (in-process, stub pool).
//! - `web_search` invalid-argument paths (empty query, oversized query,
//!   unknown strategy, unknown freshness) — checked before any HTTP call.
//! - `web_search` / `web_find_similar` / `web_extract`
//!   credential-missing paths — a stub `WebSearchPort` returns
//!   `NoProviderConfigured`, which maps to `permission_denied`. This pins the
//!   `.rules` rule: a missing credential must surface as a structured error,
//!   not a silent fallback or empty result.
//! - `web_extract` invalid-argument paths (oversized URL, oversized
//!   json_prompt) — checked before URL validation.
//! - Research-run tools without a DB → `permission_denied` (the
//!   `require_research_db!` gate), and their ledger paths on an in-memory DB.
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

use hkask_mcp_research::research::types::{
    AnnotateResearchRunRequest, BeginResearchRunRequest, CompoundSearchResult, EvaluateArtifact,
    EvaluateEvidenceRequest, ExtractOptions, ExtractRequest, ExtractedContent, FindSimilarRequest,
    FinishResearchRunRequest, GetResearchRunRequest, ProviderFailureRecord, ProviderHealthEntry,
    ProviderInfo, ProviderRecommendation, ProviderSkipRecord, RankedResult, RateLimiter,
    ResolvePaperRequest, SearchQuery, SearchRequest, SearchStrategy, WebError,
};
use hkask_mcp_server::server::McpToolError;
use hkask_mcp_test_support::{invoke_tool, parse_envelope};
use hkask_types::InferenceError;
use hkask_types::InferencePort;
use hkask_types::InferenceResult;
use hkask_types::McpErrorKind;
use hkask_types::WebID;
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

/// Generate a `stub_web_search_port!` invocation whose non-distinct arms are
/// the shared defaults: every provider method answers `no_provider!` with
/// its realistic not-configured message, and health/kinds/score answer empty.
/// The invocation supplies only what makes its stub distinct — an optional
/// custom `search` body (default: the not-configured message) and an optional
/// `resolve_title` body (default: the trait's). Stubs customizing other arms
/// (`extract`, `kinds`, ...) use `stub_web_search_port!` directly.
macro_rules! defaults_pool {
    (
        $name:ident, $fingerprint:literal,
        search($self:ident, $query:ident, $strategy:ident, $provider:ident): $search:block
        $(, resolve_title: $resolve:block)?
        $(,)?
    ) => {
        stub_web_search_port! {
            $name, $fingerprint,
            search($self, $query, $strategy, $provider): $search,
            find_similar: {
                no_provider!("Exa provider not configured. Set HKASK_EXA_API_KEY.")
            },
            extract: { no_provider!("No extract provider configured.") },
            health: { Vec::new() },
            kinds: { Vec::new() },
            score(self, _query, _intent): { Vec::new() }
            $(, resolve_title: $resolve)?
        }
    };
    (
        $name:ident, $fingerprint:literal
        $(, resolve_title: $resolve:block)?
        $(,)?
    ) => {
        defaults_pool!($name, $fingerprint, search(self, _query, _strategy, _provider): {
            no_provider!("No search provider configured.")
        } $(, resolve_title: $resolve)?);
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

/// Stub whose registered kinds are exactly the free scholarly providers —
/// the production registration state without API keys. Pins C4b's web_ping
/// claim: the profile table lists the free providers now that they carry
/// static profiles.
struct FreeProvidersPool;

stub_web_search_port! {
    FreeProvidersPool, "stub-free-providers",
    search(self, _query, _strategy, _provider): {
        no_provider!("No paid provider configured.")
    },
    find_similar: { no_provider!("Exa provider not configured. Set HKASK_EXA_API_KEY.") },
    extract: { no_provider!("No extract provider configured.") },
    health: { Vec::new() },
    kinds: {
        vec![
            "openalex".to_string(),
            "arxiv".to_string(),
            "semantic_scholar".to_string(),
        ]
    },
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
    health: { Vec::new() },
    kinds: { Vec::new() },
    score(self, _query, _intent): { Vec::new() }
}

/// Stub whose extraction returns a short, valid structured answer in the
/// format it was asked for — the shape of a Firecrawl JSON-mode response.
struct ShortJsonPool;

// Written out by hand: the stub macro's method parameters are not visible
// to the bodies passed into it, and this stub must answer in the format
// it was asked for.
#[async_trait]
impl WebSearchPort for ShortJsonPool {
    async fn search(
        &self,
        _query: &SearchQuery,
        _strategy: SearchStrategy,
        _provider: Option<&str>,
    ) -> Result<CompoundSearchResult, WebError> {
        no_provider!("No search provider configured.")
    }
    async fn find_similar(
        &self,
        _url: &str,
        _num_results: u32,
    ) -> Result<ProviderSearchOutput, WebError> {
        no_provider!("Exa provider not configured. Set HKASK_EXA_API_KEY.")
    }
    async fn extract(
        &self,
        _url: &str,
        opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        Ok(ExtractedContent {
            url: "https://example.com/page".to_string(),
            content:
                r#"{"title":"Instructions and Guides","domains":["example.com","example.org"]}"#
                    .to_string(),
            format: opts.format.clone(),
            metadata: Some(serde_json::json!({"statusCode": 200})),
        })
    }
    async fn health_check(&self) -> Vec<ProviderHealthEntry> {
        Vec::new()
    }
    fn provider_fingerprint(&self) -> String {
        "stub-short-json".to_string()
    }
    fn provider_kinds(&self) -> Vec<String> {
        Vec::new()
    }
    fn score_providers(&self, _query: &str, _intent: Option<&str>) -> Vec<ProviderRecommendation> {
        Vec::new()
    }
}

/// Stub whose fusion result carries one breaker skip record — pins that a
/// skipped provider reaches the caller as its own metadata (skipped is NOT
/// failed: no request was made; the agent sees why a configured provider
/// is absent).
struct SkipRecordingPool;

stub_web_search_port! {
    SkipRecordingPool, "stub-skip-recording",
    search(self, _query, _strategy, _provider): {
        Ok(CompoundSearchResult {
            query: _query.query.clone(),
            strategy: "web".to_string(),
            results: Vec::new(),
            providers_queried: vec![ProviderInfo {
                kind: "brave".to_string(),
                capabilities: Vec::new(),
            }],
            providers_succeeded: vec!["brave".to_string()],
            providers_failed: Vec::new(),
            providers_skipped: vec![ProviderSkipRecord {
                kind: "semantic_scholar".to_string(),
                reason: "circuit breaker open: 3 consecutive failures, 45s cooldown remaining"
                    .to_string(),
            }],
            answer_box: None,
            related_questions: Vec::new(),
            total_before_dedup: 0,
            duplicates_removed: 0,
        })
    },
    find_similar: { no_provider!("not configured") },
    extract: { no_provider!("not configured") },
    health: { Vec::new() },
    kinds: { Vec::new() },
    score(self, _query, _intent): { Vec::new() }
}

/// expect: "A breaker skip record reaches the caller as
/// `providers_skipped[{kind, reason}]` — distinct from `providers_failed`",
/// so the agent can distinguish a provider that errored from one the
/// server deliberately did not query.
#[tokio::test]
async fn web_search_surfaces_provider_skips() {
    let server = make_server_with_pool(Arc::new(SkipRecordingPool));
    let request = SearchRequest {
        query: "skip metadata".to_string(),
        num_results: Some(5),
        include_domains: None,
        exclude_domains: None,
        freshness: None,
        strategy: Some("web".to_string()),
        intent: None,
        provider: None,
        run_id: None,
    };
    let json = invoke_tool!(server, web_search, request);
    let skips = json["providers_skipped"]
        .as_array()
        .expect("providers_skipped array");
    assert!(!skips.is_empty(), "skip records must reach the caller");
    assert_eq!(skips[0]["kind"].as_str(), Some("semantic_scholar"));
    assert!(
        skips[0]["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("circuit breaker open"),
        "reason names the breaker state"
    );
    assert!(
        json["providers_failed"]
            .as_array()
            .expect("failed array")
            .is_empty(),
        "a skip is not a failure"
    );
}

/// Stub whose extraction returns a thin page shell for a PDF URL — pins
/// the PDF-specific degradation note (the JS-shell guess is wrong for a
/// PDF; the note names the corpus convert route — observed 2026-11-01:
/// a 153-char shell for a Locke & Latham PDF).
struct PdfShellPool;

stub_web_search_port! {
    PdfShellPool, "stub-pdf-shell",
    search(self, _query, _strategy, _provider): {
        no_provider!("No search provider configured.")
    },
    find_similar: {
        no_provider!("Exa provider not configured. Set HKASK_EXA_API_KEY.")
    },
    extract: {
        Ok(ExtractedContent {
            url: "https://example.com/papers/locke-latham.pdf".to_string(),
            content: "[nav shell]".to_string(),
            format: "markdown".to_string(),
            metadata: Some(serde_json::json!({"statusCode": 200})),
        })
    },
    health: { Vec::new() },
    kinds: { Vec::new() },
    score(self, _query, _intent): { Vec::new() }
}

/// expect: "A degraded PDF extraction names the PDF diagnosis and the
/// corpus_convert route, not the generic JS-shell cause list."
#[tokio::test]
async fn web_extract_degraded_pdf_names_the_corpus_route() {
    let server = make_server_with_pool(Arc::new(PdfShellPool));
    let request = ExtractRequest {
        url: "https://example.com/papers/locke-latham.pdf".to_string(),
        format: None,
        json_prompt: None,
        json_schema: None,
        main_content_only: None,
        wait_for_ms: None,
        run_id: None,
    };
    let json = invoke_tool!(server, web_extract, request);
    let note = json["note"].as_str().unwrap_or_default();
    assert!(note.contains("PDF"), "names the PDF diagnosis: {note}");
    assert!(
        note.contains("corpus_convert"),
        "names the corpus route: {note}"
    );
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

defaults_pool! {
    TitleResolvePool, "stub-title-resolve",
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

defaults_pool! {
    SuperstringFirstTitlePool, "stub-superstring-first",
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

/// Stub whose `resolve_title` errors with `WebError::RateLimited` —
/// the 2026-11-02 incident class: a Crossref 429 burst (5 parallel
/// run-scoped resolve_paper title calls, all bypassing the cache)
/// classified as `failed_precondition`, reading "tool defect" in the
/// reliability breakdown instead of "externally throttled." The fix
/// (this change): a `RateLimited` WebError surfaces as `McpErrorKind::RateLimited`.
struct RateLimitedTitlePool;

defaults_pool! {
    RateLimitedTitlePool, "stub-rate-limited-title",
    resolve_title: {
        Err(WebError::RateLimited(
            "Crossref rate limited: 429 Too Many Requests".to_string(),
        ))
    }
}

/// Stub whose `resolve_title` errors with `WebError::ProviderUnavailable` —
/// pins the honest `Unavailable` classification: a Crossref transport
/// failure is a config-gap (the ledger's `is_not_tool_fault_kind` set),
/// never a tool defect.
struct UnavailableTitlePool;

defaults_pool! {
    UnavailableTitlePool, "stub-unavailable-title",
    resolve_title: {
        Err(WebError::ProviderUnavailable(
            "Crossref request failed: connection refused".to_string(),
        ))
    }
}

/// Stub whose `resolve_title` errors with `WebError::ProviderError` — a
/// Crossref error response (`Internal`).
struct ProviderErrorTitlePool;

defaults_pool! {
    ProviderErrorTitlePool, "stub-provider-error-title",
    resolve_title: {
        Err(WebError::ProviderError(
            "Crossref error 500: internal server error".to_string(),
        ))
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
        Box::pin(async move {
            Ok(hkask_types::RerankBatch {
                scores,
                usage: hkask_types::InferenceUsage::default(),
            })
        })
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
    private_embedding_model: Option<&str>,
) -> ResearchServer {
    ResearchServer::new(
        WebID::new(),
        pool,
        Arc::new(ResponseCache::new(cache_capacity, Duration::from_secs(60))),
        RateLimiter::new(10000, 60),
        database,
        inference_port.unwrap_or_else(|| Arc::new(FailingInferencePort)),
        rerank_model.map(str::to_string),
        embedding_model.map(str::to_string),
        private_embedding_model.map(str::to_string),
        // The private leg's local rerank model — None here keeps the
        // existing pins on the opt-out skip; the routing pins build via
        // `test_server_with_private_rerank` below.
        None,
    )
}

/// zed-kask: D100 §8.1 routed shape — a construction seam that carries the
/// private leg's LOCAL rerank model (the routing pins' server shape).
fn test_server_with_private_rerank(
    pool: Arc<dyn WebSearchPort>,
    inference_port: Arc<dyn InferencePort>,
    rerank_model: Option<&str>,
    private_rerank_model: Option<&str>,
) -> ResearchServer {
    ResearchServer::new(
        WebID::new(),
        pool,
        Arc::new(ResponseCache::new(10, Duration::from_secs(60))),
        RateLimiter::new(10000, 60),
        None,
        inference_port,
        rerank_model.map(str::to_string),
        None,
        None,
        private_rerank_model.map(str::to_string),
    )
}

fn make_server_without_db() -> ResearchServer {
    test_server(
        Arc::new(NoCredentialsPool),
        10,
        None,
        None,
        None,
        None,
        None,
    )
}

fn make_server_with_pool(pool: Arc<dyn WebSearchPort>) -> ResearchServer {
    test_server(pool, 10, None, None, None, None, None)
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
        None,
    )
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
    let json = parse_envelope(&out);
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

#[tokio::test]
async fn web_ping_lists_the_free_scholarly_providers_in_the_profile_table() {
    // C4b: the free providers carry static profiles, so web_ping's
    // metacognitive table lists them — an agent reading the ping sees the
    // full set of selectable providers, not just the paid ones.
    let server = make_server_with_pool(Arc::new(FreeProvidersPool));
    let json = parse_envelope(&ok(server.web_ping().await));
    let rows: Vec<&serde_json::Value> = json
        .get("provider_profiles")
        .and_then(|p| p.as_array())
        .map(|array| array.iter().collect())
        .unwrap_or_default();
    for kind in ["openalex", "arxiv", "semantic_scholar"] {
        assert!(
            rows.iter()
                .any(|r| r.get("kind").and_then(|k| k.as_str()) == Some(kind)),
            "web_ping profile table missing {kind}: {json}"
        );
    }
    // The free providers' rows carry their profiled cost (0.0).
    let arxiv_row = rows
        .iter()
        .find(|r| r.get("kind").and_then(|k| k.as_str()) == Some("arxiv"))
        .expect("arxiv row");
    assert_eq!(
        arxiv_row.get("cost_per_call_usd").and_then(|c| c.as_f64()),
        Some(0.0),
        "free providers must be profiled at cost 0: {json}"
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

// ── web_search closed-vocabulary normalization ─────────────────────────────
// LLM tool-call emitters wrap enum values in quotes/padding (observed live
// 2026-10-09: `strategy: "\"quick\""` rejected as unknown, costing agent
// retries). The boundary normalizes once, up front — these tests pin that
// every closed-vocabulary field accepts the noisy form while genuinely
// unknown values still reject (the tests above).

#[tokio::test]
async fn web_search_accepts_quoted_and_padded_strategy_and_freshness() {
    let server = intent_selection_server().0;
    // `"quick"` with literal quote characters, and `  "web"  ` padded —
    // both must parse. Before the normalization this errored with
    // "Unknown strategy: \"quick\"".
    for (strategy, freshness) in [
        (Some("\"quick\"".to_string()), Some("'week'".to_string())),
        (
            Some("  \"web\"  ".to_string()),
            Some("  month  ".to_string()),
        ),
    ] {
        let output = server
            .web_search(Parameters(SearchRequest {
                query: "normalization probe".to_string(),
                num_results: Some(3),
                include_domains: None,
                exclude_domains: None,
                freshness,
                strategy,
                provider: None,
                run_id: None,
                intent: None,
            }))
            .await;
        let parsed = parse_envelope(&ok(output));
        assert!(
            parsed.get("results").is_some_and(|r| r.is_array()),
            "quoted strategy/freshness must succeed, got: {parsed}"
        );
    }
}

#[tokio::test]
async fn web_search_accepts_quoted_intent_and_routes_by_it() {
    let (server, pool) = intent_selection_server();
    let output = server
        .web_search(Parameters(SearchRequest {
            query: "normalization probe".to_string(),
            num_results: Some(3),
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            provider: None,
            run_id: None,
            intent: Some("  \"academic\"  ".to_string()),
        }))
        .await;
    let parsed = parse_envelope(&ok(output));
    // The normalized intent drove provider selection: the stub's top
    // configured recommendation (arxiv) was queried...
    let selected = pool
        .selected_provider
        .lock()
        .expect("selected provider lock");
    assert_eq!(
        selected.last().map(|p| p.clone()),
        Some(Some("arxiv".to_string())),
        "quoted intent must route to the top recommendation"
    );
    // ...and the surfaced ranking echoes the NORMALIZED intent (the stub's
    // rationale interpolates the intent it received).
    let rationale = parsed
        .get("provider_recommendations")
        .and_then(|r| r.get(0))
        .and_then(|r| r.get("rationale"))
        .and_then(|r| r.as_str())
        .unwrap_or_default();
    assert!(
        rationale.contains("academic"),
        "normalized intent must flow to score_providers, got: {rationale}"
    );
}

#[tokio::test]
async fn web_search_normalizes_provider_before_dispatch() {
    let (server, pool) = intent_selection_server();
    let output = server
        .web_search(Parameters(SearchRequest {
            query: "normalization probe".to_string(),
            num_results: Some(3),
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            // A quoted provider name previously flowed through raw — in
            // production the pool misattributed it to a missing API key.
            provider: Some("\"arxiv\"".to_string()),
            run_id: None,
            intent: None,
        }))
        .await;
    let parsed = parse_envelope(&ok(output));
    assert_eq!(
        parsed.get("selected_provider").and_then(|p| p.as_str()),
        Some("arxiv"),
        "quoted provider must dispatch normalized, got: {parsed}"
    );
    let selected = pool
        .selected_provider
        .lock()
        .expect("selected provider lock");
    assert_eq!(
        selected.last().map(|p| p.clone()),
        Some(Some("arxiv".to_string())),
        "the port must receive the normalized provider name"
    );
}

#[tokio::test]
async fn web_search_rejects_unknown_intent() {
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
            intent: Some("bogus".to_string()),
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("intent"),
        "error names the field: {}",
        error.message
    );
    assert!(
        error.message.contains("news"),
        "error teaches the accepted values: {}",
        error.message
    );
}

#[tokio::test]
async fn web_search_surfaces_provider_recommendations_on_non_intent_paths() {
    // C4: the live-merged ranking is the audit trail for WHY providers
    // were (or would be) chosen — surfaced on every path where the tool
    // makes the selection, not just the intent pick. A web-strategy call
    // (no intent, no provider) must carry the ranking.
    let server = intent_selection_server().0;
    let parsed = invoke_tool!(
        server,
        web_search,
        SearchRequest {
            query: "fusion audit probe".to_string(),
            num_results: Some(3),
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: Some("web".to_string()),
            provider: None,
            run_id: None,
            intent: None,
        }
    );
    let recommendations = parsed
        .get("provider_recommendations")
        .and_then(|r| r.as_array());
    assert!(
        recommendations.is_some_and(|r| !r.is_empty()),
        "web-strategy call must surface the ranking, got: {parsed}"
    );
}

#[tokio::test]
async fn web_search_skips_ranking_for_explicit_provider() {
    // The counterpoint: an explicit-provider call is the caller's own
    // deliberate override — selected_provider carries the choice, the
    // ranking is skipped.
    let server = intent_selection_server().0;
    let parsed = invoke_tool!(
        server,
        web_search,
        SearchRequest {
            query: "override probe".to_string(),
            num_results: Some(3),
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            provider: Some("arxiv".to_string()),
            run_id: None,
            intent: None,
        }
    );
    let recommendations = parsed
        .get("provider_recommendations")
        .and_then(|r| r.as_array());
    assert!(
        recommendations.is_none_or(|r| r.is_empty()),
        "explicit-provider call must skip the ranking, got: {parsed}"
    );
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

#[tokio::test]
async fn web_extract_rejects_unknown_format() {
    // The closed-set gate: Firecrawl silently maps unknown formats to
    // markdown while echoing the REQUESTED format, so an unknown value
    // must reject up front (the format check precedes the pool call).
    let server = make_server_without_db();
    let error = err(server
        .web_extract(Parameters(ExtractRequest {
            url: LITERAL_IP_URL.to_string(),
            format: Some("bogus".to_string()),
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
            run_id: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("format"),
        "error should mention format; got: {}",
        error.message
    );
}

#[tokio::test]
async fn web_extract_accepts_quoted_format() {
    // `"json"` with literal quote characters normalizes to `json` and
    // proceeds PAST the format gate — the stub pool then surfaces its
    // credential-missing arm (PermissionDenied), proving the format check
    // passed rather than silently mapping to markdown.
    let server = make_server_without_db();
    let error = err(server
        .web_extract(Parameters(ExtractRequest {
            url: LITERAL_IP_URL.to_string(),
            format: Some("\"json\"".to_string()),
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
            run_id: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::PermissionDenied);
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

// ── web_extract SSRF destination gate ─────────────────────

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

// ── web_search deep-strategy LLM rerank ────────────────────────────────────

/// Stub pool that returns three fixed results for any search — lets the
/// deep-strategy rerank stage run against a stub inference port without
/// touching the network.
struct FixedResultsPool;

defaults_pool! {
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
            providers_skipped: Vec::new(),
            total_before_dedup: 3,
            duplicates_removed: 0,
        })
    }
}

fn make_server_with_pool_and_port(
    pool: Arc<dyn WebSearchPort>,
    inference_port: Arc<dyn InferencePort>,
    rerank_model: Option<&str>,
) -> ResearchServer {
    test_server(
        pool,
        10,
        None,
        Some(inference_port),
        rerank_model,
        None,
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
    let blocked = invoke_tool!(server, web_search, request);
    assert_eq!(blocked["count"], 0);
    assert_eq!(blocked["results"], serde_json::json!([]));

    let mut request = deep_search_request();
    request.strategy = Some("quick".to_string());
    request.include_domains = Some(vec!["example.com".to_string()]);
    let included = invoke_tool!(server, web_search, request);
    assert_eq!(included["count"], 3);

    let mut request = deep_search_request();
    request.strategy = Some("quick".to_string());
    request.exclude_domains = Some(vec!["example.com".to_string()]);
    let excluded = invoke_tool!(server, web_search, request);
    assert_eq!(excluded["count"], 0);
    assert_eq!(excluded["results"], serde_json::json!([]));
}

/// expect: A run-scoped official-domain search never records provider hits
/// excluded from the response as server-observed primary sources.
#[tokio::test]
async fn filtered_search_does_not_ledger_off_domain_hits() {
    let server = make_server_with_pool_and_db(Arc::new(FixedResultsPool), Some(research_db_pool()));
    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "official-domain boundary".to_string(),
        }
    );
    let run_id = begun["run_id"].as_str().expect("run id").to_string();
    let mut request = deep_search_request();
    request.strategy = Some("quick".to_string());
    request.include_domains = Some(vec!["amf-france.org".to_string()]);
    request.run_id = Some(run_id.clone());
    let output = invoke_tool!(server, web_search, request);
    assert_eq!(output["count"], 0);
    assert_eq!(output["domain_filter_removed"], 3);
    assert_eq!(output["run_ledger"]["recorded"], 0);
    let manifest = invoke_tool!(server, get_research_run, GetResearchRunRequest { run_id });
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
    let output = invoke_tool!(server, web_search, deep_search_request());

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
    let output = invoke_tool!(server, web_search, deep_search_request());

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

/// A rerank-RECORDING inference port: every rerank call bumps the shared
/// counter and scores like the scoring stub. The private-provenance pin
/// asserts the counter stays ZERO (the configured rerank model is never
/// called for a private query); the public control asserts it moved.
struct RecordingRerankPort {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Every rerank call's MODEL string, in call order — the routing pins
    /// assert the private-provenance call used the private model only.
    models: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

stub_inference_port! {
    RecordingRerankPort,
    generate_msg: "stub: generate unused in rerank tests",
    rerank(self, _model, _query, _documents): {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.models
            .lock()
            .expect("models mutex")
            .push(_model.to_string());
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
        Box::pin(async move {
            Ok(hkask_types::RerankBatch {
                scores,
                usage: hkask_types::InferenceUsage::default(),
            })
        })
    }
}

/// zed-kask: D100 (the tool-query-embed design §8.1, operator-approved
/// 2026-11-06) — the provenance-routed rerank leg, both sides. A
/// private-provenance deep search makes ZERO rerank calls (the configured
/// cloud model never sees the private query or candidates) and surfaces
/// the skip: heuristic mode, the reason naming the private boundary, the
/// heuristic RRF order kept. The explicit public control proves the gate
/// does not over-block: the same configured model reranks as before.
#[tokio::test]
async fn private_call_skips_llm_rerank_with_the_boundary_surfaced() {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let server = make_server_with_pool_and_port(
        Arc::new(FixedResultsPool),
        Arc::new(RecordingRerankPort {
            calls: calls.clone(),
            models: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }),
        Some("test-rerank-model"),
    );
    let output = parse_envelope(&ok(hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        server.web_search(Parameters(deep_search_request())),
    )
    .await));

    let rerank = output
        .get("rerank")
        .expect("the private skip still surfaces rerank info");
    assert_eq!(
        rerank.get("mode").and_then(|m| m.as_str()),
        Some("heuristic"),
        "the private skip keeps the heuristic order, surfaced: {rerank:?}"
    );
    let reason = rerank
        .get("reason")
        .and_then(|r| r.as_str())
        .expect("the private skip must name the boundary");
    assert!(
        reason.contains("private-provenance") && reason.contains("kask.models.rerank_model"),
        "the reason names the private boundary and the refused setting: {reason}"
    );
    assert!(
        reason.contains("kask.private_inference.rerank_model"),
        "the skip names the setting that routes locally: {reason}"
    );
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "a private-provenance call makes ZERO configured-model rerank calls"
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
        "the heuristic RRF order is kept on the private skip"
    );
}

/// The public control for the rerank leg: an explicit PUBLIC provenance
/// reranks through the configured model — the gate is scoped to private
/// calls, never a blanket refusal.
#[tokio::test]
async fn public_call_reranks_through_the_configured_model() {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let server = make_server_with_pool_and_port(
        Arc::new(FixedResultsPool),
        Arc::new(RecordingRerankPort {
            calls: calls.clone(),
            models: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }),
        Some("test-rerank-model"),
    );
    let output = parse_envelope(&ok(hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PublicProvider,
        server.web_search(Parameters(deep_search_request())),
    )
    .await));

    let rerank = output
        .get("rerank")
        .expect("deep strategy must surface rerank info");
    assert_eq!(
        rerank.get("mode").and_then(|m| m.as_str()),
        Some("llm"),
        "the public call reranks as before: {rerank:?}"
    );
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::Relaxed),
        1,
        "the public control dispatches exactly one rerank call"
    );
}

/// zed-kask: D100 §8.1 routed shape (operator ruling 2026-11-06 — private
/// mode uses the ollama qwen reranker) — the positive control: with the
/// private rerank model configured, a private-provenance deep search RERANKS
/// (the surfaced skip is gone) and the recording port captured the PRIVATE
/// model only — the configured cloud model is never called for private
/// content. The public control asserts the configured model still serves
/// public calls (the routing does not over-block).
#[tokio::test]
async fn private_call_reranks_via_the_private_model_only() {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let models: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = test_server_with_private_rerank(
        Arc::new(FixedResultsPool),
        Arc::new(RecordingRerankPort {
            calls: calls.clone(),
            models: models.clone(),
        }),
        Some("test-configured-rerank-model"),
        Some("ollama/qwen3-reranker-4b:latest"),
    );
    let output = parse_envelope(&ok(hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        server.web_search(Parameters(deep_search_request())),
    )
    .await));

    let rerank = output
        .get("rerank")
        .expect("the routed private call surfaces rerank info");
    assert_eq!(
        rerank.get("mode").and_then(|m| m.as_str()),
        Some("llm"),
        "the private call reranks through the private model: {rerank:?}"
    );
    assert!(
        calls.load(std::sync::atomic::Ordering::Relaxed) > 0,
        "the private call made rerank calls through the private model"
    );
    let captured = models.lock().expect("models mutex").clone();
    assert!(
        captured
            .iter()
            .all(|model| model == "ollama/qwen3-reranker-4b:latest"),
        "every private-provenance rerank call used the private model: {captured:?}"
    );

    // The public control: the configured cloud model still serves public
    // calls — a FRESH recorder, so the captured models are this leg's alone
    // (the private leg's entries do not pollute the check).
    let models_public: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = test_server_with_private_rerank(
        Arc::new(FixedResultsPool),
        Arc::new(RecordingRerankPort {
            calls: calls.clone(),
            models: models_public.clone(),
        }),
        Some("test-configured-rerank-model"),
        Some("ollama/qwen3-reranker-4b:latest"),
    );
    let output = invoke_tool!(server, web_search, deep_search_request());
    let rerank = output
        .get("rerank")
        .expect("the public call surfaces rerank info");
    assert_eq!(
        rerank.get("mode").and_then(|m| m.as_str()),
        Some("llm"),
        "the public call still reranks through the configured model: {rerank:?}"
    );
    let captured = models_public.lock().expect("models mutex").clone();
    assert!(
        captured
            .iter()
            .all(|model| model == "test-configured-rerank-model"),
        "public calls keep the configured model: {captured:?}"
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
    let output = invoke_tool!(server, web_search, request);
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

defaults_pool! {
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
                providers_skipped: Vec::new(),
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
            providers_skipped: Vec::new(),
            total_before_dedup: 0,
            duplicates_removed: 0,
        })
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
    let first = parse_envelope(&ok(server.web_search(Parameters(make_request())).await));
    assert!(
        first["providers_failed"]
            .as_array()
            .is_some_and(|f| !f.is_empty()),
        "the transient failure must be surfaced in the first response"
    );

    // …and must NOT be replayed from cache on the identical second call.
    let second = parse_envelope(&ok(server.web_search(Parameters(make_request())).await));
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
            providers_skipped: Vec::new(),
            answer_box: None,
            related_questions: Vec::new(),
            total_before_dedup: 0,
            duplicates_removed: 0,
        })
    },
    find_similar: { no_provider!("not configured") },
    extract: { no_provider!("not configured") },
    health: { Vec::new() },
    kinds: { vec!["arxiv".to_string()] },
    score(self, _query, _intent): {
        vec![
            ProviderRecommendation {
                kind: "arxiv".to_string(),
                score: 1.0,
                rationale: format!("best for {} intent", _intent.unwrap_or("general")),
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
                configured: false,
                live_success_rate: None,
                live_p50_latency_ms: None,
                live_sample_count: None,
                live_stats_degraded: false,
            },
        ]
    }
}

/// Server backed by a fresh `IntentSelectionPool` — the fixture of the
/// provider-selection tests. Returns the server and the pool (the pool's
/// recorded provider selections are those tests' assertions).
fn intent_selection_server() -> (ResearchServer, Arc<IntentSelectionPool>) {
    let pool = Arc::new(IntentSelectionPool {
        selected_provider: std::sync::Mutex::new(Vec::new()),
    });
    let server = test_server(pool.clone(), 0, None, None, None, None, None);
    (server, pool)
}

#[tokio::test]
async fn web_search_intent_selects_top_configured_provider_and_surfaces_ranking() {
    let server = intent_selection_server().0;
    let parsed = invoke_tool!(
        server,
        web_search,
        SearchRequest {
            query: "rust async runtime benchmarks".to_string(),
            num_results: Some(5),
            include_domains: None,
            exclude_domains: None,
            freshness: None,
            strategy: None,
            intent: Some("academic".to_string()),
            provider: None,
            run_id: None,
        }
    );

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
    let json = parse_envelope(&out);

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
    let json = parse_envelope(&out);
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
    // The run tables must exist after the schema DDL runs, and the feed
    // tables the server created until 2026-11-12 must not be created.
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
    let feed_objects: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name IN \
             ('feeds','subscriptions','entries','entry_states','entries_fts','synthetic_feeds')",
            [],
            |row| row.get(0),
        )
        .expect("sqlite_master query");
    assert_eq!(feed_objects, 0, "the schema must not create feed tables");

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
    test_server(pool, 10, database, None, None, None, None)
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
    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "is the wire story corroborated?".to_string(),
        }
    );
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();
    assert_eq!(begun["status"].as_str(), Some("planned"));
    assert_eq!(run_id.len(), 16, "run_id is blake3[..16] hex: {run_id}");

    let manifest = invoke_tool!(
        server,
        get_research_run,
        GetResearchRunRequest {
            run_id: run_id.clone(),
        }
    );
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

    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "stub results".to_string(),
        }
    );
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    let output = invoke_tool!(
        server,
        web_search,
        run_scoped_stub_search(Some("deep"), &run_id)
    );
    assert_eq!(
        output["run_ledger"]["recorded"].as_u64(),
        Some(3),
        "three stub results recorded: {output}"
    );

    assert_eq!(output["run_ledger"]["already_recorded"].as_u64(), Some(0));

    // A repeat call in the same run keeps the first observation: the note
    // names the held rows instead of reporting a bare zero.
    let repeat = invoke_tool!(
        server,
        web_search,
        run_scoped_stub_search(Some("deep"), &run_id)
    );
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

    let manifest = invoke_tool!(server, get_research_run, GetResearchRunRequest { run_id });
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

    let output = invoke_tool!(
        server,
        web_search,
        run_scoped_stub_search(None, "deadbeefdeadbeef")
    );
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
    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "never-served sources".to_string(),
        }
    );
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
    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "basis required".to_string(),
        }
    );
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    // Record a source the server actually served.
    let search = invoke_tool!(server, web_search, run_scoped_stub_search(None, &run_id));
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
    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "roundtrip".to_string(),
        }
    );
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    let search = invoke_tool!(server, web_search, run_scoped_stub_search(None, &run_id));
    let served_url = search["results"][0]["url"]
        .as_str()
        .expect("stub result url")
        .to_string();

    // Annotating twice is idempotent (PRIMARY KEY (run_id, url) upsert —
    // re-annotating after a crash is safe).
    for _ in 0..2 {
        let annotated = invoke_tool!(
            server,
            annotate_research_run,
            AnnotateResearchRunRequest {
                run_id: run_id.clone(),
                url: served_url.clone(),
                verification_state: "verified".to_string(),
                basis: Some("cross-checked against the primary source".to_string()),
            }
        );
        assert_eq!(annotated["run_id"].as_str(), Some(run_id.as_str()));
    }

    let manifest = invoke_tool!(
        server,
        get_research_run,
        GetResearchRunRequest {
            run_id: run_id.clone(),
        }
    );
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
    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "external sources".to_string(),
        }
    );
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    let annotated = invoke_tool!(
        server,
        annotate_research_run,
        AnnotateResearchRunRequest {
            run_id: run_id.clone(),
            url: "https://external.example/1".to_string(),
            verification_state: "inferred".to_string(),
            basis: Some("prior knowledge".to_string()),
        }
    );
    assert_eq!(annotated["recorded_by"].as_str(), Some("agent"));

    let manifest = invoke_tool!(
        server,
        get_research_run,
        GetResearchRunRequest {
            run_id: run_id.clone(),
        }
    );
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
    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "enum check".to_string(),
        }
    );
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

// ── Closed-vocabulary boundary: sibling fields ──────────────────────────────
// The shared parse contract (normalize_closed_vocab + parse_closed_vocab*)
// covers every closed-vocabulary field in the server, not just web_search's:
// finish status, annotate verification_state, cite style and evidence
// duplication all accept quoted/padded case-variant emissions, and unknown
// values teach the accepted spellings.

#[tokio::test]
async fn finish_research_run_accepts_quoted_status() {
    let server = make_server_with_research_db();
    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "quoted status probe".to_string(),
        }
    );
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();
    // `"Blocked"` — quoted, padded, wrong case — must parse to the
    // canonical spelling and finish the run (blocked needs no
    // server-recorded source).
    let finished = invoke_tool!(
        server,
        finish_research_run,
        FinishResearchRunRequest {
            run_id,
            status: " \"Blocked\" ".to_string(),
            note: None,
        }
    );
    assert!(
        finished.is_object(),
        "quoted status must finish the run: {finished}"
    );
}

#[tokio::test]
async fn annotate_research_run_accepts_quoted_verification_state() {
    let server = make_server_with_research_db();
    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "quoted state probe".to_string(),
        }
    );
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();
    // Annotating an unseen URL records an agent-declared row — no
    // server-recorded source needed for not_checked.
    let annotated = invoke_tool!(
        server,
        annotate_research_run,
        AnnotateResearchRunRequest {
            run_id,
            url: "https://external.example/quoted".to_string(),
            verification_state: " \"Not_Checked\" ".to_string(),
            basis: None,
        }
    );
    assert_eq!(
        annotated["verification_state"].as_str(),
        Some("not_checked"),
        "quoted verification_state must parse to the canonical spelling: {annotated}"
    );
}

// ── Wire-level pins: arguments arrive as JSON, the way a client sends them ──

/// Decode `arguments` into a tool's request type exactly as the tool
/// boundary does, so a pin covers the wire shape and not a hand-built
/// struct. A decode failure is the same `invalid_argument` a client sees.
fn decode<T: serde::de::DeserializeOwned>(
    arguments: serde_json::Value,
) -> Result<Parameters<T>, McpToolError> {
    serde_json::from_value(arguments)
        .map(Parameters)
        .map_err(|error| McpToolError::invalid_argument(error.to_string()))
}

async fn cite(
    server: &ResearchServer,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, McpToolError> {
    let response = server.cite_sources(decode(arguments)?).await?;
    Ok(parse_envelope(&response))
}

/// expect: "cite_sources returns an APA 7 reference list" — Purdue OWL's
/// webpage example, sent as JSON.
#[tokio::test]
async fn cite_sources_formats_an_apa_reference_from_json_arguments() {
    let server = make_server_without_db();
    let cited = cite(
        &server,
        serde_json::json!({"sources": [{
            "url": "https://humanparts.medium.com/laziness-does-not-exist-3af27e312d01",
            "title": "Laziness does not exist",
            "authors": [{"family": "Price", "given": "Devon"}],
            "published": "2018-03-23",
            "site_name": "Medium"
        }]}),
    )
    .await
    .expect("formats");
    assert_eq!(cited["count"], 1, "{cited}");
    assert_eq!(
        cited["references"][0]["reference"],
        "Price, D. (2018, March 23). *Laziness does not exist*. Medium. https://humanparts.medium.com/laziness-does-not-exist-3af27e312d01",
        "{cited}"
    );
    assert_eq!(cited["references"][0]["source_index"], 0, "{cited}");
    assert_eq!(cited["notes"], serde_json::json!([]), "{cited}");
}

/// expect: an author sent as a string is a group name kept as written; an
/// author sent as an object is a person; no author puts the title first;
/// no date reads (n.d.); the list comes back in reference-list order with
/// each entry's request position.
#[tokio::test]
async fn cite_sources_orders_the_list_and_handles_group_missing_author_and_missing_date() {
    let server = make_server_without_db();
    let cited = cite(
        &server,
        serde_json::json!({"sources": [
            {"url": "https://example.org/z", "title": "Zebra crossings", "published": "2020"},
            {"url": "https://www.nami.org/Learn-More/Mental-Health-Conditions",
             "title": "Mental health conditions",
             "authors": ["National Alliance on Mental Illness"]},
            {"doi": "10.1145/1571941.1572114",
             "title": "Reciprocal rank fusion outperforms Condorcet and individual rank learning methods",
             "authors": [{"family": "Cormack", "given": "Gordon V."},
                         {"family": "Clarke", "given": "Charles L. A."},
                         {"family": "Büttcher", "given": "Stefan"}],
             "published": "2009"}
        ]}),
    )
    .await
    .expect("formats");
    let entries = cited["references"].as_array().expect("references");
    let order: Vec<u64> = entries
        .iter()
        .map(|entry| entry["source_index"].as_u64().expect("index"))
        .collect();
    assert_eq!(
        order,
        vec![2, 1, 0],
        "alphabetical by author, then by title: {cited}"
    );
    assert_eq!(
        entries[0]["reference"],
        "Cormack, G. V., Clarke, C. L. A., & Büttcher, S. (2009). *Reciprocal rank fusion outperforms Condorcet and individual rank learning methods*. https://doi.org/10.1145/1571941.1572114"
    );
    assert_eq!(
        entries[1]["reference"],
        "National Alliance on Mental Illness. (n.d.). *Mental health conditions*. https://www.nami.org/Learn-More/Mental-Health-Conditions"
    );
    assert_eq!(
        entries[2]["reference"],
        "*Zebra crossings*. (2020). https://example.org/z"
    );
}

/// expect: a `published` value copied from a search result ("4 days ago")
/// is refused naming the source and the accepted forms — never turned
/// into a year. Before the rebuild this produced "(4 da)".
#[tokio::test]
async fn cite_sources_refuses_a_relative_date_naming_the_source_and_the_forms() {
    let server = make_server_without_db();
    let error = cite(
        &server,
        serde_json::json!({"sources": [
            {"url": "https://example.org/a", "title": "A"},
            {"url": "https://example.org/b", "title": "B", "published": "4 days ago"}
        ]}),
    )
    .await
    .expect_err("refused");
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("sources[1].published"),
        "{}",
        error.message
    );
    assert!(error.message.contains("YYYY-MM-DD"), "{}", error.message);
}

/// expect: a structured (`json`) extraction is not judged by the length
/// floor for page text. Measured 2026-11-12 against Firecrawl: a correct
/// 75-character object came back with a note calling it a JS shell.
/// The same short text requested as markdown still carries the note.
#[tokio::test]
async fn web_extract_json_answer_is_not_labelled_a_page_shell() {
    let server = make_server_with_pool(Arc::new(ShortJsonPool));
    let response = server
        .web_extract(
            decode(serde_json::json!({
                "url": "https://example.com/page",
                "format": "json",
                "json_prompt": "title and domains",
                "json_schema": "{\"type\":\"object\"}"
            }))
            .expect("arguments decode"),
        )
        .await
        .expect("extracts");
    let structured = parse_envelope(&response);
    assert_eq!(structured["format"], "json", "{structured}");
    assert!(
        structured.get("note").is_none(),
        "a valid structured answer carries no degradation note: {structured}"
    );

    let response = server
        .web_extract(
            decode(serde_json::json!({"url": "https://example.com/page"})).expect("decode"),
        )
        .await
        .expect("extracts");
    let markdown = parse_envelope(&response);
    assert!(
        markdown["note"].is_string(),
        "75 characters of page text is still reported as thin: {markdown}"
    );
}

/// expect: the fields of the tool this one replaced are refused by name,
/// not ignored: `style`, and the old per-source `source`.
#[tokio::test]
async fn cite_sources_refuses_the_removed_style_and_source_arguments() {
    let server = make_server_without_db();
    let with_style = cite(
        &server,
        serde_json::json!({"sources": [{"url": "https://example.org/a", "title": "A"}], "style": "apa"}),
    )
    .await
    .expect_err("style is gone");
    assert_error_kind(&with_style, McpErrorKind::InvalidArgument);
    assert!(
        with_style.message.contains("style"),
        "{}",
        with_style.message
    );

    let with_source = cite(
        &server,
        serde_json::json!({"sources": [{"url": "https://example.org/a", "title": "A", "source": "example.org"}]}),
    )
    .await
    .expect_err("source is gone");
    assert_error_kind(&with_source, McpErrorKind::InvalidArgument);
    assert!(
        with_source.message.contains("source"),
        "{}",
        with_source.message
    );
}

/// expect: a misspelled argument is refused on every tool that takes
/// arguments, naming the unknown field. Before this pin a misspelled
/// optional argument was dropped: `duplicaton: "semantic"` ran the default
/// tier and reported nothing.
#[tokio::test]
async fn every_tool_refuses_an_unknown_argument_by_name() {
    use hkask_mcp_research::research::types::*;
    fn refused<T: serde::de::DeserializeOwned>(tool: &str, mut arguments: serde_json::Value) {
        arguments["zz_unknown"] = serde_json::json!(1);
        let error = match decode::<T>(arguments) {
            Ok(_) => panic!("{tool} accepted an unknown argument"),
            Err(error) => error,
        };
        assert_eq!(error.kind, McpErrorKind::InvalidArgument, "{tool}");
        assert!(
            error.message.contains("zz_unknown"),
            "{tool} must name the unknown field: {}",
            error.message
        );
    }
    refused::<SearchRequest>("web_search", serde_json::json!({"query": "q"}));
    refused::<FindSimilarRequest>(
        "web_find_similar",
        serde_json::json!({"url": "https://example.org/"}),
    );
    refused::<ExtractRequest>(
        "web_extract",
        serde_json::json!({"url": "https://example.org/"}),
    );
    refused::<BeginResearchRunRequest>("begin_research_run", serde_json::json!({"question": "q"}));
    refused::<GetResearchRunRequest>("get_research_run", serde_json::json!({"run_id": "r"}));
    refused::<FinishResearchRunRequest>(
        "finish_research_run",
        serde_json::json!({"run_id": "r", "status": "failed"}),
    );
    refused::<AnnotateResearchRunRequest>(
        "annotate_research_run",
        serde_json::json!({"run_id": "r", "url": "https://example.org/", "verification_state": "inferred"}),
    );
    refused::<ResolvePaperRequest>("resolve_paper", serde_json::json!({"query": "10.1/x"}));
    refused::<EvaluateEvidenceRequest>(
        "evaluate_evidence",
        serde_json::json!({"question": "q", "artifacts": [{"url": "https://example.org/"}]}),
    );
    refused::<CiteSourcesRequest>(
        "cite_sources",
        serde_json::json!({"sources": [{"url": "https://example.org/", "title": "T"}]}),
    );

    // The measured case: a misspelled optional argument on evaluate_evidence.
    let misspelled = decode::<EvaluateEvidenceRequest>(serde_json::json!({
        "question": "q",
        "artifacts": [{"url": "https://example.org/"}],
        "duplicaton": "semantic"
    }));
    let error = misspelled.err().expect("misspelled argument is refused");
    assert!(error.message.contains("duplicaton"), "{}", error.message);
    // A nested object is strict too.
    let nested = decode::<EvaluateEvidenceRequest>(serde_json::json!({
        "question": "q",
        "artifacts": [{"url": "https://example.org/", "contnet": "x"}]
    }));
    assert!(
        nested
            .err()
            .expect("nested unknown is refused")
            .message
            .contains("contnet")
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
    let json = parse_envelope(&out);
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

/// expect: an OpenAlex work resolves to the page OpenAlex serves
/// (`/works/W…`), from every form of the identifier, and that page URL is
/// itself an accepted input. Before, the tool returned `/work/W…`, which
/// OpenAlex refuses with HTTP 403 (measured 2026-11-12); that form is now
/// rejected as an identifier.
#[tokio::test]
async fn resolve_paper_returns_the_openalex_page_url_it_also_accepts() {
    let server = make_server_without_db();
    for query in [
        "W2741809807",
        "https://openalex.org/W2741809807",
        "https://openalex.org/works/W2741809807",
    ] {
        let response = server
            .resolve_paper(decode(serde_json::json!({ "query": query })).expect("decode"))
            .await
            .expect("resolves");
        let json = parse_envelope(&response);
        assert_eq!(json["identifier"]["kind"], "openalex", "{query}");
        assert_eq!(
            json["canonical_url"], "https://openalex.org/works/W2741809807",
            "{query}"
        );
        assert_eq!(json["stable_key"], "openalex:W2741809807", "{query}");
    }
    let error = server
        .resolve_paper(
            decode(serde_json::json!({"query": "https://openalex.org/work/W2741809807"}))
                .expect("decode"),
        )
        .await
        .expect_err("the refused URL form is not an identifier");
    assert_error_kind(&error, McpErrorKind::InvalidArgument);
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
    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "paper identity".to_string(),
        }
    );
    let run_id = begun["run_id"].as_str().expect("run_id").to_string();

    let json = invoke_tool!(
        server,
        resolve_paper,
        ResolvePaperRequest {
            query: Some("doi:10.1038/s41586-024-00000-x".to_string()),
            title: None,
            run_id: Some(run_id.clone()),
        }
    );
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

    let manifest = invoke_tool!(server, get_research_run, GetResearchRunRequest { run_id });
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
    let json = invoke_tool!(
        server,
        resolve_paper,
        ResolvePaperRequest {
            query: None,
            title: Some(
                "A Recipe For Arbitrary Text Style Transfer with Large Language Models".to_string(),
            ),
            run_id: None,
        }
    );
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
    let json = invoke_tool!(
        server,
        resolve_paper,
        ResolvePaperRequest {
            query: None,
            title: Some("Computing Machinery and Intelligence".to_string()),
            run_id: None,
        }
    );
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
async fn resolve_paper_title_mode_classifies_crossref_429_as_rate_limited() {
    // The 2026-11-02 incident: 5 parallel run-scoped resolve_paper
    // title calls, all Crossref-429'd (bypassing the response cache),
    // each classified `failed_precondition` — the reliability breakdown
    // read "tool defect" instead of "externally throttled," and the
    // sensor degraded to 0.75 on transient throttling. The fix: a
    // Crossref 429 surfaces as `RateLimited` — still degrades (the tool
    // genuinely failed for the caller) but the breakdown names the
    // remedy: back off, don't diagnose the tool.
    let server = make_server_with_pool(Arc::new(RateLimitedTitlePool));
    let error = err(server
        .resolve_paper(Parameters(ResolvePaperRequest {
            query: None,
            title: Some("A Rate Limited Search".to_string()),
            run_id: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::RateLimited);
    assert!(
        error.message.contains("Crossref rate limited"),
        "names the external throttle: {}",
        error.message
    );
}

#[tokio::test]
async fn resolve_paper_title_mode_classifies_transport_failure_as_unavailable() {
    // A Crossref transport failure (connection refused, timeout) is a
    // transient external failure — the config-gap class the ledger
    // excludes from the success-rate math, never a tool defect.
    let server = make_server_with_pool(Arc::new(UnavailableTitlePool));
    let error = err(server
        .resolve_paper(Parameters(ResolvePaperRequest {
            query: None,
            title: Some("An Unavailable Search".to_string()),
            run_id: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::Unavailable);
    assert!(
        error.message.contains("Crossref request failed"),
        "names the transport failure: {}",
        error.message
    );
}

#[tokio::test]
async fn resolve_paper_title_mode_classifies_a_crossref_error_response_as_internal() {
    // A Crossref error response (HTTP 500) carries the kind every other
    // tool here gives a provider error. It was `failed_precondition`,
    // which tells the caller a setup step is missing.
    let server = make_server_with_pool(Arc::new(ProviderErrorTitlePool));
    let error = err(server
        .resolve_paper(Parameters(ResolvePaperRequest {
            query: None,
            title: Some("A Provider Error Search".to_string()),
            run_id: None,
        }))
        .await);
    assert_error_kind(&error, McpErrorKind::Internal);
    assert!(
        error.message.contains("Crossref error 500"),
        "names the protocol failure: {}",
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
    let json = invoke_tool!(
        server,
        web_extract,
        ExtractRequest {
            url: "https://example.com/js-shell".to_string(),
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
            run_id: None,
        }
    );
    let note = json["note"].as_str().expect("degradation note present");
    assert!(
        note.contains("JS shell") && note.contains("Wayback"),
        "note names the likely cause and the alternative route: {note}"
    );
    // The degraded extraction is not cached: the second call re-fetches
    // (the stub returns the same shell, but through a fresh fetch — the
    // cache would have replayed the first response unchanged, which the
    // note's presence on the second call also demonstrates).
    let json2 = invoke_tool!(
        server,
        web_extract,
        ExtractRequest {
            url: "https://example.com/js-shell".to_string(),
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
            run_id: None,
        }
    );
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
    let json = invoke_tool!(
        server,
        web_extract,
        ExtractRequest {
            url: "https://example.com/gone".to_string(),
            format: None,
            json_prompt: None,
            json_schema: None,
            main_content_only: None,
            wait_for_ms: None,
            run_id: None,
        }
    );
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
    let json = parse_envelope(&ok(server.web_ping().await));
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
        Box::pin(async move {
            Ok(hkask_types::EmbeddingBatch {
                vectors,
                requested_model: String::new(),
                actual_model: None,
                usage: hkask_types::InferenceUsage::default(),
            })
        })
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
        None,
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

/// Run one semantic `evaluate_evidence` pass and return the surfaced
/// degradation reason — the shared preamble of the shingle-floor tests (the
/// mode assert pins that the semantic tier degraded, never errored).
async fn semantic_shingles_reason(server: &ResearchServer) -> String {
    let json = invoke_tool!(
        server,
        evaluate_evidence,
        semantic_request(Some("semantic"))
    );
    assert_eq!(json["set"]["duplication_mode"].as_str(), Some("shingles"));
    json["set"]["duplication_reason"]
        .as_str()
        .expect("degradation reason surfaced")
        .to_string()
}

#[tokio::test]
async fn evaluate_evidence_semantic_without_model_degrades_to_shingles_with_reason() {
    // No embedding model configured: the tier degrades to the deterministic
    // floor with a reason naming the setting — never silent, never an error.
    let server = make_server_with_embedding(Arc::new(EmbeddingInferencePort), None);
    let reason = semantic_shingles_reason(&server).await;
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
    let reason = semantic_shingles_reason(&server).await;
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
    let json = invoke_tool!(
        server,
        evaluate_evidence,
        semantic_request(Some("semantic"))
    );
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

/// An embed-RECORDING inference port: every embed call records the MODEL
/// string it was called with — the model string IS the routing signal, so
/// the provenance pins assert which model leg served the call.
struct ModelRecordingEmbedPort {
    embed_models: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

stub_inference_port! {
    ModelRecordingEmbedPort,
    generate_msg: "stub: generate unused in duplication tests",
    rerank(self, _model, _query, _documents): {
        Box::pin(async {
            Err(InferenceError::Connection(
                "stub: rerank unused in duplication tests".to_string(),
            ))
        })
    },
    embed(self, _model, _texts): {
        self.embed_models
            .lock()
            .expect("embed model log")
            .push(_model.to_string());
        let vectors: Vec<Vec<f32>> = _texts.iter().map(|_| vec![1.0_f32, 0.0]).collect();
        Box::pin(async move {
            Ok(hkask_types::EmbeddingBatch {
                vectors,
                requested_model: String::new(),
                actual_model: None,
                usage: hkask_types::InferenceUsage::default(),
            })
        })
    }
}

/// zed-kask: D100 (operator-approved 2026-11-06, the tool-query-embed
/// design's §8 legs) — the provenance-routed duplication tier: a
/// private-provenance call embeds its artifact content through the
/// private leg's LOCAL model ONLY (the recorded model string is the
/// private one; the configured cloud embedder never sees the content);
/// the semantic tier runs (no degradation). The explicit public control
/// proves the tier does not over-block: the same server routes the
/// public call through the configured model as before.
#[tokio::test]
async fn private_call_dedup_tier_embeds_via_the_private_model() {
    let embed_models = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = test_server(
        Arc::new(NoCredentialsPool),
        10,
        None,
        Some(Arc::new(ModelRecordingEmbedPort {
            embed_models: embed_models.clone(),
        })),
        None,
        Some("test-configured-embed"),
        Some("test-private-embed"),
    );
    let output = parse_envelope(&ok(hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        server.evaluate_evidence(Parameters(semantic_request(Some("semantic")))),
    )
    .await));
    assert_eq!(
        output["set"]["duplication_mode"].as_str(),
        Some("semantic"),
        "the private call keeps the semantic tier (no degradation): {output}"
    );
    assert!(
        output["set"].get("duplication_reason").is_none(),
        "no degradation for a routed private call: {output}"
    );
    assert_eq!(
        embed_models.lock().expect("embed model log").as_slice(),
        &["test-private-embed".to_string()],
        "the private call embeds through the PRIVATE model string only"
    );

    // The public control: the same server routes a public call through the
    // configured model — the tier is provenance-scoped, never a blanket
    // refusal.
    let output = parse_envelope(&ok(hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PublicProvider,
        server.evaluate_evidence(Parameters(semantic_request(Some("semantic")))),
    )
    .await));
    assert_eq!(output["set"]["duplication_mode"].as_str(), Some("semantic"));
    assert_eq!(
        embed_models.lock().expect("embed model log").as_slice(),
        &[
            "test-private-embed".to_string(),
            "test-configured-embed".to_string(),
        ],
        "the public control routes through the CONFIGURED model as before"
    );
}

/// The private opt-out degrade: no private embedding model configured →
/// the private call NEVER embeds (the configured cloud model is not
/// consulted) and the shingle floor runs with the surfaced reason naming
/// the private setting.
#[tokio::test]
async fn private_call_dedup_degrades_to_the_shingle_floor_when_no_private_model() {
    let embed_models = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = test_server(
        Arc::new(NoCredentialsPool),
        10,
        None,
        Some(Arc::new(ModelRecordingEmbedPort {
            embed_models: embed_models.clone(),
        })),
        None,
        Some("test-configured-embed"),
        None,
    );
    let output = parse_envelope(&ok(hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        server.evaluate_evidence(Parameters(semantic_request(Some("semantic")))),
    )
    .await));
    assert_eq!(
        output["set"]["duplication_mode"].as_str(),
        Some("shingles"),
        "the opt-out degrades to the deterministic floor: {output}"
    );
    let reason = output["set"]["duplication_reason"]
        .as_str()
        .expect("the opt-out degrade must surface its reason");
    assert!(
        reason.contains("no private embedding model")
            && reason.contains("HKASK_PRIVATE_EMBEDDING_MODEL"),
        "the reason names the private setting: {reason}"
    );
    assert!(
        embed_models.lock().expect("embed model log").is_empty(),
        "the opted-out private call makes ZERO embed calls — the configured \
         cloud model is never consulted"
    );
}

#[tokio::test]
async fn evaluate_evidence_accepts_quoted_duplication_mode() {
    // `"semantic"` with literal quote characters normalizes and proceeds
    // past the closed-set gate — same degradation path as the bare form
    // (no embedding model → shingle floor with the surfaced reason).
    let server = make_server_with_embedding(Arc::new(EmbeddingInferencePort), None);
    let json = invoke_tool!(
        server,
        evaluate_evidence,
        semantic_request(Some("\"semantic\""))
    );
    assert_eq!(json["set"]["duplication_mode"].as_str(), Some("shingles"));
    assert!(
        json["set"].get("duplication_reason").is_some(),
        "degradation reason surfaced: {json}"
    );
}

#[tokio::test]
async fn finish_research_run_transitions_and_preserves_the_sources_ledger() {
    let server = make_server_with_research_db();
    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "is the wire story corroborated?".to_string(),
        }
    );
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
    let finished = invoke_tool!(
        server,
        finish_research_run,
        FinishResearchRunRequest {
            run_id: run_id.clone(),
            status: "blocked".to_string(),
            note: Some("no sources reachable".to_string()),
        }
    );
    assert_eq!(finished["status"].as_str(), Some("blocked"));
    assert_eq!(finished["from_status"].as_str(), Some("planned"));
    assert_eq!(finished["note"].as_str(), Some("no sources reachable"));

    // The manifest reflects the terminal status AND its transition history;
    // the sources ledger is untouched.
    let manifest = invoke_tool!(
        server,
        get_research_run,
        GetResearchRunRequest {
            run_id: run_id.clone(),
        }
    );
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

    let begun = invoke_tool!(
        server,
        begin_research_run,
        BeginResearchRunRequest {
            question: "another question".to_string(),
        }
    );
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
            .contains("completed, partial, blocked, failed"),
        "message names the terminal vocabulary: {}",
        nonterminal.message
    );
}
