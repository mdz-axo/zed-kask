use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;

use crate::research::types::*;
use hkask_mcp_server::server::validate_tool_url_with_dns;

mod arxiv;
mod brave;
mod crossref;
mod exa;
mod firecrawl;
mod openalex;
mod raw_fetch;
mod semantic_scholar;
mod serapi;
mod tavily;

pub(crate) use arxiv::ArxivProvider;
pub(crate) use brave::BraveProvider;
pub use crossref::CrossrefCandidate;
pub(crate) use crossref::CrossrefProvider;
pub(crate) use crossref::title_matches_exactly;
pub(crate) use exa::ExaProvider;
pub(crate) use firecrawl::FirecrawlProvider;
pub(crate) use openalex::OpenAlexProvider;
pub(crate) use raw_fetch::{RawFetchProvider, truncate_str, validated_fetch_client};
pub(crate) use semantic_scholar::SemanticScholarProvider;
pub(crate) use serapi::{SerapiProvider, SerpEngine};
pub(crate) use tavily::TavilyProvider;

/// Build the shared HTTP client used by all research providers.
///
/// Applies a consistent user-agent and request timeout, eliminating the repeated
/// `reqwest::Client::builder()...build().expect(...)` boilerplate across providers.
///
/// Returns `Err` if the TLS backend fails to initialize rather than panicking,
/// so callers can propagate the failure through their `Result` return type.
pub(super) fn provider_http_client() -> Result<reqwest::Client, WebError> {
    reqwest::Client::builder()
        .user_agent(format!("hkask-mcp-web/{SERVER_VERSION}"))
        .timeout(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| WebError::ProviderError(format!("failed to build HTTP client: {e}")))
}

/// Render a reqwest error with its full cause chain. reqwest's `Display` shows
/// only the kind ("builder error", "error sending request") and drops the
/// cause — which made provider failures undiagnosable (a header-parse failure,
/// a bad proxy URL, and a TLS problem all read as the same bare "builder
/// error"). This walks `source()` so the surfaced error names the actual cause.
pub(crate) fn reqwest_error_detail(error: &reqwest::Error) -> String {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
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
    /// A provider that runs only when named explicitly (`provider=`), never in
    /// fused, `quick`, or intent-routed searches — used for paid specialty
    /// engines (Google Scholar, Google Books) the caller must choose on purpose.
    fn explicit_only(&self) -> bool {
        false
    }
    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError>;
    async fn health(&self) -> Result<(), WebError>;
}

/// Validate a URL for SSRF safety before making outbound requests.
///
/// Wraps the shared `validate_tool_url_with_dns` from `hkask-mcp` and converts
/// the error to `WebError`. Used by `RawFetchProvider` for defense-in-depth URL
/// validation. This is async because it resolves the hostname via DNS to
/// defeat hostname-based SSRF bypasses (CWE-918/441) — a non-literal hostname
/// resolving to a private/loopback IP is rejected here.
pub async fn validate_provider_url(url: &str) -> Result<(), WebError> {
    validate_tool_url_with_dns(url)
        .await
        .map_err(|e| WebError::BadArgs(e.message))
}

/// Validate a URL with permissive SSRF config (allows private IPs and loopback).
///
/// Used by RSS tools (`rss_fetch`, `import_opml`) where the user has
/// explicitly subscribed to a feed that may be on a local network (e.g.,
/// a self-hosted RSS aggregator at `http://localhost:4000/feed.xml`).
/// The strict variant (`validate_provider_url`) is used for arbitrary
/// user-supplied URLs (`web_extract`, `web_browse`, `discover_feeds`).
pub(crate) fn validate_provider_url_permissive(url: &str) -> Result<(), WebError> {
    hkask_mcp_server::server::validate_tool_url_permissive(url)
        .map_err(|e| WebError::BadArgs(e.message))
}

/// Whether a provider KIND name is recognized, and what gates it.
/// Static score for a provider kind: lower cost + faster latency = lower
/// score (we sort ascending). Returns a neutral mid-score for providers
/// without a profile (free providers like arxiv/semantic_scholar).
fn score_static(kind: &str) -> f64 {
    match provider_profile(kind) {
        Some(p) => {
            let latency_penalty = match p.latency_tier {
                LatencyTier::Fast => 0.0,
                LatencyTier::Medium => 0.5,
                LatencyTier::Slow => 1.0,
            };
            p.cost_per_call_usd + latency_penalty
        }
        None => 0.5,
    }
}

