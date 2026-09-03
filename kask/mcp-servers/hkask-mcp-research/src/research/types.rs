//! Core types for the hKask research service.

mod freshness;
mod ranking;
mod rate_limiter;
mod validation;

use hkask_mcp_server::AnyJsonValue;
use hkask_mcp_server::server::McpToolError;
use hkask_types::McpErrorKind;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Constants ──

pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const BRAVE_API_BASE: &str = "https://api.search.brave.com/res/v1";
pub(crate) const FIRECRAWL_API_BASE: &str = "https://api.firecrawl.dev/v2";
pub(crate) const TAVILY_API_BASE: &str = "https://api.tavily.com";
pub const SERPAPI_BASE: &str = "https://serpapi.com/search";
pub(crate) const EXA_API_BASE: &str = "https://api.exa.ai";
pub const DEFAULT_CACHE_TTL_SECS: u64 = 300;
pub(crate) const MAX_CACHE_TTL_SECS: u64 = 7200;
pub(crate) const DEFAULT_CACHE_MAX_ENTRIES: usize = 50;
pub(crate) const MAX_CACHE_MAX_ENTRIES: usize = 200;
pub(crate) const MAX_CACHE_VALUE_BYTES: usize = 1_048_576;
pub(crate) const RRF_K: u64 = 60;
pub(crate) const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;
pub(crate) const MAX_QUERY_LENGTH: usize = 400;
pub(crate) const MAX_URL_LENGTH: usize = 2048;
pub(crate) const MAX_INSTRUCTION_LENGTH: usize = 2000;
pub(crate) const MAX_JSON_PROMPT_LENGTH: usize = 4000;
pub(crate) const MAX_JSON_SCHEMA_BYTES: usize = 32_768;

// ── Re-exports ──

pub(crate) use freshness::{Freshness, freshness_brave, freshness_serpapi};
pub(crate) use ranking::{apply_rerank, llm_rerank, rrf_score};
pub use rate_limiter::RateLimiter;
pub(crate) use validation::{COMPOUND_PROVIDER_TIMEOUT_SECS, sanitize_health_error};

// ── Provider profiles (metacognitive lookup table) ──

/// Latency tier for a provider's typical response time.
/// Used by the recommendation scorer as a tiebreaker among providers with
/// similar capability/cost profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LatencyTier {
    /// < 1s typical
    Fast,
    /// 1-3s typical
    Medium,
    /// > 3s typical
    Slow,
}

/// Static profile of a web search provider: cost, latency, strengths,
/// weaknesses, and the query intents it's best for. This is the
/// metacognitive lookup table — the model reads it (via
/// `web_recommend_provider` or the `provider_profiles` field on `web_search`)
/// to pick a provider deliberately rather than relying on blind fallback.
///
/// The static table is merged with live performance data (success rate,
/// p50 latency from `reg.web.provider` spans) at runtime to produce a scored
/// recommendation. See `ProviderPerformance` and `score_providers`.
///
/// Not `Deserialize` — the static table uses `&'static str` slices which
/// can't be deserialized. `ProviderProfileOutput` is the serializable view.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderProfile {
    pub kind: &'static str,
    /// Approximate cost per call in USD. 0.0 for free providers.
    pub cost_per_call_usd: f64,
    pub latency_tier: LatencyTier,
    pub strengths: &'static [&'static str],
    pub weaknesses: &'static [&'static str],
    /// Query intents this provider is best for: "news", "academic",
    /// "semantic", "freshness", "general", "transcript".
    pub best_for: &'static [&'static str],
}

