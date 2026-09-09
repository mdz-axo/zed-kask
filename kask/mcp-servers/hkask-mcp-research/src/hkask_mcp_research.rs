#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]

pub mod research;

// Re-export service crate modules for test compatibility
pub use crate::research::db;

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;

use hkask_mcp_server::server::{
    CredentialRequirement, McpToolError, ServerContext, execute_tool, map_join_error,
    resolve_db_passphrase, validate_tool_url_with_dns,
};
use reqwest::Client;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use rusqlite::Connection;

use crate::research::db::*;
use crate::research::{
    AnnotateResearchRunRequest, ArtifactScore, BeginResearchRunRequest, BrowseOutput,
    BrowseRequest, CiteSourcesRequest, CiteStyle, Continuation, DEFAULT_CACHE_MAX_ENTRIES,
    DEFAULT_CACHE_TTL_SECS, DEFAULT_PROFILE, DeleteSyntheticRequest, DiscoverRequest,
    EditTagRequest, EvaluateEvidenceRequest, EvidenceReport, ExtractOptions, ExtractOutput,
    ExtractRequest, FetchRequest, FindSimilarOutput, FindSimilarRequest, FindSimilarResultOutput,
    GetEntriesRequest, GetResearchRunRequest, ImportOpmlRequest, ListSubscriptionsRequest,
    MAX_CACHE_MAX_ENTRIES, MAX_CACHE_TTL_SECS, MAX_INSTRUCTION_LENGTH, MAX_JSON_PROMPT_LENGTH,
    MAX_JSON_SCHEMA_BYTES, MAX_QUERY_LENGTH, MAX_URL_LENGTH, MarkReadRequest, NewResearchRun,
    PingOutput, ProviderProfileOutput, ProviderRecommendation, RateLimiter, RerankInfo,
    RerankOutcome, ResponseCache, RunSourceRecord, SearchMetadata, SearchOutput, SearchQuery,
    SearchRequest, SearchResultOutput, SearchStrategy, SensitivityStatus, SubscribeRequest,
    SynthesizeRequest, UnreadCountRequest, UnsubscribeRequest, WebSearchPort, build_provider_pool,
    cache_key, discover_feeds, fetch_feed, llm_rerank, provider_profile, score_evidence_set,
    validated_fetch_client,
};

// ── Constants ──

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_PAGE_SIZE: usize = 20;
const MAX_PAGE_SIZE: usize = 100;
const RATE_LIMIT_MAX_REQUESTS: u32 = 30;
const RATE_LIMIT_WINDOW_SECS: u64 = 60;
/// Maximum response body size for feed/synthetic fetches (64 MiB). Prevents
/// OOM from malicious or misconfigured feeds that serve unbounded content.
const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

// ── ResearchServer ──

hkask_mcp_server::mcp_server!(
    pub struct ResearchServer {
        pub pool: Arc<dyn WebSearchPort>,
        pub cache: Arc<ResponseCache>,
        pub rate_limiter: RateLimiter,
        pub research_db: Option<r2d2::Pool<hkask_storage::SqliteConnectionManager>>,
        pub rss_client: Client,
        /// Strict-policy fetch client for `rss_discover_feeds`: every redirect
        /// hop re-validated, every connect-time DNS resolution gated, proxies
        /// disabled (`validated_fetch_client`). The permissive `rss_client`
        /// above is deliberately different — user-curated RSS fetches may live
        /// on local networks by ratified policy, so they keep default redirect
        /// following. Do not use `rss_client` for arbitrary user URLs.
        pub discover_client: Client,
        /// Inference port for the deep strategy's templated LLM rerank.
        /// Resolved via `hkask_inference::resolve_inference_port()` — a
        /// `LazyInferencePort` that bridges to zed's LanguageModelRegistry
        /// over `HKASK_INFERENCE_SOCKET` on each call.
        pub inference_port: Arc<dyn hkask_types::InferencePort>,
        /// The deep strategy's rerank model — resolved ONCE at the
        /// construction seam from `HKASK_RERANK_MODEL` (the visible
        /// `kask.models.rerank_model` setting). `None` = not configured:
        /// the deep strategy degrades to heuristic order with a fail-visible
        /// reason naming the setting — never a hidden constant (the
        /// operator's no-hidden-models spec).
        pub rerank_model: Option<String>,
    }
);

// ── RSS helpers ──

pub(crate) fn spawn_db<F, T>(
    pool: r2d2::Pool<hkask_storage::SqliteConnectionManager>,
    f: F,
) -> tokio::task::JoinHandle<Result<T, anyhow::Error>>
where
    F: FnOnce(&Connection) -> Result<T, anyhow::Error> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().map_err(|e| anyhow::anyhow!("pool get: {e}"))?;
        f(&conn)
    })
}

/// Handle the result of `spawn_db`: maps Ok(Ok) → Ok(v), Ok(Err)/Err → Err(McpToolError).
macro_rules! handle_db_result {
    ($result:expr, $ok:expr) => {
        match $result {
            Ok(Ok(v)) => {
                let v: serde_json::Value = $ok(v);
                Ok(v)
            }
            Ok(Err(e)) => Err($crate::map_db_error(e)),
            Err(e) => Err(map_join_error(e, "db task failed")),
        }
    };
}

/// Classify an `anyhow::Error` from a `spawn_db` closure (almost always a wrapped
/// `rusqlite::Error`) into the MCP wire-level `McpToolError` kind by the
/// underlying SQLite error code: constraint violations are user-input problems
/// (`invalid_argument`), a missing DB row is `not_found`, a read-only DB is a
/// failed precondition, lock/busy/cannot-open/full/IO failures are availability
/// issues (`unavailable`), permission errors are `permission_denied`, and the
/// remaining SQL/infra failures remain `internal`.
pub(crate) fn map_db_error(e: anyhow::Error) -> McpToolError {
    let message = e.to_string();
    if let Some(rusqlite::Error::SqliteFailure(ffi, _)) = e.downcast_ref::<rusqlite::Error>() {
        return match ffi.code {
            rusqlite::ErrorCode::ConstraintViolation => McpToolError::invalid_argument(message),
            rusqlite::ErrorCode::PermissionDenied => McpToolError::permission_denied(message),
            rusqlite::ErrorCode::NotFound => McpToolError::not_found(message),
            rusqlite::ErrorCode::ReadOnly => McpToolError::failed_precondition(message),
            rusqlite::ErrorCode::CannotOpen
            | rusqlite::ErrorCode::DatabaseBusy
            | rusqlite::ErrorCode::DatabaseLocked
            | rusqlite::ErrorCode::DiskFull
            | rusqlite::ErrorCode::SystemIoFailure => McpToolError::unavailable(message),
            _ => McpToolError::internal(message), // rr0044-ok: mapper-internal-arm
        };
    }
    McpToolError::internal(message) // rr0044-ok: mapper-internal-arm
}

/// Require the research database, returning an Err if not configured.
macro_rules! require_research_db {
    ($self:expr) => {
        match &$self.research_db {
            Some(db) => db.clone(),
            None => {
                return Err(McpToolError::permission_denied(
                    "Research database not configured. Set HKASK_RESEARCH_DB and HKASK_DB_PASSPHRASE.",
                ));
            }
        }
    };
}

// ── Tool implementations ──

#[tool_router(server_handler)]
impl ResearchServer {
    // ═══════════════════ Web tools ═══════════════════

    #[tool(description = "Liveness and provider health check")]
    pub async fn web_ping(&self) -> Result<String, McpToolError> {
        execute_tool(self, "web_ping", async {
            if let Err(e) = self.rate_limiter.check("web_ping") {
                tracing::warn!(
                    target: "hkask.web",
                    error = %e,
                    "web_ping rate limited"
                );
                return Err(McpToolError::from(e));
            }

            let providers = self.pool.health_check().await;
            let output = PingOutput {
                status: "ok".to_string(),
                version: SERVER_VERSION.to_string(),
                providers,
            };
            Ok(serde_json::to_value(&output)
                .unwrap_or_else(|_| serde_json::json!({"error": "serialization failed"})))
        })
        .await
    }