/// Whether a provider KIND name is recognized, and what gates it. Drives the
/// `search_single_provider` error taxonomy: a recognized name that isn't
/// registered is a configuration fact (permission_denied naming the env
/// var); an unrecognized name is a closed-set mistake — typo or quoted
/// value — (invalid_argument naming the valid values). Conflating the two
/// sent agents chasing API keys for quoting mistakes (observed live
/// 2026-10-09).
pub(crate) enum KnownProviderKind {
    /// Recognized; enabled by setting this env var.
    Credential(&'static str),
    /// Recognized; free and always registered at pool construction, so its
    /// absence from a pool is a construction anomaly, not configuration.
    Free,
    /// Not a provider kind name at all.
    Unknown,
}

pub(crate) fn known_provider_kind(kind: &str) -> KnownProviderKind {
    match kind {
        "tavily" => KnownProviderKind::Credential("HKASK_TAVILY_API_KEY"),
        "brave" => KnownProviderKind::Credential("HKASK_BRAVE_API_KEY"),
        "exa" => KnownProviderKind::Credential("HKASK_EXA_API_KEY"),
        "firecrawl" => KnownProviderKind::Credential("HKASK_FIRECRAWL_API_KEY"),
        "serpapi" | "google_scholar" | "google_books" => {
            KnownProviderKind::Credential("HKASK_SERPAPI_API_KEY")
        }
        "openalex" | "arxiv" | "semantic_scholar" => KnownProviderKind::Free,
        _ => KnownProviderKind::Unknown,
    }
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
        provider: Option<&str>,
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
    /// Kinds of all configured search providers (e.g. ["brave", "exa"]).
    /// Used to surface the static profile table for metacognitive context.
    fn provider_kinds(&self) -> Vec<String>;
    /// Score each configured provider against a query + intent hint,
    /// returning ranked recommendations. See `ProviderPool::score_providers`.
    fn score_providers(&self, _query: &str, _intent: Option<&str>) -> Vec<ProviderRecommendation>;
    /// Resolve a typed paper identifier to scholarly metadata. Default:
    /// this port does not resolve papers (surfaced by the caller as a
    /// degradation note — never silent). `Ok(None)` = no record for the
    /// identifier; `Err` = the lookup itself failed.
    async fn resolve_paper_id(
        &self,
        _id: &crate::research::paper_id::PaperId,
    ) -> Result<Option<PaperMetadata>, WebError> {
        Err(WebError::NoProvider)
    }
    /// Bibliographic title resolution: Crossref's `query.bibliographic`
    /// search returns up to `rows` candidate works (DOI, title, year,
    /// venue, first author), most relevant first. Default: this port does
    /// not resolve titles (surfaced by the caller — never silent).
    /// `Ok(vec![])` = no registered work matches the title.
    async fn resolve_title(
        &self,
        _title: &str,
        _rows: u32,
    ) -> Result<Vec<CrossrefCandidate>, WebError> {
        Err(WebError::NoProvider)
    }
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

pub(crate) struct ProviderPool {
    pub(crate) search_providers: Vec<Box<dyn WebSearchProvider>>,
    pub(crate) extract_providers: Vec<Box<dyn WebExtractProvider>>,
    pub(crate) browse_providers: Vec<Box<dyn WebBrowseProvider>>,
    pub(crate) exa: Option<ExaProvider>,
    /// The OpenAlex provider, held typed (the `exa` pattern) for paper-id
    /// resolution — a direct lookup, not a pool search.
    pub(crate) openalex: Option<OpenAlexProvider>,
    /// The Crossref provider, held typed (the `exa`/`openalex` pattern) for
    /// bibliographic title resolution — `resolve_paper`'s title mode, not a
    /// pool search (Crossref's relevance search is a resolution instrument,
    /// not a discovery provider).
    pub(crate) crossref: Option<CrossrefProvider>,
    /// In-process rolling performance aggregator for the cybernetic feedback
    /// loop. Updated inline at each `reg.web.provider` span emission site;
    /// read by `score_providers` to apply live success-rate and p50-latency
    /// penalties on top of the static `ProviderProfile` table.
    pub(crate) performance:
        std::sync::Mutex<crate::research::performance::ProviderPerformanceAggregator>,
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
        openalex: Option<OpenAlexProvider>,
        crossref: Option<CrossrefProvider>,
    ) -> Self {
        Self {
            search_providers,
            extract_providers,
            browse_providers,
            exa,
            openalex,
            crossref,
            performance: std::sync::Mutex::new(
                crate::research::performance::ProviderPerformanceAggregator::new(),
            ),
        }
    }
}

impl ProviderPool {
    /// Query a single named provider. Returns a `CompoundSearchResult` with
    /// that provider's results ranked (no fusion — one provider, one rank
    /// list). Used when the caller sets `provider` explicitly or when the
    /// `quick` strategy picks a single best-scored provider.
    ///
    /// A name that isn't registered errors by WHICH mistake it is
    /// (`known_provider_kind`): a recognized kind is a configuration gap
    /// (`NoProviderConfigured` → permission_denied, naming the env var);
    /// anything else is a closed-set mistake — typo or quoted value —
    /// (`BadArgs` → invalid_argument, naming the registered kinds). The
    /// caller never gets a silent fallback to another provider.
    pub async fn search_single_provider(
        &self,
        kind: &str,
        query: &SearchQuery,
    ) -> Result<CompoundSearchResult, WebError> {
        let provider = self
            .search_providers
            .iter()
            .find(|p| p.kind() == kind)
            .ok_or_else(|| {
                let registered = self.search_provider_kinds().join(", ");
                match known_provider_kind(kind) {
                    KnownProviderKind::Credential(var) => WebError::NoProviderConfigured(format!(
                        "Provider '{kind}' is recognized but not configured — set {var} \
                         to enable it. Registered providers: {registered}."
                    )),
                    KnownProviderKind::Free => WebError::NoProviderConfigured(format!(
                        "Provider '{kind}' is free and always registered at pool \
                         construction — its absence from this pool is a construction \
                         anomaly, not a configuration issue."
                    )),
                    KnownProviderKind::Unknown => WebError::BadArgs(format!(
                        "Unknown provider '{kind}'. Registered providers: {registered}. \
                         Other recognized names (each needs an API key): tavily, brave, \
                         exa, firecrawl, serpapi, google_scholar, google_books."
                    )),
                }
            })?;

        let start = std::time::Instant::now();
        let result = provider.search(query).await;
        let latency_ms = start.elapsed().as_millis() as u64;
        // Emit per-provider outcome span for the cybernetic feedback loop.
        // Same target as search_compound so the curator's MetacognitionLoop
        // aggregates single-provider and compound calls together.
        let outcome = match &result {
            Ok(_) => "ok",
            Err(WebError::RateLimited(_)) => "rate_limited",
            Err(WebError::ProviderUnavailable(_)) => "unavailable",
            Err(WebError::ProviderError(_)) => "error",
            Err(_) => "error",
        };
        let error_kind = match &result {
            Ok(_) => None,
            Err(e) => Some(e.kind()),
        };
        tracing::info!(
            target: "reg.web.provider",
            kind = %kind,
            outcome = outcome,
            latency_ms = latency_ms,
            error_kind = error_kind.map(|k| k.to_string()).as_deref().unwrap_or(""),
            "REG"
        );
        // Record into the in-process aggregator for live score_providers
        // penalties. Best-effort — a poisoned lock skips the live path; the
        // read side surfaces the degraded state (`live_stats_degraded` +
        // the rationale part), so the skip is never silent.
        if let Ok(mut agg) = self.performance.lock() {
            agg.record_outcome(
                kind,
                crate::research::performance::ProviderOutcome {
                    latency_ms,
                    success: result.is_ok(),
                },
            );
        }
        let providers_queried = vec![ProviderInfo {
            kind: kind.to_string(),
            capabilities: provider.capabilities(),
        }];
        match result {
            Ok(output) => {
                let total = output.results.len();
                let ranked: Vec<RankedResult> = output
                    .results
                    .into_iter()
                    .enumerate()
                    .map(|(rank, r)| RankedResult {
                        rrf_score: rrf_score(RRF_K, &[rank]),
                        provider_count: 1,
                        providers: vec![kind.to_string()],
                        best_rank: Some(rank),
                        extracted_content: None,
                        content_preview: output
                            .content_previews
                            .get(&r.url.to_lowercase())
                            .cloned(),
                        semantic_score: output.semantic_scores.get(&r.url.to_lowercase()).copied(),
                        title: r.title,
                        url: r.url,
                        description: r.description,
                        source: r.source,
                        published: r.published,
                        oa_pdf_url: r.oa_pdf_url,
                    })
                    .collect();
                Ok(CompoundSearchResult {
                    query: query.query.clone(),
                    strategy: format!("provider:{kind}"),
                    results: ranked,
                    answer_box: output.answer_box,
                    related_questions: output.related_questions,
                    providers_queried,
                    providers_succeeded: vec![kind.to_string()],
                    providers_failed: Vec::new(),
                    total_before_dedup: total,
                    duplicates_removed: 0,
                })
            }
            Err(e) => Ok(CompoundSearchResult {
                query: query.query.clone(),
                strategy: format!("provider:{kind}"),
                results: Vec::new(),
                answer_box: None,
                related_questions: Vec::new(),
                providers_queried,
                providers_succeeded: Vec::new(),
                providers_failed: vec![ProviderFailureRecord {
                    kind: kind.to_string(),
                    error: e.to_string(),
                }],
                total_before_dedup: 0,
                duplicates_removed: 0,
            }),
        }
    }