/// The canonical provider profile table. Single source of truth consumed by:
/// - `web_recommend_provider` (scores providers against a query)
/// - `web_search` output (`provider_profiles` field for metacognitive surfacing)
/// - `score_providers` (merges static profile with live performance)
///
/// Keep entries aligned with the providers registered in `build_provider_pool`.
/// Free providers (arxiv, semantic_scholar) are intentionally absent — they're
/// always-on fallbacks, not selectable via the `provider` field.
pub static PROVIDER_PROFILES: &[ProviderProfile] = &[
    ProviderProfile {
        kind: "tavily",
        cost_per_call_usd: 0.003,
        latency_tier: LatencyTier::Fast,
        strengths: &["fast", "answer boxes", "content previews", "cheap"],
        weaknesses: &["smaller index than Google/Bing", "no content extraction"],
        best_for: &["general", "semantic"],
    },
    ProviderProfile {
        kind: "brave",
        cost_per_call_usd: 0.002,
        latency_tier: LatencyTier::Fast,
        strengths: &["independent index", "news", "freshness filters", "cheap"],
        weaknesses: &["no content extraction", "no semantic search"],
        best_for: &["news", "freshness", "general"],
    },
    ProviderProfile {
        kind: "exa",
        cost_per_call_usd: 0.01,
        latency_tier: LatencyTier::Medium,
        strengths: &["neural/semantic search", "content previews", "find-similar"],
        weaknesses: &["higher cost", "smaller index for generic queries"],
        best_for: &["semantic", "academic", "research"],
    },
    ProviderProfile {
        kind: "firecrawl",
        cost_per_call_usd: 0.005,
        latency_tier: LatencyTier::Medium,
        strengths: &[
            "search + extract + browse",
            "markdown output",
            "JS-heavy pages",
        ],
        weaknesses: &["smaller search index", "higher latency on browse"],
        best_for: &["general", "semantic"],
    },
    ProviderProfile {
        kind: "serpapi",
        cost_per_call_usd: 0.004,
        latency_tier: LatencyTier::Medium,
        strengths: &["Google index", "news", "freshness", "YouTube transcripts"],
        weaknesses: &["no content extraction", "higher cost", "rate limits"],
        best_for: &["news", "freshness", "transcript"],
    },
];

/// Look up a provider's static profile by kind. Returns `None` for unknown
/// providers (including free providers not in the table).
pub fn provider_profile(kind: &str) -> Option<&'static ProviderProfile> {
    PROVIDER_PROFILES.iter().find(|p| p.kind == kind)
}

// ── Provider recommendation (metacognitive scoring) ──

/// A single provider's recommendation: score, rationale, and profile.
/// Lower `score` is better (mirrors `score_static`: cost + latency penalty).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRecommendation {
    pub kind: String,
    /// Composite score: lower is better. Static layer = cost + latency
    /// penalty + intent-match bonus. Live layer (Layer 3) adds success-rate
    /// and p50-latency penalties from `reg.web.provider` spans.
    pub score: f64,
    /// Human-readable rationale for the score (which factors won/lost).
    pub rationale: String,
    pub cost_per_call_usd: f64,
    pub latency_tier: LatencyTier,
    pub strengths: Vec<String>,
    pub weaknesses: Vec<String>,
    pub best_for: Vec<String>,
    /// Whether this provider is currently configured (has an API key).
    /// `false` for providers in the static table but not wired in `build_provider_pool`.
    pub configured: bool,
    /// Live success rate over the rolling window (0.0-1.0). `None` when
    /// fewer than `MIN_SAMPLES_FOR_LIVE` observations exist — the static
    /// profile alone drives selection in that case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_success_rate: Option<f64>,
    /// Live p50 (median) latency in milliseconds over the rolling window.
    /// `None` when fewer than `MIN_SAMPLES_FOR_LIVE` observations exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_p50_latency_ms: Option<u64>,
    /// Number of observations in the rolling window for this provider.
    /// `None` when fewer than `MIN_SAMPLES_FOR_LIVE` observations exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_sample_count: Option<usize>,
}


