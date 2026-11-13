//! hkask-mcp-research — Web search, extraction, browsing, evidence scoring,
//! paper identity, and the research-run ledger.
//!
//! MCP server crate containing the research tool surface and business logic:
//! provider pool with RRF fusion, content extraction, headless browsing,
//! response caching, and rate limiting.

pub mod apa;
pub(crate) mod breaker;
pub mod cache;
pub mod db;
pub(crate) mod evidence;
pub mod paper_id;
pub(crate) mod performance;
pub mod providers;
pub(crate) mod runs;
pub(crate) mod strip_html;
pub mod types;

use std::collections::HashMap;

use providers::{
    ArxivProvider, BraveProvider, CrossrefProvider, FirecrawlProvider, OpenAlexProvider,
    RawFetchProvider, SemanticScholarProvider, SerapiProvider, SerpEngine, TavilyProvider,
    WebExtractProvider, WebSearchProvider,
};

// ── Re-exports ──

pub(crate) use cache::{ResponseCache, cache_key};
pub(crate) use evidence::{
    ArtifactScore, DEFAULT_PROFILE, EvidenceReport, SensitivityStatus, score_evidence_set,
};
pub(crate) use providers::{ExaProvider, ProviderPool, WebSearchPort};
pub(crate) use runs::{NewResearchRun, RunSourceRecord};
pub(crate) use types::RateLimiter;
pub(crate) use types::{
    AnnotateResearchRunRequest, BeginResearchRunRequest, CiteSourcesRequest,
    DEFAULT_CACHE_MAX_ENTRIES, DEFAULT_CACHE_TTL_SECS, EvaluateEvidenceRequest, ExtractOptions,
    ExtractOutput, ExtractRequest, FindSimilarOutput, FindSimilarRequest, FindSimilarResultOutput,
    FinishResearchRunRequest, GetResearchRunRequest, MAX_CACHE_MAX_ENTRIES, MAX_CACHE_TTL_SECS,
    MAX_JSON_PROMPT_LENGTH, MAX_JSON_SCHEMA_BYTES, MAX_QUERY_LENGTH, MAX_URL_LENGTH, PingOutput,
    ProviderProfileOutput, ProviderRecommendation, RerankInfo, RerankOutcome, ResolvePaperRequest,
    SearchMetadata, SearchOutput, SearchQuery, SearchRequest, SearchResultOutput, SearchStrategy,
    WebError, llm_rerank, normalize_closed_vocab, parse_closed_vocab, parse_closed_vocab_const,
    provider_profile,
};

/// Build a `ProviderPool` from a credential map.
///
/// Free providers (SemanticScholar, arXiv, RawFetch) are always included, so
/// at least one search provider is always present even with no API keys.
/// API-key providers are included when their credential is present.
pub(crate) fn build_provider_pool(
    credentials: &HashMap<String, String>,
) -> Result<ProviderPool, WebError> {
    let brave_api_key = credentials.get("HKASK_BRAVE_API_KEY").cloned();
    let firecrawl_api_key = credentials.get("HKASK_FIRECRAWL_API_KEY").cloned();
    let tavily_api_key = credentials.get("HKASK_TAVILY_API_KEY").cloned();
    let serpapi_api_key = credentials.get("HKASK_SERPAPI_API_KEY").cloned();
    let exa_api_key = credentials.get("HKASK_EXA_API_KEY").cloned();

    let mut search_providers: Vec<Box<dyn WebSearchProvider>> = Vec::new();
    let mut extract_providers: Vec<Box<dyn WebExtractProvider>> = Vec::new();

    // Optional Semantic Scholar key: the keyless free tier shares a ~1
    // req/s global pool (the 2026-11-01 rate-limit storm); a free key from
    // https://www.semanticscholar.org/product/api lifts the limit. The
    // provider is registered either way — keyless is a supported mode.
    let semantic_scholar_api_key = credentials.get("HKASK_SEMANTIC_SCHOLAR_API_KEY").cloned();

    // Free providers — no API key required
    search_providers.push(Box::new(SemanticScholarProvider::new(
        semantic_scholar_api_key,
    )?));
    search_providers.push(Box::new(ArxivProvider::new()?));
    // OpenAlex — free scholarly metadata; also held typed for paper-id
    // resolution (the `exa` pattern: one provider, two surfaces).
    let openalex_provider = OpenAlexProvider::new()?;
    search_providers.push(Box::new(openalex_provider.clone()));

    let exa_provider = exa_api_key
        .as_ref()
        .map(|key| ExaProvider::new(key.clone()))
        .transpose()?;

    if let Some(ref key) = brave_api_key {
        search_providers.push(Box::new(BraveProvider::new(key.clone())?));
    }
    if let Some(ref key) = firecrawl_api_key {
        let fc = FirecrawlProvider::new(Some(key.clone()))?;
        search_providers.push(Box::new(fc.clone()));
        extract_providers.push(Box::new(fc));
    }
    if let Some(ref key) = tavily_api_key {
        search_providers.push(Box::new(TavilyProvider::new(key.clone())?));
    }
    if let Some(ref key) = serpapi_api_key {
        search_providers.push(Box::new(SerapiProvider::new(key.clone())?));
        // Same key; explicit-only engines for scholarly and book search.
        search_providers.push(Box::new(SerapiProvider::with_engine(
            key.clone(),
            SerpEngine::Scholar,
        )?));
        search_providers.push(Box::new(SerapiProvider::with_engine(
            key.clone(),
            SerpEngine::Books,
        )?));
    }
    if let Some(ref exa) = exa_provider {
        search_providers.push(Box::new(exa.clone()));
    }

    extract_providers.push(Box::new(RawFetchProvider::new()?));

    // Crossref — free bibliographic title resolution (no API key); held
    // typed for `resolve_paper`'s title mode, not registered as a search
    // provider (its relevance search is a resolution instrument, not a
    // discovery surface).
    let crossref_provider = CrossrefProvider::new()?;

    Ok(ProviderPool::new(
        search_providers,
        extract_providers,
        exa_provider,
        Some(openalex_provider),
        Some(crossref_provider),
    ))
}