    #[tool(description = "Search the web with RRF fusion across providers. \
         Set `provider` to query a single named provider (tavily, brave, exa, \
         firecrawl, serpapi) — no fusion, no fallback. Or set `intent` (news, \
         academic, semantic, freshness, general, transcript) to have the tool \
         score the configured providers against the query and pick the top \
         recommendation for you — the ranking is surfaced in \
         provider_recommendations. When both are None, `strategy` selects: \
         quick (best-scored single keyword provider), web (all, RRF fusion), \
         news (news-capable), deep (all + 2x results + content extraction).")]
    pub async fn web_search(
        &self,
        Parameters(req): Parameters<SearchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "web_search", async {
            self.rate_limiter.check("web_search")?;

            if req.query.is_empty() {
                return Err(McpToolError::invalid_argument("query must not be empty"));
            }
            if req.query.len() > MAX_QUERY_LENGTH {
                return Err(McpToolError::invalid_argument(format!(
                    "query exceeds maximum length of {} characters",
                    MAX_QUERY_LENGTH
                )));
            }

            let strat = match req.strategy.as_deref() {
                Some(s) => s.parse::<SearchStrategy>().map_err(McpToolError::from)?,
                None => SearchStrategy::Quick,
            };

            let num_results = req.num_results.unwrap_or(10).min(50);

            let freshness = match req.freshness.as_deref() {
                Some(f) => Some(
                    f.parse::<crate::research::types::Freshness>()
                        .map_err(McpToolError::from)?,
                ),
                None => None,
            };

            let fingerprint = self.pool.provider_fingerprint();
            let ckey = cache_key(
                &strat.to_string(),
                &req.query,
                &serde_json::json!({
                    "num_results": num_results,
                    "freshness": freshness,
                    "include_domains": req.include_domains,
                    "exclude_domains": req.exclude_domains,
                    "provider": req.provider,
                    "intent": req.intent,
                }),
                &fingerprint,
            );

            // A run-scoped search bypasses the response-cache read so the
            // run ledger records what THIS request returned; the recording
            // happens on the fresh path below.
            if req.run_id.is_none()
                && let Some(cached) = self.cache.get(&ckey).await
            {
                return Ok(cached);
            }

            // Deliberate single-provider selection without an explicit
            // provider (the former web_recommend_provider + web_search(provider)
            // two-step, folded in): score the configured providers against
            // (query, intent) and query the top recommendation. The ranking is
            // surfaced in the output so the choice is auditable.
            let mut provider = req.provider.clone();
            let mut provider_recommendations: Vec<ProviderRecommendation> = Vec::new();
            if provider.is_none()
                && let Some(ref intent) = req.intent
            {
                provider_recommendations = self.pool.score_providers(&req.query, Some(intent));
                if let Some(top) = provider_recommendations.iter().find(|r| r.configured) {
                    provider = Some(top.kind.clone());
                }
            }

            let search_query = SearchQuery {
                query: req.query.clone(),
                num_results,
                include_domains: req.include_domains.unwrap_or_default(),
                exclude_domains: req.exclude_domains.unwrap_or_default(),
                freshness,
            };

            let mut compound = self
                .pool
                .search(&search_query, strat, provider.as_deref())
                .await
                .map_err(McpToolError::from)?;

            compound.results.truncate(num_results as usize);

            // Deep strategy rerank stage: ONE templated rerank call
            // carrying all candidates as documents, routed through the
            // inference IPC bridge to the provider's rerank endpoint. The
            // model resolves from `HKASK_RERANK_MODEL` (the visible
            // `kask.models.rerank_model` setting) — unconfigured is a
            // fail-visible degraded outcome naming the setting, never a
            // hidden constant (the operator's no-hidden-models spec). Every
            // degraded outcome is surfaced in `rerank` — never a silent
            // fallback.
            let rerank = if strat == SearchStrategy::Deep && compound.results.len() >= 2 {
                let outcome = match self.rerank_model.as_deref() {
                    Some(model) => {
                        llm_rerank(
                            self.inference_port.as_ref(),
                            &req.query,
                            &mut compound.results,
                            model,
                        )
                        .await
                    }
                    None => RerankOutcome {
                        scored: 0,
                        failed: compound.results.len(),
                        first_error: Some(
                            "no rerank model configured — set \
                             kask.models.rerank_model (injected as \
                             HKASK_RERANK_MODEL); kask never falls back to a \
                             hidden code constant"
                                .to_string(),
                        ),
                    },
                };
                if outcome.scored == 0 {
                    tracing::warn!(
                        target: "hkask.web",
                        error = ?outcome.first_error,
                        "LLM rerank failed — keeping heuristic RRF order"
                    );
                    Some(RerankInfo {
                        mode: "heuristic".to_string(),
                        reason: outcome
                            .first_error
                            .or_else(|| Some("no candidates scored".to_string())),
                    })
                } else if outcome.failed > 0 {
                    Some(RerankInfo {
                        mode: "llm".to_string(),
                        reason: Some(format!(
                            "{} of {} rerank scoring calls failed; unscored results \
                                 kept heuristic order at the end",
                            outcome.failed,
                            outcome.scored + outcome.failed
                        )),
                    })
                } else {
                    Some(RerankInfo {
                        mode: "llm".to_string(),
                        reason: None,
                    })
                }
            } else {
                None
            };

            // Surface which provider was actually used when a single
            // provider was selected (explicit override, intent-driven pick,
            // or quick strategy).
            let selected_provider = if provider.is_some() || strat == SearchStrategy::Quick {
                compound
                    .providers_succeeded
                    .first()
                    .cloned()
                    .or_else(|| provider.clone())
            } else {
                None
            };

            // Surface the static profiles of all configured providers so
            // the model has metacognitive context for its next call.
            let provider_profiles: Vec<ProviderProfileOutput> = self
                .pool
                .provider_kinds()
                .iter()
                .filter_map(|kind| provider_profile(kind).map(ProviderProfileOutput::from))
                .collect();

            let metadata = SearchMetadata::from(&compound);
            tracing::info!(
                target: "hkask.web",
                strategy = %metadata.strategy,
                selected_provider = ?selected_provider.as_ref(),
                providers_queried = ?metadata.providers_queried,
                providers_succeeded = ?metadata.providers_succeeded,
                providers_failed = ?metadata.providers_failed,
                total_before_dedup = metadata.total_before_dedup,
                duplicates_removed = metadata.duplicates_removed,
                top_rrf_scores = ?metadata.top_rrf_scores,
                "Regulation web_search metadata"
            );

            let search_output = SearchOutput {
                query: compound.query.clone(),
                strategy: compound.strategy.clone(),
                results: compound
                    .results
                    .iter()
                    .map(SearchResultOutput::from)
                    .collect(),
                answer_box: compound.answer_box.clone(),
                related_questions: compound.related_questions.clone(),
                count: compound.results.len(),
                providers_failed: compound.providers_failed.clone(),
                selected_provider,
                provider_profiles,
                provider_recommendations,
                rerank,
            };

            let output = serde_json::to_value(&search_output)
                .unwrap_or_else(|_| serde_json::json!({ "error": "serialization failed" }));

            // Cache only clean responses. A compound carrying provider
            // failures — in single-provider mode that is an empty result
            // plus a failure record — must not be cached: a transient
            // provider failure would otherwise be replayed as a
            // "successful" empty result for the full cache TTL.
            if compound.providers_failed.is_empty() {
                self.cache.insert(ckey, output.clone()).await;
            }

            // Server-side ledger append (the non-repudiation path): record
            // what this tool actually returned under the run. Best-effort —
            // the outcome is surfaced as a run_ledger note, never swallowed.
            let mut output = output;
            if let Some(run_id) = req.run_id.as_deref() {
                let records: Vec<RunSourceRecord> = search_output
                    .results
                    .iter()
                    .map(|result| RunSourceRecord {
                        url: result.url.clone(),
                        provider: result.providers.first().cloned(),
                        title: Some(result.title.clone()),
                        published: result.published.clone(),
                        source: result.source.clone(),
                        excerpt: result
                            .content_preview
                            .clone()
                            .or_else(|| result.description.clone()),
                    })
                    .collect();
                let note = self.append_run_ledger(run_id, records).await;
                output["run_ledger"] = note;
            }

            Ok(output)
        })
        .await
    }

    #[tool(description = "Find pages similar to a given URL using Exa findSimilar")]
    pub async fn web_find_similar(
        &self,
        Parameters(FindSimilarRequest {
            url,
            num_results,
            run_id,
        }): Parameters<FindSimilarRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "web_find_similar", async {
            self.rate_limiter.check("web_find_similar")?;

            validate_tool_url_with_dns(&url).await?;

            let num = num_results.unwrap_or(5).min(20);

            // Not cached: web_find_similar skips the response cache (unlike
            // web_search and web_extract). A find-similar result is sensitive
            // to the source URL's evolving neighbourhood and stale quickly.
            tracing::debug!(target: "hkask.web", "web_find_similar cache miss (not cached)");

            let similar = self
                .pool
                .find_similar(&url, num)
                .await
                .map_err(McpToolError::from)?;

            let results: Vec<FindSimilarResultOutput> = similar
                .results
                .into_iter()
                .map(|r| {
                    let key = r.url.to_lowercase();
                    FindSimilarResultOutput {
                        title: r.title,
                        url: r.url,
                        description: r.description,
                        source: r.source,
                        published: r.published,
                        semantic_score: similar.semantic_scores.get(&key).copied(),
                        content_preview: similar.content_previews.get(&key).cloned(),
                    }
                })
                .collect();

            let fs_output = FindSimilarOutput {
                source_url: url,
                count: results.len(),
                results,
            };

            let mut output = serde_json::to_value(&fs_output)
                .unwrap_or_else(|_| serde_json::json!({ "error": "serialization failed" }));

            // Server-side ledger append (see web_search.run_id). The origin
            // is Exa's findSimilar API — that is the server-observed origin.
            if let Some(run_id) = run_id.as_deref() {
                let records: Vec<RunSourceRecord> = fs_output
                    .results
                    .iter()
                    .map(|result| RunSourceRecord {
                        url: result.url.clone(),
                        provider: Some("exa".to_string()),
                        title: Some(result.title.clone()),
                        published: result.published.clone(),
                        source: result.source.clone(),
                        excerpt: result
                            .content_preview
                            .clone()
                            .or_else(|| result.description.clone()),
                    })
                    .collect();
                let note = self.append_run_ledger(run_id, records).await;
                output["run_ledger"] = note;
            }

            Ok(output)
        })
        .await
    }

    #[tool(description = "Extract content from a URL into markdown or structured JSON")]
    pub async fn web_extract(
        &self,
        Parameters(ExtractRequest {
            url,
            format,
            json_prompt,
            json_schema,
            main_content_only,
            wait_for_ms,
            run_id,
        }): Parameters<ExtractRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "web_extract", async {
            self.rate_limiter.check("web_extract")?;

            if url.len() > MAX_URL_LENGTH {
                return Err(McpToolError::invalid_argument(format!(
                    "url exceeds maximum length of {} characters",
                    MAX_URL_LENGTH
                )));
            }
            if let Some(ref prompt) = json_prompt
                && prompt.len() > MAX_JSON_PROMPT_LENGTH
            {
                return Err(McpToolError::invalid_argument(format!(
                    "json_prompt exceeds maximum length of {} characters",
                    MAX_JSON_PROMPT_LENGTH
                )));
            }
            if let Some(ref schema) = json_schema
                && let Ok(bytes) = serde_json::to_string(schema)
                && bytes.len() > MAX_JSON_SCHEMA_BYTES
            {
                return Err(McpToolError::invalid_argument(format!(
                    "json_schema exceeds maximum size of {} bytes",
                    MAX_JSON_SCHEMA_BYTES
                )));
            }

            validate_tool_url_with_dns(&url).await?;

            // Format is an enum, not a free string: Firecrawl silently maps
            // unknown formats to markdown while echoing the REQUESTED format
            // in its output — the caller would read markdown labelled as
            // something else. Reject unknown formats up front.
            let fmt = match format.as_deref() {
                Some("markdown") | Some("json") | None => {
                    format.unwrap_or_else(|| "markdown".to_string())
                }
                Some(other) => {
                    return Err(McpToolError::invalid_argument(format!(
                        "format must be 'markdown' or 'json', got '{other}'"
                    )));
                }
            };
            let main_content_only = main_content_only.unwrap_or(true);
            let wait_for_ms_val = wait_for_ms.unwrap_or(0);
            // Compute the cache key before moving json_schema into opts.
            let json_schema_str = json_schema
                .as_ref()
                .and_then(|v| serde_json::to_string(v).ok());
            let json_schema_inner = json_schema.map(serde_json::Value::from);

            let fingerprint = self.pool.provider_fingerprint();
            let cache_params = serde_json::json!({
                "format": fmt,
                "main_content_only": main_content_only,
                "json_prompt": json_prompt,
                "json_schema": json_schema_str,
                "wait_for_ms": wait_for_ms_val,
            });
            let ckey = cache_key("extract", &url, &cache_params, &fingerprint);

            let opts = ExtractOptions {
                format: fmt,
                json_prompt,
                json_schema: json_schema_inner,
                main_content_only,
                wait_for_ms: wait_for_ms_val,
            };

            // A run-scoped extract bypasses the response-cache read so the
            // run ledger records what THIS request returned.
            if run_id.is_none()
                && let Some(cached) = self.cache.get(&ckey).await
            {
                return Ok(cached);
            }

            let extracted = self
                .pool
                .extract(&url, &opts)
                .await
                .map_err(McpToolError::from)?;
            let extract_output = ExtractOutput {
                url: extracted.url,
                format: extracted.format,
                content: extracted.content,
                metadata: extracted.metadata,
            };
            let mut output = serde_json::to_value(&extract_output)
                .unwrap_or_else(|_| serde_json::json!({ "error": "serialization failed" }));
            self.cache.insert(ckey, output.clone()).await;

            // Server-side ledger append (see web_search.run_id): the audit
            // copy is the extracted content, capped by the ledger.
            if let Some(run_id) = run_id.as_deref() {
                let record = RunSourceRecord {
                    url: extract_output.url.clone(),
                    provider: None,
                    title: None,
                    published: None,
                    source: None,
                    excerpt: Some(extract_output.content.clone()),
                };
                let note = self.append_run_ledger(run_id, vec![record]).await;
                output["run_ledger"] = note;
            }

            Ok(output)
        })
        .await
    }

    #[tool(description = "Interactive browsing of JS-heavy pages via headless browser")]
    pub async fn web_browse(
        &self,
        Parameters(BrowseRequest {
            url,
            instruction,
            timeout_secs,
        }): Parameters<BrowseRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "web_browse", async {
            self.rate_limiter.check("web_browse")?;

            // Not cached: web_browse skips the response cache (unlike
            // web_search and web_extract). Browsed content is interactive and
            // session-specific; a cached snapshot would mislead on re-browse.
            tracing::debug!(target: "hkask.web", "web_browse cache miss (not cached)");

            if url.len() > MAX_URL_LENGTH {
                return Err(McpToolError::invalid_argument(format!(
                    "url exceeds maximum length of {} characters",
                    MAX_URL_LENGTH
                )));
            }
            if let Some(ref instr) = instruction
                && instr.len() > MAX_INSTRUCTION_LENGTH
            {
                return Err(McpToolError::invalid_argument(format!(
                    "instruction exceeds maximum length of {} characters",
                    MAX_INSTRUCTION_LENGTH
                )));
            }

            validate_tool_url_with_dns(&url).await?;

            let instr = instruction.unwrap_or_else(|| "Extract page content".to_string());
            let timeout =
                Duration::from_secs(timeout_secs.unwrap_or(30)).min(Duration::from_secs(120));

            self.pool
                .browse(&url, &instr, timeout)
                .await
                .map(|result| {
                    let output = BrowseOutput {
                        url: result.url,
                        content: result.content,
                        instruction: result.instruction,
                        actions_taken: result.actions_taken,
                    };
                    serde_json::to_value(&output)
                        .unwrap_or_else(|_| serde_json::json!({ "error": "serialization failed" }))
                })
                .map_err(McpToolError::from)
        })
        .await
    }

    // ═══════════════════ RSS tools ═══════════════════

    #[tool(description = "Subscribe to an RSS/Atom feed (Google Reader stream model)")]
    pub async fn rss_subscribe(
        &self,
        Parameters(SubscribeRequest { url, label, folder }): Parameters<SubscribeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_subscribe", async {
            self.rate_limiter.check("rss_subscribe")?;
            let db = require_research_db!(self);

            // Use permissive SSRF config: RSS feeds may be self-hosted on
            // local networks (e.g., http://localhost:4000/feed.xml). The user
            // is explicitly subscribing to this URL by choice.
            hkask_mcp_server::server::validate_tool_url_permissive(&url)?;
            let fetch_result = fetch_feed(&self.rss_client, &url, None, None).await
                .map_err(|e| McpToolError::unavailable(format!("Fetch failed: {}", e)))?;
            let stream_id = format!("feed/{url}");
            let (url_c, label_c, folder_c) = (url, label, folder);
            let etag = fetch_result.etag.clone();
            let lm = fetch_result.last_modified.clone();
            let feed_title = fetch_result
                .feed
                .title
                .as_ref()
                .map(|t| t.content.clone())
                .unwrap_or_default();
            let entry_count = fetch_result.feed.entries.len();
            let result = spawn_db(db, move |conn| {
                // N3 (panic-safe): use rusqlite's Transaction guard so a panic
                // between BEGIN and COMMIT automatically rolls back.
                let tx = rusqlite::Transaction::new_unchecked(
                    conn,
                    rusqlite::TransactionBehavior::Deferred,
                )?;
                let feed_id = upsert_feed(&tx, &url_c, &fetch_result.feed)?;
                insert_entries(&tx, feed_id, &fetch_result.feed.entries)?;
                update_feed_cache_headers(&tx, feed_id, etag.as_deref(), lm.as_deref())?;
                let exists: bool = tx.query_row("SELECT COUNT(*) FROM subscriptions WHERE stream_id = ?1", [&stream_id], |row| row.get::<_, i64>(0)).map(|c| c > 0)?;
                let result = if exists {
                    serde_json::json!({"stream_id": stream_id, "url": url_c, "subscribed": true, "note": "Already subscribed, feed refreshed"})
                } else {
                    tx.execute("INSERT INTO subscriptions (feed_id, stream_id, title, label, folder) VALUES (?1, ?2, ?3, ?4, ?5)", rusqlite::params![feed_id, stream_id, feed_title, label_c, folder_c])?;
                    serde_json::json!({"stream_id": stream_id, "url": url_c, "label": label_c, "folder": folder_c, "subscribed": true, "entry_count": entry_count})
                };
                tx.commit()?;
                Ok::<serde_json::Value, anyhow::Error>(result)
            }).await;
            handle_db_result!(result, |v| v)
        }).await
    }

    #[tool(description = "Unsubscribe from a feed (stream_id e.g. 'feed/http://...')")]
    pub async fn rss_unsubscribe(
        &self,
        Parameters(UnsubscribeRequest { stream_id }): Parameters<UnsubscribeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_unsubscribe", async {
            let db = require_research_db!(self);

            let sid = stream_id.clone();
            let result = spawn_db(db, move |conn| {
                conn.execute("DELETE FROM subscriptions WHERE stream_id = ?1", [&sid])
                    .map_err(|e| anyhow::anyhow!(e))
            })
            .await;
            handle_db_result!(
                result,
                |removed| serde_json::json!({"stream_id": stream_id, "unsubscribed": removed > 0, "removed": removed})
            )
        }).await
    }

    #[tool(description = "List subscriptions, optionally filtered by folder")]
    pub async fn rss_list_subscriptions(
        &self,
        Parameters(ListSubscriptionsRequest { folder }): Parameters<ListSubscriptionsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_list_subscriptions", async {
            let db = require_research_db!(self);
            let result = spawn_db(db, move |conn| list_subscriptions(conn, folder.as_deref())).await;
            handle_db_result!(
                result,
                |subs: Vec<serde_json::Value>| serde_json::json!({"count": subs.len(), "subscriptions": subs})
            )
        }).await
    }

    #[tool(description = "Fetch/sync new entries from a feed (supports ETag/Last-Modified)")]
    pub async fn rss_fetch(
        &self,
        Parameters(FetchRequest { stream_id }): Parameters<FetchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_fetch", async {
            self.rate_limiter.check("rss_fetch")?;
            let db = require_research_db!(self);
            let sid = stream_id.clone();
            let lookup = spawn_db(db, move |conn| resolve_feed_with_headers(conn, &sid)).await;

            let (feed_url, cached_etag, cached_lm) = match lookup {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => {
                    return Err(McpToolError::not_found(e.to_string()));
                }
                Err(e) => {
                    return Err(map_join_error(e, "rss fetch task failed"));
                }
            };

            // Dispatch: synthetic feeds (url starts with synthetic://) are
            // re-extracted via the synthetic fetch path rather than fetched as RSS.
            if feed_url.starts_with("synthetic://") {
                return self
                    .fetch_synthetic_inner(&stream_id, feed_url)
                    .await;
            }

            // Stored-SSRF defense: validate the DB-stored feed URL before
            // fetching. The URL was originally user-supplied via rss_subscribe
            // or rss_import_opml; re-validate at fetch time to catch URLs that
            // were inserted before validation was added, or that a compromised
            // DB could have altered. Use permissive config (allows localhost/
            // private IPs) because RSS feeds may be self-hosted on local
            // networks — the user explicitly subscribed to them.
            hkask_mcp_server::server::validate_tool_url_permissive(&feed_url)?;

            let db = require_research_db!(self);
            let fetch_result = fetch_feed(
                &self.rss_client,
                &feed_url,
                cached_etag.as_deref(),
                cached_lm.as_deref(),
            )
            .await
            .map_err(|e| McpToolError::unavailable(format!("Fetch failed: {}", e)))?;

            if fetch_result.status == 304 {
                return Ok(serde_json::json!({
                    "stream_id": stream_id,
                    "new_entries": 0,
                    "fetched": true,
                    "not_modified": true,
                }));
            }

            let sid2 = stream_id.clone();
            let etag = fetch_result.etag.clone();
            let lm = fetch_result.last_modified.clone();

            let result = spawn_db(db, move |conn| {
                // N3 (panic-safe): use rusqlite's Transaction guard so a panic
                // between BEGIN and COMMIT automatically rolls back.
                let tx = rusqlite::Transaction::new_unchecked(
                    conn,
                    rusqlite::TransactionBehavior::Deferred,
                )?;
                let feed_id = upsert_feed(&tx, &feed_url, &fetch_result.feed)?;
                let new_count = insert_entries(&tx, feed_id, &fetch_result.feed.entries)?;
                update_feed_cache_headers(&tx, feed_id, etag.as_deref(), lm.as_deref())?;
                tx.commit()?;
                Ok::<usize, anyhow::Error>(new_count)
            })
            .await;

            handle_db_result!(
                result,
                |new_count| serde_json::json!({"stream_id": sid2, "new_entries": new_count, "fetched": true})
            )
        }).await
    }

    #[tool(
        description = "Get entries from a stream (Google Reader stream IDs: feed/*, user/-/state/*, user/-/label/*)"
    )]
    pub async fn rss_get_entries(
        &self,
        Parameters(GetEntriesRequest {
            stream_id,
            unread_only,
            starred_only,
            count,
            continuation_token,
        }): Parameters<GetEntriesRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_get_entries", async {
            let db = require_research_db!(self);
            let limit = (count.unwrap_or(DEFAULT_PAGE_SIZE as u32) as usize).min(MAX_PAGE_SIZE);
            let offset = match continuation_token.as_ref() {
                None => 0,
                Some(t) => {
                    // A malformed continuation token must surface as an
                    // explicit error, not silently reset to offset 0 —
                    // otherwise a corrupted token is indistinguishable from
                    // "no token" and the client silently re-reads the first
                    // page (`.rules` broken-feedback-loop trap).
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(t)
                        .map_err(|e| {
                            McpToolError::invalid_argument(format!(
                                "continuation_token is not valid base64: {e}"
                            ))
                        })?;
                    let cont: Continuation = serde_json::from_slice(&bytes)
                        .map_err(|e| {
                            McpToolError::invalid_argument(format!(
                                "continuation_token is not a valid continuation payload: {e}"
                            ))
                        })?;
                    cont.offset
                }
            };

            let sid = stream_id.clone();
            let result = spawn_db(db, move |conn| {
                query_entries(
                    conn,
                    &sid,
                    unread_only.unwrap_or(false),
                    starred_only.unwrap_or(false),
                    offset,
                    limit + 1,
                )
            })
            .await;

            handle_db_result!(result, |mut entries: Vec<serde_json::Value>| {
                let has_more = entries.len() > limit;
                if has_more {
                    entries.truncate(limit);
                }
                let next_token = has_more.then(|| {
                    let cont = Continuation {
                        offset: offset + limit,
                        stream_id: stream_id.clone(),
                    };
                    base64::engine::general_purpose::STANDARD
                        .encode(serde_json::to_vec(&cont).unwrap_or_default())
                });
                serde_json::json!({"stream_id": stream_id, "entries": entries, "count": entries.len(), "continuation_token": next_token})
            })
        }).await
    }

    #[tool(description = "Mark all entries in a stream as read")]
    pub async fn rss_mark_all_read(
        &self,
        Parameters(MarkReadRequest { stream_id }): Parameters<MarkReadRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_mark_all_read", async {
            let db = require_research_db!(self);
            let sid = stream_id.clone();
            let result = spawn_db(db, move |conn| mark_stream_read(conn, &sid)).await;
            handle_db_result!(
                result,
                |marked| serde_json::json!({"stream_id": stream_id, "marked_read": marked})
            )
        })
        .await
    }

    #[tool(description = "Get unread count for a stream")]
    pub async fn rss_get_unread_count(
        &self,
        Parameters(UnreadCountRequest { stream_id }): Parameters<UnreadCountRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_get_unread_count", async {
            let db = require_research_db!(self);
            let sid = stream_id.clone();
            let result = spawn_db(db, move |conn| count_entries(conn, &sid, true)).await;
            handle_db_result!(
                result,
                |count| serde_json::json!({"stream_id": stream_id, "unread_count": count})
            )
        })
        .await
    }

    #[tool(description = "Full-text search across feed entries")]
    pub async fn rss_search(
        &self,
        Parameters(crate::research::rss_types::SearchRequest { query, limit }): Parameters<
            crate::research::rss_types::SearchRequest,
        >,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_search", async {
            let db = require_research_db!(self);
            let limit = (limit.unwrap_or(10) as usize).min(MAX_PAGE_SIZE);
            let q = query.clone();
            let result = spawn_db(db, move |conn| search_entries(conn, &q, limit)).await;
            handle_db_result!(
                result,
                |results: Vec<serde_json::Value>| serde_json::json!({"query": query, "results": results, "count": results.len()})
            )
        }).await
    }

    #[tool(description = "Export subscriptions as OPML 2.0")]
    pub async fn rss_export_opml(&self) -> Result<String, McpToolError> {
        execute_tool(self, "rss_export_opml", async {
            let db = require_research_db!(self);
            let result = spawn_db(db, export_opml).await;
            handle_db_result!(result, |opml| serde_json::json!({"opml": opml}))
        })
        .await
    }

    #[tool(description = "Import subscriptions from OPML content")]
    pub async fn rss_import_opml(
        &self,
        Parameters(ImportOpmlRequest { opml_content }): Parameters<ImportOpmlRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_import_opml", async {
            let db = require_research_db!(self);
            let result = spawn_db(db, move |conn| import_opml(conn, &opml_content)).await;
            handle_db_result!(result, |v| v)
        })
        .await
    }

    #[tool(description = "Discover RSS/Atom feeds from a URL via HTML link autodiscovery")]
    pub async fn rss_discover_feeds(
        &self,
        Parameters(DiscoverRequest { url }): Parameters<DiscoverRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_discover_feeds", async {
            self.rate_limiter.check("rss_discover_feeds")?;
            validate_tool_url_with_dns(&url).await?;
            match discover_feeds(&self.discover_client, &url).await {
                Ok(feeds) => {
                    Ok(serde_json::json!({"url": url, "feeds": feeds, "count": feeds.len()}))
                }
                Err(e) => Err(McpToolError::unavailable(e.to_string())),
            }
        })
        .await
    }

    #[tool(description = "Edit tags on entries: mark read/unread, star/unstar, add/remove labels")]
    pub async fn rss_edit_tag(
        &self,
        Parameters(req): Parameters<EditTagRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_edit_tag", async {
            let db = require_research_db!(self);
            let result = spawn_db(db, move |conn| edit_tags(conn, &req)).await;
            handle_db_result!(result, |v| v)
        })
        .await
    }

    // ═══════════════════ Synthetic feed tools ═══════════════════

    #[tool(
        description = "Create a synthetic feed from a non-feed website or JSON API. Extracts items using the specified extractor kind (css, json_path, diff_hash) and stores them as feed entries. Optionally subscribes to the created feed."
    )]
    pub async fn rss_synthesize(
        &self,
        Parameters(req): Parameters<SynthesizeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_synthesize", async {
            self.rate_limiter.check("rss_synthesize")?;
            let db = require_research_db!(self);

            // Parse the extractor kind.
            let kind: crate::research::synthetic::ExtractorKind =
                req.extractor_kind.parse().map_err(McpToolError::from)?;

            // Parse the extractor spec JSON.
            let spec: crate::research::synthetic::ExtractorSpec =
                serde_json::from_str(&req.extractor_spec).map_err(|e| {
                    McpToolError::invalid_argument(format!("invalid extractor_spec JSON: {e}"))
                })?;

            // Validate the source URL (SSRF defense).
            hkask_mcp_server::server::validate_tool_url_permissive(&req.source_url)?;

            // Fetch the source.
            let response = self
                .rss_client
                .get(&req.source_url)
                .send()
                .await
                .map_err(|e| McpToolError::unavailable(format!("fetch source: {e}")))?;
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let status = response.status();
            if !status.is_success() {
                return Err(McpToolError::unavailable(format!(
                    "source returned HTTP {}",
                    status
                )));
            }
            let body = response.bytes().await.map_err(|e| {
                McpToolError::unavailable(format!("read source body: {e}"))
            })?;
            if body.len() > MAX_RESPONSE_BYTES {
                return Err(McpToolError::unavailable(format!(
                    "source body is {} bytes ({:.1} MiB) — exceeds the {} byte limit",
                    body.len(),
                    body.len() as f64 / (1024.0 * 1024.0),
                    MAX_RESPONSE_BYTES
                )));
            };

            let title = req.title.clone().unwrap_or_else(|| req.source_url.clone());
            let description = req.description.clone().unwrap_or_default();
            let source_url = req.source_url.clone();
            let extractor_kind_str = req.extractor_kind.clone();
            let extractor_spec_str = req.extractor_spec.clone();
            let cadence = req.cadence_hint_secs;
            let label = req.label.clone();
            let folder = req.folder.clone();
            let want_subscribe = req.subscribe.unwrap_or(true);

            // Extract items. For css/json_path/diff_hash, use the sync extract()
            // function. For llm_schema and pdf_ocr, use the async pool-based
            // extractors.
            let (feed, _entry_count, extract_hash) = match kind {
                crate::research::synthetic::ExtractorKind::DiffHash => {
                    let (feed, hash) =
                        crate::research::synthetic::build_diff_hash_feed(&body, &source_url, &title);
                    let count = feed.entries.len();
                    (feed, count, Some(hash))
                }
                crate::research::synthetic::ExtractorKind::LlmSchema => {
                    let items = crate::research::synthetic::extract_llm_schema(
                        self.pool.as_ref(),
                        &spec,
                        &source_url,
                    )
                    .await
                    .map_err(McpToolError::from)?;
                    let entries = crate::research::synthetic::items_to_entries(items, &title);
                    let count = entries.len();
                    let mut feed = crate::research::synthetic::build_synthetic_feed(
                        &source_url,
                        &title,
                        &description,
                    );
                    feed.entries = entries;
                    (feed, count, None)
                }
                crate::research::synthetic::ExtractorKind::PdfOcr => {
                    let items = crate::research::synthetic::extract_pdf_ocr(
                        self.pool.as_ref(),
                        &spec,
                        &source_url,
                        &body,
                    )
                    .await
                    .map_err(McpToolError::from)?;
                    if items.is_empty() {
                        // diff_hash post-processing or empty PDF.
                        let (feed, hash) = crate::research::synthetic::build_diff_hash_feed(
                            &body,
                            &source_url,
                            &title,
                        );
                        let count = feed.entries.len();
                        (feed, count, Some(hash))
                    } else {
                        let entries = crate::research::synthetic::items_to_entries(items, &title);
                        let count = entries.len();
                        let mut feed = crate::research::synthetic::build_synthetic_feed(
                            &source_url,
                            &title,
                            &description,
                        );
                        feed.entries = entries;
                        (feed, count, None)
                    }
                }
                _ => {
                    // css or json_path — sync extraction.
                    let items = crate::research::synthetic::extract(
                        kind,
                        &spec,
                        &source_url,
                        &body,
                        &content_type,
                    )
                    .map_err(McpToolError::from)?;
                    let entries = crate::research::synthetic::items_to_entries(items, &title);
                    let count = entries.len();
                    let mut feed = crate::research::synthetic::build_synthetic_feed(
                        &source_url,
                        &title,
                        &description,
                    );
                    feed.entries = entries;
                    (feed, count, None)
                }
            };

            // Insert into DB: create feeds row, synthetic_feeds row, entries.
            let feed_title = feed
                .title
                .as_ref()
                .map(|t| t.content.clone())
                .unwrap_or_else(|| source_url.clone());
            let feed_for_upsert = feed;
            let result = spawn_db(db, move |conn| {
                let tx = rusqlite::Transaction::new_unchecked(
                    conn,
                    rusqlite::TransactionBehavior::Deferred,
                )?;

                // Insert the feed with a placeholder synthetic:// URL.
                // We insert with a temporary URL, get the feed_id, then update
                // the URL to synthetic://<feed_id>.
                let temp_url = "synthetic://pending".to_string();
                let feed_id = upsert_feed(&tx, &temp_url, &feed_for_upsert)?;

                // Update the URL to the canonical synthetic:// form.
                tx.execute(
                    "UPDATE feeds SET url = ?1 WHERE id = ?2",
                    rusqlite::params![format!("synthetic://{feed_id}"), feed_id],
                )?;

                // Insert entries.
                let new_entries = insert_entries(&tx, feed_id, &feed_for_upsert.entries)?;

                // Insert synthetic_feeds binding.
                insert_synthetic_feed(
                    &tx,
                    feed_id,
                    &source_url,
                    &extractor_kind_str,
                    &extractor_spec_str,
                    cadence,
                )?;

                // Update extraction status.
                update_synthetic_status(
                    &tx,
                    feed_id,
                    new_entries,
                    extract_hash.as_deref(),
                    None,
                )?;

                // Optionally subscribe.
                let stream_id = format!("feed/synthetic://{feed_id}");
                let sub_note = if want_subscribe {
                    tx.execute(
                        "INSERT OR IGNORE INTO subscriptions (feed_id, stream_id, title, label, folder)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![feed_id, &stream_id, &feed_title, label, folder],
                    )?;
                    "subscribed"
                } else {
                    "not_subscribed"
                };

                tx.commit()?;

                Ok::<serde_json::Value, anyhow::Error>(serde_json::json!({
                    "feed_id": feed_id,
                    "stream_id": stream_id,
                    "source_url": source_url,
                    "extractor_kind": extractor_kind_str,
                    "new_entries": new_entries,
                    "subscribed": want_subscribe,
                    "subscription_status": sub_note,
                }))
            })
            .await;
            handle_db_result!(result, |v| v)
        })
        .await
    }

    /// Shared synthetic feed fetch logic. Called by both `rss_fetch` (when it
    /// detects a `synthetic://` URL).
    /// The `feed_url` must already be resolved and start with `synthetic://`.
    async fn fetch_synthetic_inner(
        &self,
        stream_id: &str,
        feed_url: String,
    ) -> Result<serde_json::Value, McpToolError> {
        let db = require_research_db!(self);

        // Look up the synthetic feed binding.
        let feed_url_for_lookup = feed_url.clone();
        let synth_row = spawn_db(db.clone(), move |conn| {
            lookup_synthetic_by_feed_url(conn, &feed_url_for_lookup)
        })
        .await;
        let synth = match synth_row {
            Ok(Ok(Some(row))) => row,
            Ok(Ok(None)) => {
                return Err(McpToolError::not_found("synthetic feed binding not found"));
            }
            Ok(Err(e)) => return Err(map_db_error(e)),
            Err(e) => return Err(map_join_error(e, "db lookup failed")),
        };

        // Parse the extractor kind and spec.
        let kind: crate::research::synthetic::ExtractorKind =
            synth.extractor_kind.parse().map_err(McpToolError::from)?;
        let spec: crate::research::synthetic::ExtractorSpec =
            serde_json::from_str(&synth.extractor_spec).map_err(|e| {
                McpToolError::internal(format!("invalid stored extractor_spec: {e}")) // rr0044-ok: parse-own-stored-data
            })?;

        // Validate and fetch the source URL.
        hkask_mcp_server::server::validate_tool_url_permissive(&synth.source_url)?;
        let response = self
            .rss_client
            .get(&synth.source_url)
            .send()
            .await
            .map_err(|e| McpToolError::unavailable(format!("fetch source: {e}")))?;
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let status = response.status();
        if !status.is_success() {
            // Record the error.
            let err_msg = format!("source returned HTTP {status}");
            let feed_id = synth.feed_id;
            let err_for_db = err_msg.clone();
            let _ = spawn_db(db.clone(), move |conn| {
                update_synthetic_status(conn, feed_id, 0, None, Some(&err_for_db))
            })
            .await;
            return Err(McpToolError::unavailable(err_msg));
        }
        let body = response
            .bytes()
            .await
            .map_err(|e| McpToolError::unavailable(format!("read source body: {e}")))?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(McpToolError::unavailable(format!(
                "source body is {} bytes ({:.1} MiB) — exceeds the {} byte limit",
                body.len(),
                body.len() as f64 / (1024.0 * 1024.0),
                MAX_RESPONSE_BYTES
            )));
        };

        // For diff_hash: check if content changed.
        if kind == crate::research::synthetic::ExtractorKind::DiffHash {
            let new_hash = crate::research::synthetic::content_hash(&body);
            if Some(new_hash.as_str()) == synth.last_extract_hash.as_deref() {
                return Ok(serde_json::json!({
                    "stream_id": stream_id,
                    "new_entries": 0,
                    "fetched": true,
                    "not_modified": true,
                }));
            }
            // Content changed — create a new entry.
            let (feed, hash) = crate::research::synthetic::build_diff_hash_feed(
                &body,
                &synth.source_url,
                &synth.source_url,
            );
            let feed_id = synth.feed_id;
            let hash_for_db = hash.clone();
            let result = spawn_db(db, move |conn| {
                let tx = rusqlite::Transaction::new_unchecked(
                    conn,
                    rusqlite::TransactionBehavior::Deferred,
                )?;
                let new_count = insert_entries(&tx, feed_id, &feed.entries)?;
                update_synthetic_status(&tx, feed_id, new_count, Some(&hash_for_db), None)?;
                tx.commit()?;
                Ok::<usize, anyhow::Error>(new_count)
            })
            .await;
            match result {
                Ok(Ok(new_count)) => Ok(serde_json::json!({
                    "stream_id": stream_id,
                    "new_entries": new_count,
                    "fetched": true,
                })),
                Ok(Err(e)) => Err(map_db_error(e)),
                Err(e) => Err(map_join_error(e, "db task failed")),
            }
        } else {
            // css, json_path, llm_schema, or pdf_ocr extraction.
            let items = match kind {
                crate::research::synthetic::ExtractorKind::LlmSchema => {
                    crate::research::synthetic::extract_llm_schema(
                        self.pool.as_ref(),
                        &spec,
                        &synth.source_url,
                    )
                    .await
                    .map_err(McpToolError::from)?
                }
                crate::research::synthetic::ExtractorKind::PdfOcr => {
                    crate::research::synthetic::extract_pdf_ocr(
                        self.pool.as_ref(),
                        &spec,
                        &synth.source_url,
                        &body,
                    )
                    .await
                    .map_err(McpToolError::from)?
                }
                _ => {
                    // css or json_path — sync extraction.
                    crate::research::synthetic::extract(
                        kind,
                        &spec,
                        &synth.source_url,
                        &body,
                        &content_type,
                    )
                    .map_err(McpToolError::from)?
                }
            };

            let entries = crate::research::synthetic::items_to_entries(items, &synth.source_url);
            let feed_id = synth.feed_id;
            let result = spawn_db(db, move |conn| {
                let tx = rusqlite::Transaction::new_unchecked(
                    conn,
                    rusqlite::TransactionBehavior::Deferred,
                )?;
                let new_count = insert_entries(&tx, feed_id, &entries)?;
                update_synthetic_status(&tx, feed_id, new_count, None, None)?;
                tx.commit()?;
                Ok::<usize, anyhow::Error>(new_count)
            })
            .await;
            match result {
                Ok(Ok(new_count)) => Ok(serde_json::json!({
                    "stream_id": stream_id,
                    "new_entries": new_count,
                    "fetched": true,
                })),
                Ok(Err(e)) => Err(map_db_error(e)),
                Err(e) => Err(map_join_error(e, "db task failed")),
            }
        }
    }

    #[tool(description = "List all synthetic feeds with their specs and last-extraction stats")]
    pub async fn rss_list_synthetic(&self) -> Result<String, McpToolError> {
        execute_tool(self, "rss_list_synthetic", async {
            let db = require_research_db!(self);
            let result = spawn_db(db, move |conn| list_synthetic_feeds(conn)).await;
            handle_db_result!(result, |feeds: Vec<serde_json::Value>| serde_json::json!({
                "count": feeds.len(),
                "synthetic_feeds": feeds
            }))
        })
        .await
    }

    #[tool(
        description = "Delete a synthetic feed and all its entries (stream_id e.g. 'feed/synthetic://123')"
    )]
    pub async fn rss_delete_synthetic(
        &self,
        Parameters(req): Parameters<DeleteSyntheticRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "rss_delete_synthetic",
            async {
                let db = require_research_db!(self);

                // Resolve feed_url from stream_id.
                let sid = req.stream_id.clone();
                let feed_url_result = spawn_db(db.clone(), move |conn| {
                    // resolve_feed_url returns Option<String>, wrap in Ok for the
                    // spawn_db Result<Result<_, anyhow>, JoinError> shape.
                    Ok::<Option<String>, anyhow::Error>(resolve_feed_url(conn, &sid))
                })
                .await;

                let feed_url = match feed_url_result {
                    Ok(Ok(Some(url))) => url,
                    Ok(Ok(None)) => {
                        return Err(McpToolError::not_found("stream_id not found"));
                    }
                    Ok(Err(e)) => return Err(map_db_error(e)),
                    Err(e) => return Err(map_join_error(e, "db lookup failed")),
                };

                if !feed_url.starts_with("synthetic://") {
                    return Err(McpToolError::invalid_argument(
                        "not a synthetic feed; use rss_unsubscribe instead",
                    ));
                }

                let feed_id: i64 = feed_url
                    .strip_prefix("synthetic://")
                    .ok_or_else(|| /* rr0044-ok: unreachable-invariant */ McpToolError::internal("feed_url missing synthetic:// prefix despite starts_with check"))?
                    .parse()
                    .map_err(|e| McpToolError::invalid_argument(format!("invalid feed_id: {e}")))?;

                let result = spawn_db(db, move |conn| delete_synthetic_feed(conn, feed_id)).await;
                handle_db_result!(result, |removed| serde_json::json!({
                    "stream_id": req.stream_id,
                    "deleted": removed > 0,
                    "removed": removed
                }))
            },
        )
        .await
    }

    // ═══════════════════ Research-run ledger ═══════════════════

    #[tool(
        description = "Begin a research run: opens a server-side ledger that records what the research tools actually return under this run. Returns {run_id, status}. Pass the run_id to web_search / web_extract / web_find_similar to record their results non-repudiably (recorded_by='server'); read the manifest via get_research_run."
    )]
    pub async fn begin_research_run(
        &self,
        Parameters(BeginResearchRunRequest { question }): Parameters<BeginResearchRunRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "begin_research_run", async {
            if question.trim().is_empty() {
                return Err(McpToolError::invalid_argument("question must not be empty"));
            }
            let database = require_research_db!(self);
            let question_for_task = question.clone();
            let result = spawn_db(database, move |connection| {
                crate::research::runs::begin_research_run(connection, &question_for_task)
            })
            .await;
            handle_db_result!(result, |run: NewResearchRun| serde_json::json!({
                "run_id": run.run_id,
                "status": "planned",
            }))
        })
        .await
    }

    #[tool(
        description = "Get a research run's manifest: question, status, every recorded source (server-observed and agent-declared), and per-source confidence recomputed server-side from the ledger's own excerpt copies — deterministic, not the agent's claim."
    )]
    pub async fn get_research_run(
        &self,
        Parameters(GetResearchRunRequest { run_id }): Parameters<GetResearchRunRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "get_research_run", async {
            if run_id.trim().is_empty() {
                return Err(McpToolError::invalid_argument("run_id must not be empty"));
            }
            let database = require_research_db!(self);
            let run_id_for_task = run_id.clone();
            let result = spawn_db(database, move |connection| {
                crate::research::runs::get_research_run(connection, &run_id_for_task)
            })
            .await;
            match result {
                Ok(Ok(Some(manifest))) => Ok(manifest),
                Ok(Ok(None)) => Err(McpToolError::not_found(format!(
                    "research run '{run_id}' not found"
                ))),
                Ok(Err(error)) => Err(map_db_error(error)),
                Err(error) => Err(map_join_error(error, "db task failed")),
            }
        })
        .await
    }

    #[tool(
        description = "Annotate a research run's source with an agent-declared verification state (not_checked, inferred, partial, verified, blocked, failed). `verified` is accepted ONLY for sources the server itself recorded under this run (pass run_id to web_search/web_extract/web_find_similar) and requires a basis — the server refuses verification claims about sources it never served. Annotations are upsert-idempotent; annotating an unseen URL records it as an agent-declared row."
    )]
    pub async fn annotate_research_run(
        &self,
        Parameters(AnnotateResearchRunRequest {
            run_id,
            url,
            verification_state,
            basis,
        }): Parameters<AnnotateResearchRunRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "annotate_research_run", async {
            if run_id.trim().is_empty() {
                return Err(McpToolError::invalid_argument("run_id must not be empty"));
            }
            if url.trim().is_empty() {
                return Err(McpToolError::invalid_argument("url must not be empty"));
            }
            let database = require_research_db!(self);
            let run_id_for_task = run_id.clone();
            let url_for_task = url.clone();
            let state_for_task = verification_state.clone();
            let basis_for_task = basis.clone();
            let result = spawn_db(database, move |connection| {
                Ok(crate::research::runs::annotate_run_source(
                    connection,
                    &run_id_for_task,
                    &url_for_task,
                    &state_for_task,
                    basis_for_task.as_deref(),
                ))
            })
            .await;
            match result {
                Ok(Ok(Ok(()))) => Ok(serde_json::json!({
                    "run_id": run_id,
                    "url": url,
                    "verification_state": verification_state,
                    "recorded_by": "agent",
                })),
                Ok(Ok(Err(crate::research::runs::AnnotateError::RunNotFound))) => Err(
                    McpToolError::not_found(format!("research run '{run_id}' not found")),
                ),
                Ok(Ok(Err(crate::research::runs::AnnotateError::Invalid(rule)))) => {
                    Err(McpToolError::invalid_argument(rule))
                }
                Ok(Ok(Err(crate::research::runs::AnnotateError::Db(error)))) => {
                    Err(map_db_error(error))
                }
                Ok(Err(error)) => Err(map_db_error(error)),
                Err(error) => Err(map_join_error(error, "db task failed")),
            }
        })
        .await
    }

    /// Append server-observed records to a run's ledger (the
    /// non-repudiation path). Best-effort by design: the outcome is
    /// returned as a `run_ledger` note for the tool's output —
    /// `{"recorded": n}` on success, `{"recorded": 0, "error": ...}` on
    /// failure or when the DB is not configured. Never silent in either
    /// direction (the T01 reliability pattern: the public response
    /// surfaces persistence outcomes).
    async fn append_run_ledger(
        &self,
        run_id: &str,
        records: Vec<RunSourceRecord>,
    ) -> serde_json::Value {
        let Some(database) = self.research_db.clone() else {
            return serde_json::json!({
                "recorded": 0,
                "error": "research database not configured — set HKASK_RESEARCH_DB and \
                          HKASK_DB_PASSPHRASE to record runs",
            });
        };
        let run_id_for_task = run_id.to_string();
        let append = spawn_db(database, move |connection| {
            crate::research::runs::append_run_sources(connection, &run_id_for_task, &records)
        })
        .await;
        match append {
            Ok(Ok(inserted)) => serde_json::json!({ "recorded": inserted }),
            Ok(Err(error)) => {
                tracing::warn!(
                    target: "hkask.research.runs",
                    run_id,
                    error = %error,
                    "run ledger append failed"
                );
                serde_json::json!({ "recorded": 0, "error": error.to_string() })
            }
            Err(error) => {
                tracing::warn!(
                    target: "hkask.research.runs",
                    run_id,
                    error = %error,
                    "run ledger append task failed"
                );
                serde_json::json!({ "recorded": 0, "error": error.to_string() })
            }
        }
    }

    // ═══════════════════ Evidence evaluation ═══════════════════

    #[tool(
        description = "Evaluate retrieved evidence against a research question. Scores each artifact deterministically into per-component signals (base, corroboration as independent evidence units with syndication-aware content clustering, recency, content) with basis strings, plus a set-level report (content clusters, ordering sensitivity under weight-profile substitution, duplication mode). Emits SEPIO-anchored confidence and corroboration links. Use after web_search/web_extract to assess evidence quality before synthesis."
    )]
    pub async fn evaluate_evidence(
        &self,
        Parameters(EvaluateEvidenceRequest {
            question,
            artifacts,
        }): Parameters<EvaluateEvidenceRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "evaluate_evidence", async {
            if question.trim().is_empty() {
                return Err(McpToolError::invalid_argument("question must not be empty"));
            }
            if artifacts.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "artifacts must not be empty",
                ));
            }

            // Deterministic signal computation (not LLM relay — G3 contract).
            // Scoring lives in `score_evidence_set` (pure, unit-tested).
            let scored = score_evidence_set(&artifacts, &DEFAULT_PROFILE);

            let artifact_reports: Vec<serde_json::Value> = artifacts
                .iter()
                .zip(scored.artifacts.iter())
                .map(|(artifact, score)| {
                    let mut report = serde_json::Map::new();
                    report.insert("url".to_string(), serde_json::json!(artifact.url));
                    report.insert("title".to_string(), serde_json::json!(artifact.title));
                    report.insert(
                        "confidence".to_string(),
                        serde_json::json!((score.confidence * 100.0).round() / 100.0),
                    );
                    report.insert(
                        "signals".to_string(),
                        serde_json::json!(
                            score
                                .signals
                                .iter()
                                .map(|signal| serde_json::json!({
                                    "component": signal.component.name(),
                                    "earned": (signal.earned * 100.0).round() / 100.0,
                                    "basis": signal.basis,
                                }))
                                .collect::<Vec<serde_json::Value>>()
                        ),
                    );
                    report.insert(
                        "corroboration_count".to_string(),
                        serde_json::json!(score.corroboration_count),
                    );
                    report.insert(
                        "published_age_days".to_string(),
                        serde_json::json!(score.published_age_days),
                    );
                    // Ontology-concept keys (the fixture-guarded labeling
                    // contract), routed through the bridge constants.
                    report.insert(
                        hkask_bridge_ontology::sepio::HAS_CONFIDENCE_LEVEL.to_string(),
                        serde_json::json!(format!("{:.2}", score.confidence)),
                    );
                    report.insert(
                        hkask_bridge_ontology::sepio::HAS_SUPPORTING_EVIDENCE.to_string(),
                        supporting_evidence_note(score, &scored),
                    );
                    serde_json::Value::Object(report)
                })
                .collect();

            // Overall assessment: the question's evidence base.
            let average_confidence: f64 = scored
                .artifacts
                .iter()
                .map(|score| score.confidence)
                .sum::<f64>()
                / scored.artifacts.len() as f64;

            let mut result = serde_json::json!({
                "question": question,
                "average_confidence": (average_confidence * 100.0).round() / 100.0,
                "artifacts": artifact_reports,
                "set": {
                    "distinct_domains": scored.distinct_domains,
                    "sourced_count": scored.sourced_count,
                    "content_clusters": scored
                        .content_clusters
                        .iter()
                        .map(|cluster| serde_json::json!({
                            "domains": cluster.domains,
                            "artifact_urls": cluster.artifact_urls,
                        }))
                        .collect::<Vec<serde_json::Value>>(),
                    "sensitivity": sensitivity_json(&scored.sensitivity),
                    "duplication_mode": scored.duplication_mode,
                },
            });
            // Ontology-concept key: the StepVerification concept labels this
            // result (evidence quality was assessed). Routed through the
            // fixture-guarded bridge constant, not a string literal.
            result[hkask_bridge_ontology::pko::STEP_VERIFICATION] =
                serde_json::json!("evidence_quality_assessed");
            Ok(result)
        })
        .await
    }

    // ═══════════════════ Citation ═══════════════════

    #[tool(
        description = "Generate citations from retrieved sources. Normalizes web_search/web_extract results into a canonical citation record and emits citations in the requested style (apa, bibtex, chicago, json). Deterministic formatting — no LLM relay."
    )]
    pub async fn cite_sources(
        &self,
        Parameters(CiteSourcesRequest { sources, style }): Parameters<CiteSourcesRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "cite_sources",
            async {
                if sources.is_empty() {
                    return Err(McpToolError::invalid_argument("sources must not be empty"));
                }

                let citations: Vec<String> = sources
                    .iter()
                    .map(|s| {
                        let authors = s.authors.as_ref().and_then(|a| {
                            if a.is_empty() {
                                None
                            } else {
                                Some(a.join(", "))
                            }
                        });
                        let year = s
                            .published
                            .as_deref()
                            .and_then(|p| p.get(..4))
                            .unwrap_or("n.d.");
                        let title = s.title.clone().unwrap_or_else(|| {
                            s.url.split('/').nth(3).unwrap_or("Untitled").to_string()
                        });

                        match style {
                            CiteStyle::Apa => {
                                let author_part = authors.unwrap_or_else(|| {
                                    s.source.clone().unwrap_or_else(|| "Anonymous".to_string())
                                });
                                format!(
                                    "{author_part} ({year}). {title}. Retrieved from {url}",
                                    author_part = author_part,
                                    year = year,
                                    title = title,
                                    url = s.url,
                                )
                            }
                            CiteStyle::Bibtex => {
                                let key = s
                                    .source
                                    .as_deref()
                                    .unwrap_or("unknown")
                                    .split('.')
                                    .next()
                                    .unwrap_or("unknown");
                                let author_field = authors.unwrap_or_else(|| {
                                    s.source.clone().unwrap_or_else(|| "Anonymous".to_string())
                                });
                                format!(
                                    "@misc{{{key}_{year},\n  author = {{{author_field}}},\n  title = {{{title}}},\n  year = {{{year}}},\n  url = {{{url}}}\n}}",
                                    key = key,
                                    year = year,
                                    author_field = author_field,
                                    title = title,
                                    url = s.url,
                                )
                            }
                            CiteStyle::Chicago => {
                                let author_part = authors.unwrap_or_else(|| {
                                    s.source.clone().unwrap_or_else(|| "Anonymous".to_string())
                                });
                                format!(
                                    "{author_part}. \"{title}.\" Accessed {url}.",
                                    author_part = author_part,
                                    title = title,
                                    url = s.url,
                                )
                            }
                            CiteStyle::Json => serde_json::json!({
                                "url": s.url,
                                "title": title,
                                "authors": s.authors,
                                "published": s.published,
                                "source": s.source,
                                "year": year,
                            })
                            .to_string(),
                        }
                    })
                    .collect();

                let mut result = serde_json::json!({
                    "style": serde_json::to_value(&style).unwrap_or_default(),
                    "count": citations.len(),
                    "citations": citations,
                });
                // Ontology-concept key: the dcterms:references concept labels
                // this result (citations were generated). Routed through the
                // fixture-guarded bridge constant, not a string literal.
                result[hkask_bridge_ontology::dc_bibo::REFERENCES] =
                    serde_json::json!("citations_generated");
                Ok(result)
            },
        )
        .await
    }
}