// ── Request types ──

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    pub query: String,
    pub num_results: Option<u32>,
    pub include_domains: Option<Vec<String>>,
    pub exclude_domains: Option<Vec<String>>,
    pub freshness: Option<String>,
    pub strategy: Option<String>,
    /// Deliberate provider selection without an explicit `provider`: when
    /// `provider` is None and `intent` is set (news, academic, semantic,
    /// freshness, general, transcript), the tool scores the configured
    /// providers against (query, intent) — cost, latency, strengths,
    /// capability match — and queries the top recommendation as a
    /// single-provider call. The ranking is surfaced in the output's
    /// `provider_recommendations` and the choice in `selected_provider`.
    /// The former two-step web_recommend_provider + web_search(provider)
    /// pattern, folded in.
    pub intent: Option<String>,
    /// Explicit provider override: "tavily", "brave", "exa", "firecrawl",
    /// "serpapi". When set, only that provider is queried — no fusion, no
    /// fallback. When `None` with an `intent`, the top-recommended provider
    /// is queried; with neither, the `strategy` field selects providers
    /// (quick = best-scored single keyword provider; web/news/deep = fan out
    /// with RRF fusion).
    pub provider: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindSimilarRequest {
    pub url: String,
    pub num_results: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExtractRequest {
    pub url: String,
    pub format: Option<String>,
    pub json_prompt: Option<String>,
    /// Optional JSON Schema describing the structured output to extract.
    ///
    /// Accepts arbitrary JSON. Typed as [`AnyJsonValue`] (not `serde_json::Value`)
    /// so the generated tool input schema is the empty object `{}` rather than the
    /// bare boolean `true` schemars emits for `Value` — Ollama rejects boolean
    /// property schemas with `400 cannot unmarshal bool into ... api.ToolProperty`.
    pub json_schema: Option<AnyJsonValue>,
    pub main_content_only: Option<bool>,
    pub wait_for_ms: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrowseRequest {
    pub url: String,
    pub instruction: Option<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct EvaluateEvidenceRequest {
    /// The research question to evaluate evidence against.
    pub question: String,
    /// Artifacts to evaluate (URLs + optional content/metadata from web_search/web_extract).
    pub artifacts: Vec<EvaluateArtifact>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct EvaluateArtifact {
    pub url: String,
    /// Title of the source (from SearchResultOutput.title).
    pub title: Option<String>,
    /// Publication date (from SearchResultOutput.published).
    pub published: Option<String>,
    /// Source domain (from SearchResultOutput.source).
    pub source: Option<String>,
    /// Extracted content (from web_extract).
    pub content: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CiteSourcesRequest {
    /// Sources to cite (URLs + metadata from web_search/web_extract results).
    pub sources: Vec<CiteSource>,
    /// Citation style.
    pub style: CiteStyle,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CiteSource {
    pub url: String,
    pub title: Option<String>,
    pub published: Option<String>,
    pub source: Option<String>,
    pub authors: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CiteStyle {
    Apa,
    Bibtex,
    Chicago,
    Json,
}

// ── Result types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub source: Option<String>,
    pub published: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedContent {
    pub url: String,
    pub content: String,
    pub format: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseResult {
    pub url: String,
    pub content: String,
    pub instruction: Option<String>,
    pub actions_taken: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub num_results: u32,
    pub include_domains: Vec<String>,
    pub exclude_domains: Vec<String>,
    pub(crate) freshness: Option<Freshness>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractOptions {
    pub format: String,
    pub json_prompt: Option<String>,
    pub json_schema: Option<serde_json::Value>,
    pub main_content_only: bool,
    pub wait_for_ms: u64,
}

// ── Error type ──

#[derive(Debug, thiserror::Error)]
pub enum WebError {
    #[error("Bad arguments: {0}")]
    BadArgs(String),
    #[error("Provider unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("Provider error: {0}")]
    ProviderError(String),
    #[error("Rate limited: {0}")]
    RateLimited(String),
    #[error("No provider available")]
    NoProvider,
    /// No provider configured because the gating credential is missing. The
    /// message names the env var(s) the operator must set. Distinct from
    /// `NoProvider` (genuine "no provider available, possibly transient") so
    /// the `From<WebError>` impl can map this to `permission_denied` rather
    /// than `unavailable` — letting operators distinguish "provider is down"
    /// from "provider was never configured."
    #[error("{0}")]
    NoProviderConfigured(String),
}

impl WebError {
    pub fn kind(&self) -> McpErrorKind {
        match self {
            Self::BadArgs(_) => McpErrorKind::InvalidArgument,
            Self::ProviderUnavailable(_) => McpErrorKind::Unavailable,
            Self::ProviderError(_) => McpErrorKind::Internal,
            Self::RateLimited(_) => McpErrorKind::RateLimited,
            Self::NoProvider => McpErrorKind::Unavailable,
            Self::NoProviderConfigured(_) => McpErrorKind::PermissionDenied,
        }
    }
}

impl From<WebError> for McpToolError {
    fn from(e: WebError) -> Self {
        McpToolError::new(e.kind(), e.to_string())
    }
}

// ── Capability / provider types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchCapability {
    Keyword,
    News,
    Freshness,
    Semantic,
    Transcript,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedResult {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub source: Option<String>,
    pub published: Option<String>,
    pub rrf_score: f64,
    pub provider_count: usize,
    pub providers: Vec<String>,
    pub best_rank: Option<usize>,
    pub content_preview: Option<String>,
    pub semantic_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerBox {
    pub title: Option<String>,
    pub snippet: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub kind: String,
    pub capabilities: Vec<SearchCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderFailureRecord {
    pub kind: String,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct CompoundSearchResult {
    pub query: String,
    pub strategy: String,
    pub results: Vec<RankedResult>,
    pub answer_box: Option<AnswerBox>,
    pub related_questions: Vec<String>,
    pub providers_queried: Vec<ProviderInfo>,
    pub providers_succeeded: Vec<String>,
    pub providers_failed: Vec<ProviderFailureRecord>,
    pub total_before_dedup: usize,
    pub duplicates_removed: usize,
}

// ── Strategy & filter types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchStrategy {
    Quick,
    Web,
    News,
    Deep,
}

impl SearchStrategy {
    pub(crate) fn provider_filter(&self) -> ProviderFilter {
        match self {
            Self::Quick => ProviderFilter::Capabilities(vec![SearchCapability::Keyword]),
            Self::Web => ProviderFilter::All,
            Self::News => ProviderFilter::Capabilities(vec![SearchCapability::News]),
            Self::Deep => ProviderFilter::All,
        }
    }
}

pub(crate) enum ProviderFilter {
    All,
    Capabilities(Vec<SearchCapability>),
}

impl std::fmt::Display for SearchStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Quick => "quick",
            Self::Web => "web",
            Self::News => "news",
            Self::Deep => "deep",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for SearchStrategy {
    type Err = WebError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "quick" => Ok(Self::Quick),
            "web" | "semantic" => Ok(Self::Web),
            "news" => Ok(Self::News),
            "deep" | "research" => Ok(Self::Deep),
            _ => Err(WebError::BadArgs(format!(
                "Unknown strategy: {s}. Use: quick, web, news, deep"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RerankSignal {
    Recency,
    Semantic,
    ContentQuality,
}

// ── Output types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SearchResultOutput {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub source: Option<String>,
    pub published: Option<String>,
    pub content_preview: Option<String>,
    /// Search providers that returned this result (for source classification).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<String>,
}

impl From<&RankedResult> for SearchResultOutput {
    fn from(r: &RankedResult) -> Self {
        Self {
            title: r.title.clone(),
            url: r.url.clone(),
            description: r.description.clone(),
            source: r.source.clone(),
            published: r.published.clone(),
            content_preview: r.content_preview.clone(),
            providers: r.providers.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SearchOutput {
    pub query: String,
    pub strategy: String,
    pub results: Vec<SearchResultOutput>,
    pub answer_box: Option<AnswerBox>,
    pub related_questions: Vec<String>,
    pub count: usize,
    /// Providers that were queried but failed, so callers can distinguish a
    /// genuine zero-result search from one where every provider errored.
    pub providers_failed: Vec<ProviderFailureRecord>,
    /// The provider that was actually queried when `provider` was set or
    /// `quick` strategy selected a single provider. `None` for compound
    /// strategies (web/news/deep fan out across multiple).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_provider: Option<String>,
    /// Static profiles of all configured providers, for metacognitive
    /// surfacing — the model reads this to pick deliberately next time.
    /// Empty when no profiles are registered (e.g. only free providers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provider_profiles: Vec<ProviderProfileOutput>,
    /// The provider ranking computed when `intent`-driven selection ran
    /// (provider unset, intent set): every configured/unconfigured provider
    /// with score, rationale, and profile. Empty for explicit `provider`
    /// calls and compound strategies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provider_recommendations: Vec<ProviderRecommendation>,
    /// How the deep strategy's rerank stage ran. `mode: "llm"` when the
    /// templated LLM scoring calls produced the ordering; `mode:
    /// "heuristic"` when every scoring call failed and the heuristic RRF
    /// order was kept. `reason` is present on any degraded outcome — full
    /// failure (naming the cause) or partial failure (naming how many
    /// scoring calls failed) — and `None` only when every call succeeded.
    /// `None` for non-deep strategies (they do not rerank).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rerank: Option<RerankInfo>,
}

/// Surfaced rerank stage outcome for the deep strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RerankInfo {
    /// "llm" or "heuristic"
    pub mode: String,
    /// Why the rerank stage degraded — present on heuristic fallback (all
    /// scoring calls failed) or partial failure (some calls failed);
    /// `None` only when every scoring call succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Serializable view of a `ProviderProfile` for tool output. Owned `String`s
/// because the static table uses `&'static str` (not serializable as owned).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProviderProfileOutput {
    pub kind: String,
    pub cost_per_call_usd: f64,
    pub latency_tier: LatencyTier,
    pub strengths: Vec<String>,
    pub weaknesses: Vec<String>,
    pub best_for: Vec<String>,
}

impl From<&ProviderProfile> for ProviderProfileOutput {
    fn from(p: &ProviderProfile) -> Self {
        Self {
            kind: p.kind.to_string(),
            cost_per_call_usd: p.cost_per_call_usd,
            latency_tier: p.latency_tier,
            strengths: p.strengths.iter().map(|s| s.to_string()).collect(),
            weaknesses: p.weaknesses.iter().map(|s| s.to_string()).collect(),
            best_for: p.best_for.iter().map(|s| s.to_string()).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SearchMetadata {
    pub strategy: String,
    pub providers_queried: Vec<ProviderInfo>,
    pub providers_succeeded: Vec<String>,
    pub providers_failed: Vec<ProviderFailureRecord>,
    pub total_before_dedup: usize,
    pub duplicates_removed: usize,
    pub top_rrf_scores: Vec<f64>,
}

impl From<&CompoundSearchResult> for SearchMetadata {
    fn from(c: &CompoundSearchResult) -> Self {
        Self {
            strategy: c.strategy.clone(),
            providers_queried: c.providers_queried.clone(),
            providers_succeeded: c.providers_succeeded.clone(),
            providers_failed: c.providers_failed.clone(),
            total_before_dedup: c.total_before_dedup,
            duplicates_removed: c.duplicates_removed,
            top_rrf_scores: c.results.iter().take(5).map(|r| r.rrf_score).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FindSimilarResultOutput {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub source: Option<String>,
    pub published: Option<String>,
    pub semantic_score: Option<f64>,
    pub content_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FindSimilarOutput {
    pub source_url: String,
    pub results: Vec<FindSimilarResultOutput>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ExtractOutput {
    pub url: String,
    pub format: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BrowseOutput {
    pub url: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    pub actions_taken: Vec<String>,
}

// ── Health / ping types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHealthEntry {
    pub kind: String,
    pub healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PingOutput {
    pub status: String,
    pub version: String,
    pub providers: Vec<ProviderHealthEntry>,
}

// ── Capability context ──
//
// N1 (2026-07-20): CapabilityContext removed. Every tool call in the MCP
// server passed `None` for `ctx`, making the tool-allowlist check dead code.
// Tool dispatch is enforced at the membrane (GovernedTool), not at the
// port — see kask/docs/diataxis/hkask-mcp-server/explanation.md. The port-level
// check was speculative and never wired. If per-tool capability gating is
// needed at the port in the future, reintroduce it with a real wiring plan.
