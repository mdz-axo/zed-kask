use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;

use crate::research::types::*;
use hkask_mcp_server::server::validate_tool_url;

mod arxiv;
mod brave;
mod browserbase;
mod exa;
mod firecrawl;
mod raw_fetch;
mod semantic_scholar;
mod serapi;
mod tavily;

pub use arxiv::ArxivProvider;
pub use brave::BraveProvider;
pub use browserbase::BrowserbaseProvider;
pub use exa::ExaProvider;
pub use firecrawl::FirecrawlProvider;
pub use raw_fetch::{RawFetchProvider, truncate_str};
pub use semantic_scholar::SemanticScholarProvider;
pub use serapi::SerapiProvider;
pub use tavily::TavilyProvider;

/// Build the shared HTTP client used by all research providers.
///
/// Applies a consistent user-agent and request timeout, eliminating the repeated
/// `reqwest::Client::builder()...build().expect(...)` boilerplate across providers.
pub(super) fn provider_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!("hkask-mcp-web/{SERVER_VERSION}"))
        .timeout(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
        .build()
        .expect("reqwest client builder is infallible with these settings")
}

#[derive(Default)]
pub struct ProviderSearchOutput {
    pub results: Vec<SearchResult>,
    pub answer_box: Option<AnswerBox>,
    pub related_questions: Vec<String>,
    pub content_previews: HashMap<String, String>,
    pub semantic_scores: HashMap<String, f64>,
}

#[async_trait]
pub(crate) trait WebSearchProvider: Send + Sync {
    fn kind(&self) -> &str;
    fn capabilities(&self) -> Vec<SearchCapability>;
    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError>;
    async fn health(&self) -> Result<(), WebError>;
}

/// Validate a URL for SSRF safety before making outbound requests.
///
/// Wraps the shared `validate_tool_url` from `hkask-mcp` and converts the error
/// to `WebError`. Used by `RawFetchProvider` for defense-in-depth URL validation.
pub fn validate_provider_url(url: &str) -> Result<(), WebError> {
    validate_tool_url(url).map_err(|e| WebError::BadArgs(e.message))
}

/// Validate a URL with permissive SSRF config (allows private IPs and loopback).
///
/// Used by RSS tools (`rss_fetch`, `import_opml`) where the user has
/// explicitly subscribed to a feed that may be on a local network (e.g.,
/// a self-hosted RSS aggregator at `http://localhost:4000/feed.xml`).
/// The strict variant (`validate_provider_url`) is used for arbitrary
/// user-supplied URLs (`web_extract`, `web_browse`, `discover_feeds`).
pub fn validate_provider_url_permissive(url: &str) -> Result<(), WebError> {
    hkask_mcp_server::server::validate_tool_url_permissive(url)
        .map_err(|e| WebError::BadArgs(e.message))
}

/// Port trait for web search operations at the application core boundary.
///
/// Tool handlers depend on this trait; `ProviderPool` implements it as the
/// adapter. This keeps provider-specific details (like `pool.exa` direct
/// access) out of the tool layer.
#[async_trait]
pub trait WebSearchPort: Send + Sync {
    async fn search(
        &self,
        query: &SearchQuery,
        strategy: SearchStrategy,
    ) -> Result<CompoundSearchResult, WebError>;
    async fn find_similar(
        &self,
        url: &str,
        num_results: u32,
    ) -> Result<ProviderSearchOutput, WebError>;
    async fn extract(&self, url: &str, opts: &ExtractOptions)
    -> Result<ExtractedContent, WebError>;
    async fn browse(
        &self,
        url: &str,
        instruction: &str,
        timeout: Duration,
    ) -> Result<BrowseResult, WebError>;
    async fn health_check(&self) -> Vec<ProviderHealthEntry>;
    fn provider_fingerprint(&self) -> String;
}

#[async_trait]
pub(crate) trait WebExtractProvider: Send + Sync {
    fn kind(&self) -> &str;
    async fn extract(&self, url: &str, opts: &ExtractOptions)
    -> Result<ExtractedContent, WebError>;
    async fn health(&self) -> Result<(), WebError>;
}