    /// The live-merged scoring model for one provider kind: the static
    /// profile score plus the live success-rate/p50 penalty from the
    /// in-process `reg.web.provider` feedback loop. The single model
    /// behind both `score_providers`' ranking and the quick strategy's
    /// pick — one feedback loop, two consumers. Below
    /// `MIN_SAMPLES_FOR_LIVE` the penalty is 0 and this reduces to the
    /// static score.
    fn live_merged_score(&self, kind: &str) -> (f64, Vec<&'static str>) {
        let (live_penalty, live_rationale) =
            crate::research::performance::live_performance_penalty(&self.performance, kind);
        (score_static(kind) + live_penalty.max(0.0), live_rationale)
    }

    /// Pick the quick strategy's provider: the registered, keyword-capable,
    /// non-explicit-only candidate with the best (lowest) live-merged
    /// score. C3: the quick pick was static-only, ignoring the live
    /// feedback the server already collects — with the shared model a
    /// provider that keeps failing drops out of quick automatically.
    /// Ties break on alphabetical kind for determinism. Errors with the
    /// keyword-capability message when no candidate exists.
    pub(crate) fn pick_quick_provider(&self) -> Result<&str, WebError> {
        let candidates: Vec<&dyn WebSearchProvider> = self
            .search_providers
            .iter()
            .filter(|p| !p.explicit_only())
            .filter(|p| p.capabilities().contains(&SearchCapability::Keyword))
            .map(|p| p.as_ref())
            .collect();
        if candidates.is_empty() {
            return Err(WebError::NoProviderConfigured(
                "No keyword-capable provider configured. Set an API key \
                 (HKASK_BRAVE_API_KEY, HKASK_TAVILY_API_KEY, etc.) to use web_search."
                    .to_string(),
            ));
        }
        Ok(candidates
            .into_iter()
            .map(|p| (self.live_merged_score(p.kind()).0, p.kind()))
            .min_by(|a, b| {
                a.0.partial_cmp(&b.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.1.cmp(b.1))
            })
            .expect("non-empty candidates checked above")
            .1)
    }

