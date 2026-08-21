//! hkask-mcp-research — Web search, extraction, browsing, and RSS feed management.
//!
//! MCP server crate containing the research tool surface and business logic:
//! provider pool with RRF fusion, content extraction, headless browsing,
//! RSS feed management, response caching, and rate limiting.

pub(crate) mod cache;
pub(crate) mod db;
pub(crate) mod feed;
pub(crate) mod providers;
pub(crate) mod rss_types;
pub(crate) mod strip_html;
pub(crate) mod synthetic;
pub(crate) mod types;

use std::collections::HashMap;

use providers::{
    ArxivProvider, BraveProvider, BrowserbaseProvider, FirecrawlProvider, RawFetchProvider,
    SemanticScholarProvider, SerapiProvider, TavilyProvider, WebBrowseProvider, WebExtractProvider,
    WebSearchProvider,
};

// ── Re-exports ──

pub(crate) use cache::{ResponseCache, cache_key};
pub(crate) use feed::{discover_feeds, fetch_feed};
pub(crate) use providers::{ExaProvider, ProviderPool, WebSearchPort};
pub(crate) use rss_types::{
    Continuation, DeleteSyntheticRequest, DiscoverRequest, EditTagRequest, FetchRequest,
    FetchSyntheticRequest, GetEntriesRequest, ImportOpmlRequest, ListSubscriptionsRequest,
    MarkReadRequest, SubscribeRequest, SynthesizeRequest, UnreadCountRequest, UnsubscribeRequest,
};
pub(crate) use types::RateLimiter;
pub(crate) use types::{
    BrowseOutput, BrowseRequest, CiteSourcesRequest, CiteStyle, DEFAULT_CACHE_MAX_ENTRIES,
    DEFAULT_CACHE_TTL_SECS, EvaluateEvidenceRequest, ExtractOptions, ExtractOutput, ExtractRequest,
    FindSimilarOutput, FindSimilarRequest, FindSimilarResultOutput, MAX_CACHE_MAX_ENTRIES,
    MAX_CACHE_TTL_SECS, MAX_INSTRUCTION_LENGTH, MAX_JSON_PROMPT_LENGTH, MAX_JSON_SCHEMA_BYTES,
    MAX_QUERY_LENGTH, MAX_URL_LENGTH, PingOutput, SearchMetadata, SearchOutput, SearchQuery,
    SearchRequest, SearchResultOutput, SearchStrategy, WebError,
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
    let browserbase_api_key = credentials.get("HKASK_BROWSERBASE_API_KEY").cloned();

    let mut search_providers: Vec<Box<dyn WebSearchProvider>> = Vec::new();
    let mut extract_providers: Vec<Box<dyn WebExtractProvider>> = Vec::new();
    let mut browse_providers: Vec<Box<dyn WebBrowseProvider>> = Vec::new();

    // Free providers — no API key required
    search_providers.push(Box::new(SemanticScholarProvider::new()?));
    search_providers.push(Box::new(ArxivProvider::new()?));

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
        extract_providers.push(Box::new(fc.clone()));
        browse_providers.push(Box::new(fc));
    }
    if let Some(ref key) = tavily_api_key {
        search_providers.push(Box::new(TavilyProvider::new(key.clone())?));
    }
    if let Some(ref key) = serpapi_api_key {
        search_providers.push(Box::new(SerapiProvider::new(key.clone())?));
    }
    if let Some(ref exa) = exa_provider {
        search_providers.push(Box::new(exa.clone()));
    }
    if let Some(ref key) = browserbase_api_key {
        browse_providers.push(Box::new(BrowserbaseProvider::new(key.clone())?));
    }

    extract_providers.push(Box::new(RawFetchProvider::new()?));

    Ok(ProviderPool::new(
        search_providers,
        extract_providers,
        browse_providers,
        exa_provider,
    ))
}