// ── evaluate_evidence output helpers ──

/// The SEPIO:0000440 (has-supporting-evidence) note in the duplicate-aware
/// model's terms: independent units corroborate, or the artifacts are one
/// duplicated unit — never a domain count that syndication inflates.
fn supporting_evidence_note(score: &ArtifactScore, scored: &EvidenceReport) -> serde_json::Value {
    if score.corroboration_count > 1 {
        serde_json::Value::String(format!(
            "{} independent evidence units corroborate the evidence base",
            score.corroboration_count
        ))
    } else if scored.sourced_count > 1 {
        serde_json::Value::String(format!(
            "{} artifacts form one evidence unit — duplication, not independent corroboration",
            scored.sourced_count
        ))
    } else {
        serde_json::Value::Null
    }
}

/// The wire form of the sensitivity report.
fn sensitivity_json(sensitivity: &SensitivityStatus) -> serde_json::Value {
    match sensitivity {
        SensitivityStatus::Stable { profiles_evaluated } => serde_json::json!({
            "status": "stable",
            "profiles_evaluated": profiles_evaluated,
        }),
        SensitivityStatus::Unstable { driver } => serde_json::json!({
            "status": "unstable",
            "driver": driver.name(),
        }),
        SensitivityStatus::NotEvaluable { reason } => serde_json::json!({
            "status": "not_evaluable",
            "reason": reason,
        }),
    }
}