    pub async fn search_compound(
        &self,
        query: &SearchQuery,
        strategy: SearchStrategy,
    ) -> CompoundSearchResult {
        let filtered: Vec<&dyn WebSearchProvider> = match strategy.provider_filter() {
            ProviderFilter::All => self
                .search_providers
                .iter()
                .filter(|p| !p.explicit_only())
                .map(|p| p.as_ref())
                .collect(),
            ProviderFilter::Capabilities(caps) => self
                .search_providers
                .iter()
                .filter(|p| !p.explicit_only())
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
            .map(|p| {
                // Capture a reference to the in-process aggregator so each
                // provider future can record its outcome. The borrow is valid
                // for the duration of `search_compound` (which holds `&self`).
                let performance = &self.performance;
                async move {
                    let kind = p.kind().to_string();
                    let start = std::time::Instant::now();
                    let result = match tokio::time::timeout(
                        Duration::from_secs(COMPOUND_PROVIDER_TIMEOUT_SECS),
                        p.search(query),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(_) => {
                            tracing::warn!(
                                provider = %kind,
                                timeout_secs = COMPOUND_PROVIDER_TIMEOUT_SECS,
                                "Compound search provider timed out"
                            );
                            Err(WebError::ProviderUnavailable(format!(
                                "Provider timed out after {COMPOUND_PROVIDER_TIMEOUT_SECS}s"
                            )))
                        }
                    };
                    let latency_ms = start.elapsed().as_millis() as u64;
                    let success = result.is_ok();
                    // Emit per-provider outcome span for the cybernetic feedback
                    // loop. The curator's MetacognitionLoop and reg_query read
                    // these to compute rolling success-rate/latency per provider,
                    // which feeds back into score_providers (Layer 3 dynamic).
                    let outcome = match &result {
                        Ok(_) => "ok",
                        Err(WebError::RateLimited(_)) => "rate_limited",
                        Err(WebError::ProviderUnavailable(_)) => "unavailable",
                        Err(WebError::ProviderError(_)) => "error",
                        Err(_) => "error",
                    };
                    let error_kind = match &result {
                        Ok(_) => None,
                        Err(e) => Some(e.kind()),
                    };
                    tracing::info!(
                        target: "reg.web.provider",
                        kind = %kind,
                        outcome = outcome,
                        latency_ms = latency_ms,
                        error_kind = error_kind.map(|k| k.to_string()).as_deref().unwrap_or(""),
                        "REG"
                    );
                    // Record into the in-process aggregator for live
                    // score_providers penalties. Best-effort — a poisoned
                    // lock skips the live path (static profile still applies).
                    if let Ok(mut agg) = performance.lock() {
                        agg.record_outcome(
                            &kind,
                            crate::research::performance::ProviderOutcome {
                                latency_ms,
                                success,
                            },
                        );
                    }
                    (kind, result)
                }
            })
            .collect();

        let results = futures_util::future::join_all(futures).await;

        let mut succeeded: Vec<String> = Vec::new();
        let mut failed: Vec<ProviderFailureRecord> = Vec::new();
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
                    failed.push(ProviderFailureRecord {
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
            oa_pdf_url: Option<String>,
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
                    // First provider to report a candidate OA copy wins; a
                    // later provider's None never clears it.
                    if entry.oa_pdf_url.is_none() {
                        entry.oa_pdf_url = result.oa_pdf_url;
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
                            oa_pdf_url: result.oa_pdf_url,
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
                    oa_pdf_url: entry.oa_pdf_url,
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
            None => Err(WebError::NoProviderConfigured(
                "Exa provider not configured. Set HKASK_EXA_API_KEY to use web_find_similar."
                    .to_string(),
            )),
        }
    }

    pub async fn extract_with_fallback(
        &self,
        url: &str,
        opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        // SSRF defense-in-depth: validate once at the pool boundary so each
        // provider in the fallback chain doesn't re-resolve DNS. The tool
        // layer (validate_tool_url_with_dns) is the outer gate; this is the
        // inner gate before any provider fetches the URL.
        validate_provider_url(url).await?;
        try_fallback!(&self.extract_providers, extract, url, opts)
    }

    pub async fn browse_with_fallback(
        &self,
        url: &str,
        instruction: &str,
        timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        // SSRF defense-in-depth: validate once at the pool boundary.
        validate_provider_url(url).await?;
        if self.browse_providers.is_empty() {
            return Err(WebError::NoProviderConfigured(
                "No browse provider configured. Set HKASK_FIRECRAWL_API_KEY, \
                 HKASK_TAVILY_API_KEY, or HKASK_EXA_API_KEY to use web_browse."
                    .to_string(),
            ));
        }
        try_fallback!(&self.browse_providers, browse, url, instruction, timeout)
    }

    /// Score each configured search provider against a query + intent hint,
    /// returning ranked recommendations. This is the metacognitive surface:
    /// `web_search` calls it on every tool-selected path — the `intent`
    /// pick queries the top configured recommendation, and every other
    /// selection path (quick, web, news, deep) surfaces the ranking in the
    /// response's `provider_recommendations` as the audit trail. The
    /// quick strategy's pick (`pick_quick_provider`) shares the same
    /// live-merged model (`live_merged_score`) — one feedback loop, two
    /// consumers.
    ///
    /// Scoring (lower is better):
    /// - Static: `cost_per_call_usd` + latency penalty (Fast=0, Medium=0.5, Slow=1.0)
    /// - Intent match: -0.5 bonus when the provider's `best_for` includes the intent
    /// - Capability match: -0.3 bonus when the provider has a capability matching intent
    ///
    /// Layer 3 merges live success-rate and p50-latency penalties from the
    /// in-process `ProviderPerformanceAggregator`, fed by `reg.web.provider`
    /// spans. Below `MIN_SAMPLES_FOR_LIVE` (3), the static profile alone
    /// drives selection — the live data is too thin to trust.
    pub fn score_providers(
        &self,
        _query: &str,
        intent: Option<&str>,
    ) -> Vec<ProviderRecommendation> {
        let configured_kinds: std::collections::HashSet<&str> =
            self.search_providers.iter().map(|p| p.kind()).collect();

        // Pool-global: the live channel's health is one lock state shared by
        // every provider's recommendation (loop-register L23, S4).
        let live_degraded = crate::research::performance::live_channel_degraded(&self.performance);

        let mut recs: Vec<ProviderRecommendation> = PROVIDER_PROFILES
            .iter()
            .map(|profile| {
                let configured = configured_kinds.contains(profile.kind);
                // Live-merged base (C3): the single scoring model shared
                // with the quick strategy's pick — static profile + live
                // success/p50 penalty.
                let (mut score, live_rationale) = self.live_merged_score(profile.kind);
                let mut rationale_parts: Vec<&str> = Vec::new();

                // Intent match bonus
                if let Some(intent) = intent {
                    if profile.best_for.contains(&intent) {
                        score -= 0.5;
                        rationale_parts.push("intent match");
                    }
                    // Capability match for specific intents
                    let provider = self
                        .search_providers
                        .iter()
                        .find(|p| p.kind() == profile.kind);
                    if let Some(p) = provider {
                        let caps = p.capabilities();
                        let cap_match = match intent {
                            "news" => caps.contains(&SearchCapability::News),
                            "freshness" => caps.contains(&SearchCapability::Freshness),
                            "semantic" | "academic" | "research" => {
                                caps.contains(&SearchCapability::Semantic)
                            }
                            "transcript" => caps.contains(&SearchCapability::Transcript),
                            _ => false,
                        };
                        if cap_match {
                            score -= 0.3;
                            rationale_parts.push("capability match");
                        }
                    }
                }

                // Zero-penalty rationales still carry load-bearing news: the
                // poisoned-lock arm reports the degraded live channel at
                // penalty 0.0. Gating the rationale on the penalty is what
                // made that degradation silent (loop-register L23).
                rationale_parts.extend(live_rationale.iter());
                // Snapshot live stats for surfacing in the recommendation.
                // `None` below MIN_SAMPLES_FOR_LIVE — the model sees the static
                // profile alone until enough data accumulates.
                let live_stats =
                    crate::research::performance::snapshot_stats(&self.performance, profile.kind);

                // Unconfigured providers get a penalty so they rank below configured ones
                if !configured {
                    score += 10.0;
                    rationale_parts.push("not configured (no API key)");
                }

                let rationale = if rationale_parts.is_empty() {
                    format!(
                        "cost ${:.4}/call + {:?} latency",
                        profile.cost_per_call_usd, profile.latency_tier
                    )
                } else {
                    rationale_parts.join(", ")
                };

                ProviderRecommendation {
                    kind: profile.kind.to_string(),
                    score,
                    rationale,
                    cost_per_call_usd: profile.cost_per_call_usd,
                    latency_tier: profile.latency_tier,
                    strengths: profile.strengths.iter().map(|s| s.to_string()).collect(),
                    weaknesses: profile.weaknesses.iter().map(|s| s.to_string()).collect(),
                    best_for: profile.best_for.iter().map(|s| s.to_string()).collect(),
                    configured,
                    live_success_rate: live_stats.as_ref().map(|s| s.success_rate),
                    live_p50_latency_ms: live_stats.as_ref().map(|s| s.p50_latency_ms),
                    live_sample_count: live_stats.as_ref().map(|s| s.sample_count),
                    live_stats_degraded: live_degraded,
                }
            })
            .collect();

        recs.sort_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.kind.cmp(&b.kind))
        });
        recs
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
            ($surface:expr, $provs:expr) => {
                for p in $provs {
                    let k = p.kind().to_string();
                    let r = p.health().await;
                    entries.push(health_entry(k, $surface, r));
                }
            };
        }
        health_them!("search", &self.search_providers);
        health_them!("extract", &self.extract_providers);
        health_them!("browse", &self.browse_providers);
        if let Some(ref exa) = self.exa {
            let r = WebSearchProvider::health(exa).await;
            entries.push(health_entry("exa-similar".into(), "find_similar", r));
        }
        entries
    }
}

