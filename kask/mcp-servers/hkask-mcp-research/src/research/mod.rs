//! hkask-services-research — Web search, extraction, browsing, and RSS feed management.
//!
//! Service crate containing the business logic for the research MCP server:
//! provider pool with RRF fusion, content extraction, headless browsing,
//! RSS feed management, response caching, and rate limiting.
//!
//! The MCP server crate (`hkask-mcp-research`) is a thin tool surface that
//! delegates to this service crate.

pub mod cache;
pub mod db;
pub mod feed;
pub mod providers;
pub mod rss_types;
pub mod strip_html;
pub mod types;

use std::collections::HashMap;

use providers::{
    ArxivProvider, BraveProvider, BrowserbaseProvider, FirecrawlProvider, RawFetchProvider,
    SemanticScholarProvider, SerapiProvider, TavilyProvider, WebBrowseProvider, WebExtractProvider,
    WebSearchProvider,
};

// ── Re-exports ──

pub use cache::{CacheKey, ResponseCache, cache_key};
pub use feed::{discover_feeds, fetch_feed};
pub use providers::{ExaProvider, ProviderPool, WebSearchPort};
pub use rss_types::{
    Continuation, DiscoverRequest, EditTagRequest, FetchRequest, FetchResult, GetEntriesRequest,
    ImportOpmlRequest, ListSubscriptionsRequest, MarkReadRequest, SubscribeRequest,
    UnreadCountRequest, UnsubscribeRequest,
};
pub use types::RateLimiter;
pub use types::{
    AnswerBox, BrowseOutput, BrowseRequest, BrowseResult, COMPOUND_PROVIDER_TIMEOUT_SECS,
    CompoundSearchResult, DEFAULT_CACHE_MAX_ENTRIES, DEFAULT_CACHE_TTL_SECS,
    DEFAULT_REQUEST_TIMEOUT_SECS, ExtractOptions, ExtractOutput, ExtractRequest, ExtractedContent,
    FindSimilarOutput, FindSimilarRequest, FindSimilarResultOutput, MAX_CACHE_MAX_ENTRIES,
    MAX_CACHE_TTL_SECS, MAX_CACHE_VALUE_BYTES, MAX_INSTRUCTION_LENGTH, MAX_JSON_PROMPT_LENGTH,
    MAX_JSON_SCHEMA_BYTES, MAX_QUERY_LENGTH, MAX_URL_LENGTH, PingOutput, ProviderError,
    ProviderFilter, ProviderHealthEntry, ProviderInfo, RATE_LIMIT_MAX_REQUESTS,
    RATE_LIMIT_WINDOW_SECS, RRF_K, RankedResult, RerankSignal, SearchCapability, SearchMetadata,
    SearchOutput, SearchQuery, SearchRequest, SearchResult, SearchResultOutput, SearchStrategy,
    WebError,
};

/// Build a `ProviderPool` from a credential map.
///
/// Free providers (SemanticScholar, arXiv, RawFetch) are always included.
/// API-key providers are included when their credential is present.
/// Returns `WebError::ProviderUnavailable` if no search providers are configured.
pub fn build_provider_pool(
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
    search_providers.push(Box::new(SemanticScholarProvider::new()));
    search_providers.push(Box::new(ArxivProvider::new()));

    let exa_provider = exa_api_key
        .as_ref()
        .map(|key| ExaProvider::new(key.clone()));

    if let Some(ref key) = brave_api_key {
        search_providers.push(Box::new(BraveProvider::new(key.clone())));
    }
    if let Some(ref key) = firecrawl_api_key {
        let fc = FirecrawlProvider::new(Some(key.clone()));
        search_providers.push(Box::new(fc.clone()));
        extract_providers.push(Box::new(fc.clone()));
        browse_providers.push(Box::new(fc));
    }
    if let Some(ref key) = tavily_api_key {
        search_providers.push(Box::new(TavilyProvider::new(key.clone())));
    }
    if let Some(ref key) = serpapi_api_key {
        search_providers.push(Box::new(SerapiProvider::new(key.clone())));
    }
    if let Some(ref exa) = exa_provider {
        search_providers.push(Box::new(exa.clone()));
    }
    if let Some(ref key) = browserbase_api_key {
        browse_providers.push(Box::new(BrowserbaseProvider::new(key.clone())));
    }

    extract_providers.push(Box::new(RawFetchProvider::new()));

    if search_providers.is_empty() {
        return Err(WebError::ProviderUnavailable(
            "No search providers configured. Set at least one of: HKASK_BRAVE_API_KEY, \
             HKASK_FIRECRAWL_API_KEY, HKASK_TAVILY_API_KEY, HKASK_SERPAPI_API_KEY, \
             HKASK_EXA_API_KEY"
                .into(),
        ));
    }

    Ok(ProviderPool::new(
        search_providers,
        extract_providers,
        browse_providers,
        exa_provider,
    ))
}
