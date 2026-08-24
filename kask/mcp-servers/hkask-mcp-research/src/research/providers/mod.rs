use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;

use crate::research::types::*;
use hkask_mcp_server::server::validate_tool_url_with_dns;

mod arxiv;
mod brave;
mod exa;
mod firecrawl;
mod raw_fetch;
mod semantic_scholar;
mod serapi;
mod tavily;

pub(crate) use arxiv::ArxivProvider;
pub(crate) use brave::BraveProvider;
pub(crate) use exa::ExaProvider;
pub(crate) use firecrawl::FirecrawlProvider;
pub(crate) use raw_fetch::{RawFetchProvider, truncate_str};
pub(crate) use semantic_scholar::SemanticScholarProvider;
pub(crate) use serapi::SerapiProvider;
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

/// Pick the best provider from a set of candidates using the static profile
/// table. Scoring: lower cost and faster latency tier score higher. Ties
/// break on alphabetical kind for determinism. Returns the first candidate
/// if none have a profile (free providers only).
///
/// Layer 3 will merge live performance data (success rate, p50 latency)
/// into this score. For now, the static profile drives selection — already
/// a deliberate choice over blind first-Ok-wins fallback.
pub(crate) fn pick_best_provider<'a>(
    candidates: &[&'a (dyn WebSearchProvider + 'a)],
) -> &'a (dyn WebSearchProvider + 'a) {
    candidates
        .iter()
        .min_by(|a, b| {
            let sa = score_static((*a).kind());
            let sb = score_static((*b).kind());
            sa.partial_cmp(&sb)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| (*a).kind().cmp((*b).kind()))
        })
        .copied()
        .expect("pick_best_provider requires at least one candidate")
}

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
    ) -> Self {
        Self {
            search_providers,
            extract_providers,
            browse_providers,
            exa,
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
    /// Returns `NoProviderConfigured` if the named provider isn't registered
    /// (missing API key) so the caller can surface a clear error rather than
    /// silently falling back to another provider.
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
                WebError::NoProviderConfigured(format!(
                    "Provider '{kind}' is not configured. Set the corresponding API key \
                     or pick a configured provider via web_recommend_provider."
                ))
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
        // penalties. Best-effort — a poisoned lock skips the live path.
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
    /// the model calls `web_recommend_provider` to pick deliberately rather
    /// than relying on blind fallback.
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

        let mut recs: Vec<ProviderRecommendation> = PROVIDER_PROFILES
            .iter()
            .map(|profile| {
                let configured = configured_kinds.contains(profile.kind);
                let mut score = score_static(profile.kind);
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

                // Live performance penalty (Layer 3). Merges rolling
                // success-rate and p50-latency from the in-process aggregator.
                // No-op below MIN_SAMPLES_FOR_LIVE — static profile alone.
                let (live_penalty, live_rationale) =
                    crate::research::performance::live_performance_penalty(
                        &self.performance,
                        profile.kind,
                    );
                if live_penalty > 0.0 {
                    score += live_penalty;
                    rationale_parts.extend(live_rationale.iter());
                }
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
            let r = WebSearchProvider::health(exa).await;
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
            // fallback. The caller picked deliberately (likely via
            // web_recommend_provider). Returns NoProviderConfigured if the
            // named provider isn't registered.
            self.search_single_provider(kind, query).await?
        } else if strategy == SearchStrategy::Quick {
            // Quick strategy: pick the single best-scored keyword-capable
            // provider, not blind first-Ok-wins fallback. Scoring uses the
            // static profile table (cost, latency tier, best_for match).
            // Live performance data (success rate, p50 latency) merges in
            // Layer 3. Falls back to the first keyword provider only if no
            // profiled provider is configured (free providers only).
            let candidates: Vec<&dyn WebSearchProvider> = self
                .search_providers
                .iter()
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
            let picked = pick_best_provider(&candidates);
            self.search_single_provider(picked.kind(), query).await?
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

    /// Stub provider for testing `pick_best_provider`. Returns a fixed kind
    /// and keyword capability — enough for the scorer to exercise the
    /// static profile table.
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

    /// `pick_best_provider` must select the lowest-cost, fastest-latency
    /// provider from the static profile table — not blind first-Ok-wins.
    /// This pins the deliberate-selection behavior: `quick` strategy picks
    /// Brave ($0.002, Fast) over Tavily ($0.003, Fast) over Exa ($0.01, Medium).
    #[test]
    fn pick_best_provider_prefers_lower_cost_faster_latency() {
        let brave = StubProvider { kind: "brave" };
        let tavily = StubProvider { kind: "tavily" };
        let exa = StubProvider { kind: "exa" };
        let candidates: Vec<&dyn WebSearchProvider> = vec![&exa, &tavily, &brave];
        let picked = pick_best_provider(&candidates);
        assert_eq!(
            picked.kind(),
            "brave",
            "quick strategy must pick the lowest-cost fastest-latency provider, \
             not blind first-Ok-wins fallback"
        );
    }

    /// When no candidate has a profile (free providers only),
    /// `pick_best_provider` falls back to the first candidate with a neutral
    /// score — deterministic via alphabetical tiebreak.
    #[test]
    fn pick_best_provider_unprofiled_falls_back_alphabetically() {
        let arxiv = StubProvider { kind: "arxiv" };
        let semantic = StubProvider {
            kind: "semantic_scholar",
        };
        let candidates: Vec<&dyn WebSearchProvider> = vec![&arxiv, &semantic];
        let picked = pick_best_provider(&candidates);
        assert_eq!(
            picked.kind(),
            "arxiv",
            "unprofiled providers should tie-break alphabetically for determinism"
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
        let pool = ProviderPool::new(vec![Box::new(brave)], Vec::new(), Vec::new(), None);
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
        let pool = ProviderPool::new(vec![Box::new(brave)], Vec::new(), Vec::new(), None);
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
}