#[async_trait]
pub(crate) trait WebBrowseProvider: Send + Sync {
    fn kind(&self) -> &str;
    async fn browse(
        &self,
        url: &str,
        instruction: &str,
        timeout: Duration,
    ) -> Result<BrowseResult, WebError>;
    async fn health(&self) -> Result<(), WebError>;
}

pub struct ProviderPool {
    pub(crate) search_providers: Vec<Box<dyn WebSearchProvider>>,
    pub(crate) extract_providers: Vec<Box<dyn WebExtractProvider>>,
    pub(crate) browse_providers: Vec<Box<dyn WebBrowseProvider>>,
    pub(crate) exa: Option<ExaProvider>,
}

/// Try each provider sequentially, returning first Ok or last Err.
macro_rules! try_fallback {
    ($providers:expr, $call:ident, $($arg:expr),* $(,)?) => {{
        let mut last_err = WebError::NoProvider;
        for p in $providers {
            match p.$call($($arg,)*).await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    tracing::warn!(provider = p.kind(), error = %e);
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }};
}

impl ProviderPool {
    /// Construct a new `ProviderPool` with the given providers.
    ///
    /// This is the authoritative constructor — all pool creation should go through
    /// here rather than setting fields directly, to maintain the hexagonal boundary.
    pub(crate) fn new(
        search_providers: Vec<Box<dyn WebSearchProvider>>,
        extract_providers: Vec<Box<dyn WebExtractProvider>>,
        browse_providers: Vec<Box<dyn WebBrowseProvider>>,
        exa: Option<ExaProvider>,
    ) -> Self {
        Self {
            search_providers,
            extract_providers,
            browse_providers,
            exa,
        }
    }

    /// Fallback loop: try each search provider, return first Ok results.
    async fn search_fallback(
        providers: &[&dyn WebSearchProvider],
        query: &SearchQuery,
    ) -> Result<Vec<SearchResult>, WebError> {
        let mut last_err = WebError::NoProvider;
        for p in providers {
            match p.search(query).await {
                Ok(v) => return Ok(v.results),
                Err(e) => {
                    tracing::warn!(provider = p.kind(), error = %e);
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }
}

impl ProviderPool {
    pub async fn search_with_fallback(
        &self,
        query: &SearchQuery,
        primary: &str,
    ) -> Result<Vec<SearchResult>, WebError> {
        let mut ordered: Vec<&dyn WebSearchProvider> =
            self.search_providers.iter().map(|p| p.as_ref()).collect();
        if let Some(idx) = ordered.iter().position(|p| p.kind() == primary) {
            ordered.swap(0, idx);
        }
        Self::search_fallback(&ordered, query).await
    }

    pub async fn search_by_capability(
        &self,
        query: &SearchQuery,
        required_caps: &[SearchCapability],
    ) -> Result<Vec<SearchResult>, WebError> {
        let filtered: Vec<&dyn WebSearchProvider> = self
            .search_providers
            .iter()
            .filter(|p| required_caps.iter().all(|c| p.capabilities().contains(c)))
            .map(|p| p.as_ref())
            .collect();
        Self::search_fallback(&filtered, query).await
    }

    pub async fn search_compound(
        &self,
        query: &SearchQuery,
        strategy: SearchStrategy,
    ) -> CompoundSearchResult {
        let filtered: Vec<&dyn WebSearchProvider> = match strategy.provider_filter() {
            ProviderFilter::All => self.search_providers.iter().map(|p| p.as_ref()).collect(),
            ProviderFilter::Capabilities(caps) => self
                .search_providers
                .iter()
                .filter(|p| {
                    let p_caps = p.capabilities();
                    caps.iter().all(|c| p_caps.contains(c))
                })
                .map(|p| p.as_ref())
                .collect(),
        };

        let providers_queried: Vec<ProviderInfo> = filtered
            .iter()
            .map(|p| ProviderInfo {
                kind: p.kind().to_string(),
                capabilities: p.capabilities(),
            })
            .collect();

        let futures: Vec<_> = filtered
            .iter()
            .map(|p| async {
                let kind = p.kind().to_string();
                match tokio::time::timeout(
                    Duration::from_secs(COMPOUND_PROVIDER_TIMEOUT_SECS),
                    p.search(query),
                )
                .await
                {
                    Ok(result) => (kind, result),
                    Err(_) => {
                        tracing::warn!(
                            provider = %kind,
                            timeout_secs = COMPOUND_PROVIDER_TIMEOUT_SECS,
                            "Compound search provider timed out"
                        );
                        (
                            kind,
                            Err(WebError::ProviderUnavailable(format!(
                                "Provider timed out after {COMPOUND_PROVIDER_TIMEOUT_SECS}s"
                            ))),
                        )
                    }
                }
            })
            .collect();

        let results = futures_util::future::join_all(futures).await;

        let mut succeeded: Vec<String> = Vec::new();
        let mut failed: Vec<ProviderError> = Vec::new();
        let mut all_results: Vec<(String, usize, SearchResult)> = Vec::new();
        let mut merged_answer_box: Option<AnswerBox> = None;
        let mut merged_related_questions: Vec<String> = Vec::new();
        let mut merged_content_previews: HashMap<String, String> = HashMap::new();
        let mut merged_semantic_scores: HashMap<String, f64> = HashMap::new();

        for (kind, result) in results {
            match result {
                Ok(output) => {
                    for (rank, item) in output.results.into_iter().enumerate() {
                        all_results.push((kind.clone(), rank, item));
                    }
                    if output.answer_box.is_some() && merged_answer_box.is_none() {
                        merged_answer_box = output.answer_box;
                    }
                    merged_related_questions.extend(output.related_questions);
                    merged_content_previews.extend(output.content_previews);
                    merged_semantic_scores.extend(output.semantic_scores);
                    succeeded.push(kind);
                }
                Err(e) => {
                    tracing::warn!(provider = %kind, error = %e, "Compound search provider failed");
                    failed.push(ProviderError {
                        kind: kind.clone(),
                        error: e.to_string(),
                    });
                }
            }
        }

        let total_before_dedup = all_results.len();

        struct UrlEntry {
            url_original: String,
            title: String,
            description: Option<String>,
            source: Option<String>,
            published: Option<String>,
            providers: Vec<String>,
            ranks: Vec<usize>,
        }

        let mut url_map: HashMap<String, UrlEntry> = HashMap::new();

        for (provider, rank, result) in all_results {
            let key = result.url.to_lowercase();
            match url_map.get_mut(&key) {
                Some(entry) => {
                    entry.providers.push(provider.clone());
                    entry.ranks.push(rank);
                    // Always prefer academic sources over web/search-engine sources
                    let is_academic = matches!(
                        result.source.as_deref(),
                        Some("arXiv") | Some("arxiv") | Some("semantic_scholar")
                    );
                    if is_academic && result.source.is_some() {
                        entry.source = result.source.clone();
                    }
                }
                None => {
                    url_map.insert(
                        key,
                        UrlEntry {
                            url_original: result.url,
                            title: result.title,
                            description: result.description,
                            source: result.source,
                            published: result.published,
                            providers: vec![provider],
                            ranks: vec![rank],
                        },
                    );
                }
            }
        }

        let mut ranked: Vec<RankedResult> = url_map
            .into_iter()
            .map(|(key, entry)| {
                let provider_count = entry.providers.len();
                let best_rank = *entry.ranks.iter().min().unwrap_or(&0);
                let content_preview = merged_content_previews.get(&key).cloned();
                let semantic_score = merged_semantic_scores.get(&key).copied();
                let rrf_score = rrf_score(RRF_K, &entry.ranks);

                RankedResult {
                    title: entry.title,
                    url: entry.url_original,
                    description: entry.description,
                    source: entry.source,
                    published: entry.published,
                    rrf_score,
                    provider_count,
                    providers: entry.providers,
                    best_rank: Some(best_rank),
                    content_preview,
                    semantic_score,
                    extracted_content: None,
                }
            })
            .collect();

        ranked.sort_by(|a, b| {
            b.rrf_score
                .partial_cmp(&a.rrf_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let duplicates_removed = total_before_dedup - ranked.len();

        CompoundSearchResult {
            query: query.query.clone(),
            strategy: strategy.to_string(),
            results: ranked,
            answer_box: merged_answer_box,
            related_questions: merged_related_questions,
            providers_queried,
            providers_succeeded: succeeded,
            providers_failed: failed,
            total_before_dedup,
            duplicates_removed,
        }
    }

    pub async fn find_similar(
        &self,
        url: &str,
        num_results: u32,
    ) -> Result<ProviderSearchOutput, WebError> {
        match self.exa {
            Some(ref exa) => exa.find_similar(url, num_results).await,
            None => Err(WebError::NoProvider),
        }
    }

    pub async fn extract_with_fallback(
        &self,
        url: &str,
        opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        try_fallback!(&self.extract_providers, extract, url, opts)
    }

    pub async fn browse_with_fallback(
        &self,
        url: &str,
        instruction: &str,
        timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        try_fallback!(&self.browse_providers, browse, url, instruction, timeout)
    }

    pub fn search_provider_kinds(&self) -> Vec<String> {
        self.search_providers
            .iter()
            .map(|p| p.kind().to_string())
            .collect()
    }

    pub fn extract_provider_kinds(&self) -> Vec<String> {
        self.extract_providers
            .iter()
            .map(|p| p.kind().to_string())
            .collect()
    }

    pub fn browse_provider_kinds(&self) -> Vec<String> {
        self.browse_providers
            .iter()
            .map(|p| p.kind().to_string())
            .collect()
    }

    pub fn provider_fingerprint(&self) -> String {
        let mut kinds: Vec<String> = self.search_provider_kinds();
        kinds.extend(self.extract_provider_kinds());
        kinds.extend(self.browse_provider_kinds());
        if self.exa.is_some() {
            kinds.push("exa-similar".into());
        }
        kinds.sort();
        kinds.join(",")
    }

    pub async fn health_check_all(&self) -> Vec<ProviderHealthEntry> {
        let mut entries = Vec::new();
        macro_rules! health_them {
            ($provs:expr) => {
                for p in $provs {
                    let k = p.kind().to_string();
                    let r = p.health().await;
                    entries.push(health_entry(k, r));
                }
            };
        }
        health_them!(&self.search_providers);
        health_them!(&self.extract_providers);
        health_them!(&self.browse_providers);
        if let Some(ref exa) = self.exa {
            let r = exa.health().await;
            entries.push(health_entry("exa-similar".into(), r));
        }
        entries
    }
}

fn health_entry(kind: String, result: Result<(), WebError>) -> ProviderHealthEntry {
    ProviderHealthEntry {
        kind,
        healthy: result.is_ok(),
        error: result.err().map(|e| sanitize_health_error(&e.to_string())),
    }
}

// WebSearchPort implementation - ProviderPool as the adapter
// WebSearchPort implementation - ProviderPool as the adapter

#[async_trait]
impl WebSearchPort for ProviderPool {
    async fn search(
        &self,
        query: &SearchQuery,
        strategy: SearchStrategy,
    ) -> Result<CompoundSearchResult, WebError> {
        // N1: CapabilityContext removed; OCAP is enforced at the dispatcher
        // membrane (GovernedTool), not at the port.
        if query.query.is_empty() {
            return Err(WebError::BadArgs("query must not be empty".into()));
        }
        if query.query.len() > MAX_QUERY_LENGTH {
            return Err(WebError::BadArgs(format!(
                "query exceeds maximum length of {} characters",
                MAX_QUERY_LENGTH
            )));
        }

        let mut compound = if strategy == SearchStrategy::Quick {
            let results = self
                .search_by_capability(query, &[SearchCapability::Keyword])
                .await?;
            let total = results.len();
            let pname = self
                .search_providers
                .iter()
                .find(|p| p.capabilities().contains(&SearchCapability::Keyword))
                .map(|p| p.kind())
                .unwrap_or("unknown")
                .to_string();
            CompoundSearchResult {
                query: query.query.clone(),
                strategy: strategy.to_string(),
                results: results
                    .into_iter()
                    .enumerate()
                    .map(|(i, r)| RankedResult {
                        rrf_score: rrf_score(RRF_K, &[i]),
                        provider_count: 1,
                        providers: vec![pname.clone()],
                        best_rank: Some(i),
                        extracted_content: None,
                        content_preview: None,
                        semantic_score: None,
                        title: r.title,
                        url: r.url,
                        description: r.description,
                        source: r.source,
                        published: r.published,
                    })
                    .collect(),
                providers_queried: vec![ProviderInfo {
                    kind: pname.clone(),
                    capabilities: vec![SearchCapability::Keyword],
                }],
                providers_succeeded: vec![pname],
                answer_box: None,
                related_questions: Vec::new(),
                providers_failed: Vec::new(),
                total_before_dedup: total,
                duplicates_removed: 0,
            }
        } else {
            // N4: before dispatching a compound search, verify the strategy's
            // provider filter actually matches at least one configured provider.
            // Without this, `strategy: "news"` silently returns 0 results when
            // no News-capable provider has an API key (Brave/SerpAPI absent),
            // and the user sees an empty result with no explanation.
            if let ProviderFilter::Capabilities(ref caps) = strategy.provider_filter() {
                let has_match = self.search_providers.iter().any(|p| {
                    let p_caps = p.capabilities();
                    caps.iter().all(|c| p_caps.contains(c))
                });
                if !has_match {
                    return Err(WebError::ProviderUnavailable(format!(
                        "No providers configured for strategy '{strategy}'. \
                         Required capabilities: {:?}. Set the corresponding API key.",
                        caps
                    )));
                }
            }
            // Deep strategy: request more results from each provider for a broader
            // RRF candidate pool, giving fusion more signal to dedup and rank.
            let search_query = if strategy == SearchStrategy::Deep {
                SearchQuery {
                    num_results: query.num_results.saturating_mul(2).min(50),
                    ..query.clone()
                }
            } else {
                query.clone()
            };
            self.search_compound(&search_query, strategy).await
        };

        apply_rerank(&mut compound.results, RerankSignal::Recency);
        apply_rerank(&mut compound.results, RerankSignal::Semantic);
        apply_rerank(&mut compound.results, RerankSignal::ContentQuality);

        // Deep strategy: extract content from top results to enrich the response.
        // This populates content_preview, giving users actual page content
        // alongside the link and snippet — the key differentiation from Web.
        if strategy == SearchStrategy::Deep && !compound.results.is_empty() {
            let top_n = compound.results.len().min(3);
            let opts = ExtractOptions {
                format: "markdown".to_string(),
                json_prompt: None,
                json_schema: None,
                main_content_only: true,
                wait_for_ms: 0,
            };
            let top_urls: Vec<String> = compound.results[..top_n]
                .iter()
                .map(|r| r.url.clone())
                .collect();
            let futures: Vec<_> = top_urls
                .into_iter()
                .map(|url| {
                    let opts = opts.clone();
                    async move {
                        match self.extract_with_fallback(&url, &opts).await {
                            Ok(content) => Some((url, content.content)),
                            Err(e) => {
                                tracing::debug!(
                                    url = %url,
                                    error = %e,
                                    "Deep search content extraction failed"
                                );
                                None
                            }
                        }
                    }
                })
                .collect();
            let extracted = futures_util::future::join_all(futures).await;
            for (url, content) in extracted.into_iter().flatten() {
                if let Some(r) = compound.results.iter_mut().find(|r| r.url == url) {
                    let preview: String = content.chars().take(500).collect();
                    r.content_preview = Some(preview);
                }
            }
        }

        Ok(compound)
    }

    async fn find_similar(
        &self,
        url: &str,
        num_results: u32,
    ) -> Result<ProviderSearchOutput, WebError> {
        match self.exa {
            Some(ref exa) => exa.find_similar(url, num_results).await,
            None => Err(WebError::NoProvider),
        }
    }

    async fn extract(
        &self,
        url: &str,
        opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        self.extract_with_fallback(url, opts).await
    }

    async fn browse(
        &self,
        url: &str,
        instruction: &str,
        timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        self.browse_with_fallback(url, instruction, timeout).await
    }

    async fn health_check(&self) -> Vec<ProviderHealthEntry> {
        self.health_check_all().await
    }

    fn provider_fingerprint(&self) -> String {
        ProviderPool::provider_fingerprint(self)
    }
}