fn health_entry(kind: String, surface: &str, result: Result<(), WebError>) -> ProviderHealthEntry {
    ProviderHealthEntry {
        kind,
        surface: surface.to_string(),
        healthy: result.is_ok(),
        error: result.err().map(|e| sanitize_health_error(&e.to_string())),
    }
}

// WebSearchPort implementation - ProviderPool as the adapter

#[async_trait]
impl WebSearchPort for ProviderPool {
    async fn search(
        &self,
        query: &SearchQuery,
        strategy: SearchStrategy,
        provider: Option<&str>,
    ) -> Result<CompoundSearchResult, WebError> {
        // N1: CapabilityContext removed; Tool dispatch is enforced at the
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

        let mut compound = if let Some(kind) = provider {
            // Explicit provider override — single provider, no fusion, no
            // fallback. The caller picked deliberately (from the intent
            // ranking surfaced in provider_recommendations, or web_ping's
            // provider list). An unrecognized name is a closed-set mistake
            // (BadArgs); a recognized-but-unconfigured one is a credential
            // gap (NoProviderConfigured) — see search_single_provider.
            self.search_single_provider(kind, query).await?
        } else if strategy == SearchStrategy::Quick {
            // Quick strategy: pick the single best-scored keyword-capable
            // provider, not blind first-Ok-wins fallback. C3: the pick uses
            // the live-merged scoring (static profile + live success/p50
            // penalties from the reg.web.provider feedback loop) — the same
            // model as score_providers' ranking — so a provider that keeps
            // failing drops out of quick automatically. Below
            // MIN_SAMPLES_FOR_LIVE this reduces to the static pick.
            let picked = self.pick_quick_provider()?;
            self.search_single_provider(picked, query).await?
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
        self.find_similar(url, num_results).await
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

    async fn resolve_paper_id(
        &self,
        id: &crate::research::paper_id::PaperId,
    ) -> Result<Option<PaperMetadata>, WebError> {
        match &self.openalex {
            Some(provider) => provider.resolve(id).await,
            None => Err(WebError::NoProvider),
        }
    }

    /// Bibliographic title resolution: Crossref's `query.bibliographic`
    /// search returns up to `rows` candidate works (DOI, title, year,
    /// venue, first author), most relevant first. Every candidate is
    /// surfaced — a title search can hit a different work than intended,
    /// so the caller verifies the match before trusting the resolution.
    /// `Ok(vec![])` is a legitimate outcome (no registered work matches).
    async fn resolve_title(
        &self,
        title: &str,
        rows: u32,
    ) -> Result<Vec<CrossrefCandidate>, WebError> {
        match &self.crossref {
            Some(provider) => provider.search_bibliographic(title, rows).await,
            None => Err(WebError::NoProvider),
        }
    }

    async fn health_check(&self) -> Vec<ProviderHealthEntry> {
        self.health_check_all().await
    }

    fn provider_fingerprint(&self) -> String {
        ProviderPool::provider_fingerprint(self)
    }

    fn provider_kinds(&self) -> Vec<String> {
        self.search_provider_kinds()
    }

    fn score_providers(&self, query: &str, intent: Option<&str>) -> Vec<ProviderRecommendation> {
        ProviderPool::score_providers(self, query, intent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stub provider for testing the quick pick and pool-surface pins.
    /// Returns a fixed kind and keyword capability — enough for the scorer
    /// to exercise the static profile table.
    struct StubProvider {
        kind: &'static str,
    }

    #[async_trait]
    impl WebSearchProvider for StubProvider {
        fn kind(&self) -> &str {
            self.kind
        }
        fn capabilities(&self) -> Vec<SearchCapability> {
            vec![SearchCapability::Keyword]
        }
        async fn search(&self, _query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
            Err(WebError::NoProvider)
        }
        async fn health(&self) -> Result<(), WebError> {
            Ok(())
        }
    }

    /// Extract-surface stub — same fixed kind, for the pool-surface pin.
    struct StubExtractProvider {
        kind: &'static str,
    }

    #[async_trait]
    impl WebExtractProvider for StubExtractProvider {
        fn kind(&self) -> &str {
            self.kind
        }
        async fn extract(
            &self,
            _url: &str,
            _opts: &ExtractOptions,
        ) -> Result<ExtractedContent, WebError> {
            Err(WebError::NoProvider)
        }
        async fn health(&self) -> Result<(), WebError> {
            Ok(())
        }
    }

    /// Browse-surface stub — same fixed kind, for the pool-surface pin.
    struct StubBrowseProvider {
        kind: &'static str,
    }

    #[async_trait]
    impl WebBrowseProvider for StubBrowseProvider {
        fn kind(&self) -> &str {
            self.kind
        }
        async fn browse(
            &self,
            _url: &str,
            _instruction: &str,
            _timeout: std::time::Duration,
        ) -> Result<BrowseResult, WebError> {
            Err(WebError::NoProvider)
        }
        async fn health(&self) -> Result<(), WebError> {
            Ok(())
        }
    }

    /// R-03 pin (mcp-tool-review): health entries carry the pool surface
    /// they serve. The same provider kind registers once per surface it
    /// serves (firecrawl: search+extract+browse; tavily/exa: search+browse)
    /// — without the label, the ping's health list shows indistinguishable
    /// duplicate kinds and the caller cannot tell which registration is
    /// unhealthy.
    #[tokio::test]
    async fn health_entries_label_the_pool_surface() {
        let pool = ProviderPool::new(
            vec![Box::new(StubProvider { kind: "stub" })],
            vec![Box::new(StubExtractProvider { kind: "stub" })],
            vec![Box::new(StubBrowseProvider { kind: "stub" })],
            None,
            None,
            None,
        );
        let entries = pool.health_check_all().await;
        assert_eq!(entries.len(), 3, "one entry per surface registration");
        let surfaces: Vec<&str> = entries.iter().map(|e| e.surface.as_str()).collect();
        assert!(surfaces.contains(&"search"), "surfaces: {surfaces:?}");
        assert!(surfaces.contains(&"extract"), "surfaces: {surfaces:?}");
        assert!(surfaces.contains(&"browse"), "surfaces: {surfaces:?}");
        // The distinguishability contract: every (kind, surface) pair is
        // unique — three same-kind registrations are three distinct rows.
        let mut pairs: Vec<(&str, &str)> = entries
            .iter()
            .map(|e| (e.kind.as_str(), e.surface.as_str()))
            .collect();
        pairs.sort();
        pairs.dedup();
        assert_eq!(
            pairs.len(),
            entries.len(),
            "every (kind, surface) pair is unique: {pairs:?}"
        );
    }

    /// The quick pick must select the lowest-cost, fastest-latency provider
    /// from the static profile table — not blind first-Ok-wins. This pins
    /// the deliberate-selection behavior (and the static layer of the
    /// live-merged model): `quick` strategy picks Brave ($0.002, Fast) over
    /// Tavily ($0.003, Fast) over Exa ($0.01, Medium).
    #[test]
    fn pick_quick_provider_prefers_lower_cost_faster_latency() {
        let pool = ProviderPool::new(
            vec![
                Box::new(StubProvider { kind: "exa" }),
                Box::new(StubProvider { kind: "tavily" }),
                Box::new(StubProvider { kind: "brave" }),
            ],
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        );
        assert_eq!(
            pool.pick_quick_provider().expect("quick pick"),
            "brave",
            "quick strategy must pick the lowest-cost fastest-latency provider, \
             not blind first-Ok-wins fallback"
        );
    }

    /// When no candidate has a profile (free providers only), the quick
    /// pick falls back to the neutral score — deterministic via
    /// alphabetical tiebreak.
    #[test]
    fn pick_quick_provider_unprofiled_falls_back_alphabetically() {
        let pool = ProviderPool::new(
            vec![
                Box::new(StubProvider { kind: "arxiv" }),
                Box::new(StubProvider {
                    kind: "semantic_scholar",
                }),
            ],
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        );
        assert_eq!(
            pool.pick_quick_provider().expect("quick pick"),
            "arxiv",
            "unprofiled providers should tie-break alphabetically for determinism"
        );
    }

    /// C3: the quick pick reads the live feedback loop. Brave wins the
    /// static layer ($0.002 < $0.003), but after enough live failures
    /// (≥ MIN_SAMPLES_FOR_LIVE, success rate < 0.5 → +2.0 penalty) the
    /// pick must flip to tavily — a persistently failing provider drops
    /// out of quick automatically, the behavior the static-only pick
    /// could not deliver.
    #[test]
    fn pick_quick_provider_drops_a_persistently_failing_provider() {
        let pool = ProviderPool::new(
            vec![
                Box::new(StubProvider { kind: "brave" }),
                Box::new(StubProvider { kind: "tavily" }),
            ],
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        );
        // Static pick: brave.
        assert_eq!(pool.pick_quick_provider().expect("quick pick"), "brave");
        // Record enough failures for brave to cross MIN_SAMPLES_FOR_LIVE
        // with a success rate under 0.5.
        {
            let mut agg = pool.performance.lock().expect("performance lock");
            for _ in 0..3 {
                agg.record_outcome(
                    "brave",
                    crate::research::performance::ProviderOutcome {
                        latency_ms: 100,
                        success: false,
                    },
                );
            }
        }
        // Live-merged pick: tavily — brave's +2.0 penalty dwarfs the
        // 0.001 static gap.
        assert_eq!(
            pool.pick_quick_provider().expect("quick pick"),
            "tavily",
            "live failures must flip the quick pick — the feedback loop drives \
             selection, not just reporting"
        );
    }

    /// `score_static` must return a lower score for cheaper + faster providers.
    #[test]
    fn score_static_lower_is_better() {
        let brave_score = score_static("brave");
        let exa_score = score_static("exa");
        assert!(
            brave_score < exa_score,
            "brave (${brave_score}) should score lower than exa (${exa_score}) — \
             cheaper + same-or-faster latency"
        );
    }

    /// `score_providers` must rank configured providers above unconfigured ones,
    /// and apply the intent-match bonus. This pins the metacognitive surface:
    /// the model reads ranked recommendations to pick deliberately.
    #[test]
    fn score_providers_ranks_configured_above_unconfigured() {
        // Build a pool with only Brave configured (no API keys for others).
        let brave = StubProvider { kind: "brave" };
        let pool = ProviderPool::new(
            vec![Box::new(brave)],
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        );
        let recs = pool.score_providers("test query", None);
        // Brave (configured) should rank first; others get the +10 unconfigured penalty.
        assert_eq!(recs[0].kind, "brave");
        assert!(recs[0].configured, "top recommendation must be configured");
        for r in &recs[1..] {
            assert!(
                !r.configured,
                "lower recommendations should be unconfigured"
            );
            assert!(
                r.score > recs[0].score,
                "unconfigured must score higher (worse)"
            );
        }
    }

    /// `score_providers` must apply the intent-match bonus: a news intent
    /// should rank Brave (best_for includes "news") above Tavily (doesn't).
    #[test]
    fn score_providers_intent_match_ranks_higher() {
        let brave = StubProvider { kind: "brave" };
        let tavily = StubProvider { kind: "tavily" };
        let pool = ProviderPool::new(
            vec![Box::new(brave), Box::new(tavily)],
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        );
        let recs = pool.score_providers("latest AI news", Some("news"));
        // Brave (best_for includes "news") should rank above Tavily.
        assert_eq!(recs[0].kind, "brave");
        assert!(
            recs[0].score < recs.iter().find(|r| r.kind == "tavily").unwrap().score,
            "brave should score lower (better) than tavily for news intent"
        );
    }

    /// Live performance data (Layer 3): recording failures for a provider
    /// must push its score down via the live-penalty path. This pins the
    /// cybernetic feedback loop — the aggregator feeds back into selection.
    #[test]
    fn score_providers_live_penalty_applies_after_failures() {
        let brave = StubProvider { kind: "brave" };
        let tavily = StubProvider { kind: "tavily" };
        let pool = ProviderPool::new(
            vec![Box::new(brave), Box::new(tavily)],
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        );
        // Baseline: brave scores lower (better) than tavily (cheaper + faster).
        let baseline = pool.score_providers("test", None);
        let brave_baseline = baseline.iter().find(|r| r.kind == "brave").unwrap().score;
        let tavily_baseline = baseline.iter().find(|r| r.kind == "tavily").unwrap().score;
        assert!(brave_baseline < tavily_baseline);

        // Record 4 failures for brave (success rate 0.0 < 0.5 → +2.0 penalty).
        {
            let mut agg = pool.performance.lock().unwrap();
            for _ in 0..4 {
                agg.record_outcome(
                    "brave",
                    crate::research::performance::ProviderOutcome {
                        latency_ms: 100,
                        success: false,
                    },
                );
            }
        }

        // After failures, brave should score worse than tavily (the live
        // penalty overcomes the static cost advantage).
        let after = pool.score_providers("test", None);
        let brave_after = after.iter().find(|r| r.kind == "brave").unwrap().score;
        let tavily_after = after.iter().find(|r| r.kind == "tavily").unwrap().score;
        assert!(
            brave_after > tavily_after,
            "brave should score worse than tavily after 4 failures \
             (brave={brave_after}, tavily={tavily_after}) — live penalty must apply"
        );
        // Brave's score should have increased by the penalty.
        assert!(brave_after > brave_baseline);
        // Live stats should be surfaced.
        let brave_rec = after.iter().find(|r| r.kind == "brave").unwrap();
        assert_eq!(brave_rec.live_sample_count, Some(4));
        assert_eq!(brave_rec.live_success_rate, Some(0.0));
    }

    /// Below MIN_SAMPLES_FOR_LIVE (3), live penalties must NOT apply — the
    /// static profile alone drives selection. This pins the cold-start guard.
    #[test]
    fn score_providers_no_live_penalty_below_sample_threshold() {
        let brave = StubProvider { kind: "brave" };
        let pool = ProviderPool::new(
            vec![Box::new(brave)],
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        );
        // Record 2 failures (below the 3-sample threshold).
        {
            let mut agg = pool.performance.lock().unwrap();
            for _ in 0..2 {
                agg.record_outcome(
                    "brave",
                    crate::research::performance::ProviderOutcome {
                        latency_ms: 100,
                        success: false,
                    },
                );
            }
        }
        let recs = pool.score_providers("test", None);
        let brave_rec = recs.iter().find(|r| r.kind == "brave").unwrap();
        // Live stats should be None (below threshold).
        assert!(brave_rec.live_sample_count.is_none());
        assert!(brave_rec.live_success_rate.is_none());
    }

    /// Stub that records whether it was searched, for routing tests.
    struct CountingStub {
        kind: &'static str,
        explicit: bool,
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait]
    impl WebSearchProvider for CountingStub {
        fn kind(&self) -> &str {
            self.kind
        }
        fn capabilities(&self) -> Vec<SearchCapability> {
            vec![SearchCapability::Keyword, SearchCapability::Semantic]
        }
        fn explicit_only(&self) -> bool {
            self.explicit
        }
        async fn search(&self, _query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ProviderSearchOutput::default())
        }
        async fn health(&self) -> Result<(), WebError> {
            Ok(())
        }
    }

    /// expect: fused and quick searches never call an explicit-only provider
    /// (paid Google Scholar/Books), while `provider=` reaches it.
    #[tokio::test]
    async fn explicit_only_providers_run_only_when_named() -> Result<(), WebError> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let web_calls = std::sync::Arc::new(AtomicUsize::new(0));
        let scholar_calls = std::sync::Arc::new(AtomicUsize::new(0));
        let pool = ProviderPool::new(
            vec![
                Box::new(CountingStub {
                    kind: "brave",
                    explicit: false,
                    calls: web_calls.clone(),
                }),
                Box::new(CountingStub {
                    kind: "google_scholar",
                    explicit: true,
                    calls: scholar_calls.clone(),
                }),
            ],
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        );
        let query = SearchQuery {
            query: "industry economics".to_string(),
            num_results: 5,
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
            freshness: None,
        };
        for strategy in [
            SearchStrategy::Web,
            SearchStrategy::Deep,
            SearchStrategy::Quick,
        ] {
            pool.search(&query, strategy, None).await?;
        }
        assert_eq!(
            scholar_calls.load(Ordering::SeqCst),
            0,
            "fused/quick search called an explicit-only provider"
        );
        assert!(web_calls.load(Ordering::SeqCst) >= 3);
        pool.search(&query, SearchStrategy::Quick, Some("google_scholar"))
            .await?;
        assert_eq!(
            scholar_calls.load(Ordering::SeqCst),
            1,
            "named provider was not called"
        );
        Ok(())
    }

    /// A provider name outside the closed set (typo, quoted value) is a
    /// caller mistake: BadArgs (→ invalid_argument) naming the registered
    /// kinds — NOT a credentials problem. Before the split, `"tavil"` and a
    /// quoted `"tavily"` both read as "set the API key", sending agents
    /// chasing credentials for a spelling error (observed live 2026-10-09).
    #[tokio::test]
    async fn single_provider_unknown_name_is_a_closed_set_error() {
        let pool = ProviderPool::new(
            vec![Box::new(StubProvider { kind: "brave" })],
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        );
        let query = SearchQuery {
            query: "test".to_string(),
            num_results: 5,
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
            freshness: None,
        };
        let err = pool
            .search_single_provider("tavil", &query)
            .await
            .expect_err("typo'd name must error");
        assert!(matches!(err, WebError::BadArgs(_)), "got {err:?}");
        let msg = err.to_string();
        assert!(msg.contains("Unknown provider 'tavil'"), "{msg}");
        assert!(
            msg.contains("brave"),
            "must name the registered kinds: {msg}"
        );
    }

    /// A recognized kind that isn't registered is a configuration gap:
    /// NoProviderConfigured (→ permission_denied) naming the env var that
    /// enables it — the agent's next move is the keychain, not a retry.
    #[tokio::test]
    async fn single_provider_known_unconfigured_names_the_credential() {
        let pool = ProviderPool::new(
            vec![Box::new(StubProvider { kind: "brave" })],
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        );
        let query = SearchQuery {
            query: "test".to_string(),
            num_results: 5,
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
            freshness: None,
        };
        let err = pool
            .search_single_provider("tavily", &query)
            .await
            .expect_err("unconfigured known kind must error");
        assert!(
            matches!(err, WebError::NoProviderConfigured(_)),
            "got {err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains("HKASK_TAVILY_API_KEY"), "{msg}");
    }

    /// Free providers are always registered at construction — one absent
    /// from a pool is a construction anomaly, never a closed-set mistake.
    #[tokio::test]
    async fn single_provider_free_kind_absence_is_a_construction_anomaly() {
        let pool = ProviderPool::new(
            vec![Box::new(StubProvider { kind: "brave" })],
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        );
        let query = SearchQuery {
            query: "test".to_string(),
            num_results: 5,
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
            freshness: None,
        };
        let err = pool
            .search_single_provider("openalex", &query)
            .await
            .expect_err("absent free kind must error");
        assert!(
            matches!(err, WebError::NoProviderConfigured(_)),
            "got {err:?}"
        );
        assert!(err.to_string().contains("construction"), "{}", err);
    }

    /// S4 (loop-register L23): a degraded live-performance channel must be
    /// surfaced, never silently read as "live data says this provider is
    /// fine". Poisoning the pool's performance mutex is the degraded state;
    /// before this contract the caller dropped zero-penalty rationales, so
    /// the poisoned arm's degradation message never reached the
    /// recommendation.
    #[test]
    fn poisoned_performance_channel_surfaces_the_degradation_in_every_recommendation() {
        let pool = ProviderPool::new(vec![], vec![], vec![], None, None, None);
        // Poison the channel: a panic while holding the lock poisons the
        // std Mutex permanently — the exact degraded state under test.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = pool.performance.lock().expect("lock before poisoning");
            panic!("poison the performance channel");
        }));
        let recs = pool.score_providers("query", None);
        assert!(
            !recs.is_empty(),
            "the static profile table always yields recommendations"
        );
        for rec in &recs {
            assert!(
                rec.rationale.contains("live performance unavailable"),
                "a degraded live channel must be surfaced in the rationale — got: {}",
                rec.rationale
            );
        }
    }

    /// The machine-readable half of the S4 contract: `live_stats_degraded`
    /// distinguishes a broken channel (`true`; live fields `None`) from a
    /// healthy pool with too few samples (`false`; live fields `None`) —
    /// the two states were indistinguishable before.
    #[test]
    fn live_stats_degraded_distinguishes_poisoned_channel_from_thin_samples() {
        let pool = ProviderPool::new(vec![], vec![], vec![], None, None, None);
        // Healthy, below MIN_SAMPLES_FOR_LIVE: not degraded, live fields None.
        let healthy = pool.score_providers("query", None);
        assert!(
            healthy
                .iter()
                .all(|r| !r.live_stats_degraded && r.live_sample_count.is_none()),
            "a healthy pool with thin samples must read as not-degraded with live fields None"
        );
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = pool.performance.lock().expect("lock before poisoning");
            panic!("poison the performance channel");
        }));
        let degraded = pool.score_providers("query", None);
        assert!(
            degraded.iter().all(|r| r.live_stats_degraded),
            "a poisoned channel must set live_stats_degraded on every recommendation"
        );
    }
}