// ── Entry point ──

/// Run the research MCP server (used by binary target).
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    // Resolve the inference port before entering the sync server-
    // construction closure. `resolve_inference_port` is async (it constructs
    // a `LazyInferencePort` — the bridge connection itself is deferred to
    // each call, which re-tries the socket); the closure passed to
    // `run_server` is sync, so the await must happen here. Used by the deep
    // strategy's LLM rerank stage.
    let inference_port = hkask_inference::resolve_inference_port().await;
    hkask_mcp_server::run_server(
        "hkask-mcp-research",
        SERVER_VERSION,
        |ctx: ServerContext| {
            let cache_ttl = hkask_mcp_server::parse_env_warn(
                "HKASK_WEB_CACHE_TTL_SECS",
                DEFAULT_CACHE_TTL_SECS,
            )
            .min(MAX_CACHE_TTL_SECS);
            let cache_max = hkask_mcp_server::parse_env_warn(
                "HKASK_WEB_CACHE_MAX_ENTRIES",
                DEFAULT_CACHE_MAX_ENTRIES,
            )
            .min(MAX_CACHE_MAX_ENTRIES);

            let pool = build_provider_pool(&ctx.credentials).map_err(|e| {
                hkask_mcp_server::McpError::UnexpectedResponse {
                    context: "research server init".into(),
                    detail: e.to_string(),
                }
            })?;

            let research_db = {
                // Databases live in the internal data dir (the ONLY thing that
                // lives there — artifact files and outputs go to the visible
                // artifacts dir under {server}-mcp/{artifact-type}/). Default
                // DB path is `{kask_data_dir}/mcp/research/research.db`, resolved
                // via `resolve_under_data_dir`. Override via `HKASK_RESEARCH_DB`.
                let research_db_path = std::env::var("HKASK_RESEARCH_DB").ok().unwrap_or_else(|| {
                    let default_path = hkask_types::agent_paths::resolve_under_data_dir(
                        &hkask_types::agent_paths::mcp_server_db("research", "research"),
                    );
                    if let Some(parent) = default_path.parent() {
                        if let Err(error) = std::fs::create_dir_all(parent) {
                            tracing::warn!(
                                target: "hkask.research.init",
                                path = %default_path.display(),
                                %error,
                                "Failed to create default research DB directory \
                                 — the subsequent DB open will surface the failure"
                            );
                        }
                    }
                    tracing::info!(
                        target: "hkask.research.init",
                        path = %default_path.display(),
                        "Using default research database path (HKASK_RESEARCH_DB not set)"
                    );
                    default_path.to_string_lossy().to_string()
                });

                // Resolve passphrase via the canonical 2-tier chain
                // (ctx.credentials → resolve_credential which does env → keychain).
                let passphrase = match resolve_db_passphrase(&ctx.credentials) {
                    Ok(passphrase) => Some(passphrase),
                    Err(error) => {
                        tracing::warn!(
                            target = "hkask.research.init",
                            %error,
                            "Falling back to no research database. RSS and research-run tools will be unavailable."
                        );
                        None
                    }
                };

                match passphrase {
                    Some(passphrase) => {
                        match hkask_storage::Database::open_with_extensions(
                            &research_db_path,
                            &passphrase,
                            db::RESEARCH_SCHEMA_DDL,
                        ) {
                            Ok(db) => db
                                .sqlite_pool()
                                .map_err(|e| {
                                    // Opened-but-broken must not read as "not
                                    // configured" — warn so the operator can
                                    // distinguish the two.
                                    tracing::warn!(
                                        target = "hkask.research.init",
                                        error = %e,
                                        path = %research_db_path,
                                        "Research database opened but pool extraction failed — \
                                         RSS and research-run tools will be unavailable"
                                    );
                                    e
                                })
                                .ok(),
                            Err(e) => {
                                tracing::warn!(
                                    target = "hkask.research.init",
                                    error = %e,
                                    path = %research_db_path,
                                    "Failed to open research database — RSS and research-run tools will be \
                                     unavailable. Check HKASK_RESEARCH_DB path and \
                                     HKASK_DB_PASSPHRASE."
                                );
                                None
                            }
                        }
                    }
                    None => None,
                }
            };

            let rss_client = Client::builder()
                .user_agent(format!("hkask-mcp-research/{}", SERVER_VERSION))
                .build()
                .map_err(|e| hkask_mcp_server::McpError::UnexpectedResponse {
                    context: "research rss client build".into(),
                    detail: e.to_string(),
                })?;

            // Strict-policy fetch client for `rss_discover_feeds` — every
            // redirect hop and every connect-time resolution gated.
            let discover_client = validated_fetch_client().map_err(|e| {
                hkask_mcp_server::McpError::UnexpectedResponse {
                    context: "research discover client build".into(),
                    detail: e.to_string(),
                }
            })?;

            // Resolve the inference port for the deep strategy's LLM rerank.
            // Resolved above (before the sync construction closure); the lazy
            // port re-tries the bridge on each call, so a socket that appears
            // after server start is picked up without a restart.
            Ok(ResearchServer::new(
                ctx.webid,
                Arc::new(pool),
                Arc::new(ResponseCache::new(
                    cache_max,
                    Duration::from_secs(cache_ttl),
                )),
                RateLimiter::new(RATE_LIMIT_MAX_REQUESTS, RATE_LIMIT_WINDOW_SECS),
                research_db,
                rss_client,
                discover_client,
                inference_port.clone(),
                // The visible settings chain, resolved once at the
                // construction seam (no hidden constant — the operator's
                // no-hidden-models spec).
                hkask_inference::model_constants::rerank_model(),
            ))
        },
        credential_requirements(),
    )
    .await
}

pub(crate) fn credential_requirements() -> Vec<CredentialRequirement> {
    let opt = CredentialRequirement::optional;
    vec![
        opt("HKASK_BRAVE_API_KEY", "Brave Search API key"),
        opt("HKASK_FIRECRAWL_API_KEY", "Firecrawl API key"),
        opt("HKASK_TAVILY_API_KEY", "Tavily API key"),
        opt("HKASK_SERPAPI_API_KEY", "SerpAPI key"),
        opt("HKASK_EXA_API_KEY", "Exa API key"),
        opt(
            "HKASK_DB_PASSPHRASE",
            "Passphrase for SQLCipher encryption (required if HKASK_RESEARCH_DB is set)",
        ),
    ]
}
