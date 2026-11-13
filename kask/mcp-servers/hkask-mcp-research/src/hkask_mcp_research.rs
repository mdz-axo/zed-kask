#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]

pub(crate) mod evidence_service;
pub(crate) mod ontology;
pub mod research;

// Shared cfg(test) fixtures — `src/test_support.rs` (the media crate's
// test_support precedent).
#[cfg(test)]
mod test_support;

// Re-export service crate modules for test compatibility
pub use crate::research::db;

use std::sync::Arc;
use std::time::Duration;

use hkask_mcp_server::server::{
    CredentialRequirement, McpToolError, ServerContext, execute_tool, map_join_error,
    resolve_db_passphrase, validate_tool_url_with_dns,
};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use rusqlite::Connection;

use crate::evidence_service::{
    run_source_record, search_url_in_domains, sensitivity_json, supporting_evidence_note,
    validate_extraction,
};
use crate::research::{
    AnnotateResearchRunRequest, BeginResearchRunRequest, CiteSourcesRequest,
    DEFAULT_CACHE_MAX_ENTRIES, DEFAULT_CACHE_TTL_SECS, DEFAULT_PROFILE, EvaluateEvidenceRequest,
    ExtractOptions, ExtractOutput, ExtractRequest, FindSimilarOutput, FindSimilarRequest,
    FindSimilarResultOutput, FinishResearchRunRequest, GetResearchRunRequest,
    MAX_CACHE_MAX_ENTRIES, MAX_CACHE_TTL_SECS, MAX_JSON_PROMPT_LENGTH, MAX_JSON_SCHEMA_BYTES,
    MAX_QUERY_LENGTH, MAX_URL_LENGTH, NewResearchRun, PingOutput, ProviderProfileOutput,
    ProviderRecommendation, RateLimiter, RerankInfo, RerankOutcome, ResolvePaperRequest,
    ResponseCache, RunSourceRecord, SearchMetadata, SearchOutput, SearchQuery, SearchRequest,
    SearchResultOutput, SearchStrategy, WebError, WebSearchPort, build_provider_pool, cache_key,
    llm_rerank, normalize_closed_vocab, parse_closed_vocab, parse_closed_vocab_const,
    provider_profile, score_evidence_set,
};

// ── Constants ──

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const RATE_LIMIT_MAX_REQUESTS: u32 = 30;
const RATE_LIMIT_WINDOW_SECS: u64 = 60;

// ── ResearchServer ──

hkask_mcp_server::mcp_server!(
    pub struct ResearchServer {
        pub pool: Arc<dyn WebSearchPort>,
        pub cache: Arc<ResponseCache>,
        pub rate_limiter: RateLimiter,
        pub research_db: Option<r2d2::Pool<hkask_storage::SqliteConnectionManager>>,
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
        /// The embedding model for the semantic duplication tier — resolved
        /// ONCE at the construction seam from `HKASK_EMBEDDING_MODEL` (the
        /// visible `kask.embedding_model` setting chain). `None` = not
        /// configured: the tier degrades to the deterministic shingle floor
        /// with a surfaced reason naming the setting — unset is a legitimate
        /// degraded mode because the floor exists (never a hidden constant).
        pub embedding_model: Option<String>,
        /// zed-kask: D100 (operator-approved 2026-11-06, the tool-query-embed
        /// design's §8 legs) — the private leg's LOCAL embedding model for
        /// the semantic duplication tier on private-provenance calls,
        /// resolved ONCE at the construction seam from
        /// `HKASK_PRIVATE_EMBEDDING_MODEL` (the visible
        /// `kask.private_inference.embedding_model` setting — the same
        /// one-home reader the curator child routes by; the model string IS
        /// the routing signal). `None`/empty = the operator's explicit
        /// opt-out → the surfaced shingle-floor degrade, never the
        /// configured cloud embedder — private artifact content never
        /// widens.
        pub private_embedding_model: Option<String>,
        /// zed-kask: D100 §8.1 routed shape (operator ruling 2026-11-06)
        /// — the private leg's LOCAL rerank model for private-provenance
        /// deep-search rerank, resolved ONCE at the construction seam from
        /// `HKASK_PRIVATE_RERANK_MODEL` (the visible
        /// `kask.private_inference.rerank_model` setting — the one-home
        /// reader; the model string IS the routing signal: the zed side
        /// serves `ollama/`-qualified rerank models through the local
        /// judge protocol, loopback and keyless). `None`/empty = the
        /// operator's explicit opt-out → the surfaced heuristic skip,
        /// never the configured cloud reranker — private thread content
        /// never widens.
        pub private_rerank_model: Option<String>,
    } = crate::ontology::annotate;
);

// ── Database helpers ──

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
            _ => McpToolError::internal(message),
        };
    }
    McpToolError::internal(message)
}

/// Require the research database, returning an Err if not configured.
/// The message names the actual causes: the DB opens at its DEFAULT path
/// when `HKASK_RESEARCH_DB` is unset, so an unconfigured DB is almost always
/// a missing `HKASK_DB_PASSPHRASE` or an open failure — not a missing
/// `HKASK_RESEARCH_DB` (the var only selects a custom location).
macro_rules! require_research_db {
    ($self:expr) => {
        match &$self.research_db {
            Some(db) => db.clone(),
            None => {
                return Err(McpToolError::permission_denied(
                    "Research database not available — set HKASK_DB_PASSPHRASE \
                     (the DB opens at its default path; set HKASK_RESEARCH_DB only for a \
                     custom location) and check the server log for an open failure.",
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
            // Static provider profiles live here (2026-09-30, zk-reference
            // lesson L5): the table is per-server-version constant, so it is
            // served once per ping instead of ~1KB on every web_search
            // response. web_search keeps the per-call
            // `provider_recommendations` audit of tool-selected picks.
            let provider_profiles: Vec<ProviderProfileOutput> = self
                .pool
                .provider_kinds()
                .iter()
                .filter_map(|kind| provider_profile(kind).map(ProviderProfileOutput::from))
                .collect();
            let output = PingOutput {
                status: "ok".to_string(),
                version: SERVER_VERSION.to_string(),
                providers,
                provider_profiles,
            };
            Ok(serde_json::to_value(&output)
                .unwrap_or_else(|_| serde_json::json!({"error": "serialization failed"})))
        })
        .await
    }

    #[tool(description = "Search the web with RRF fusion across providers. \
         Set `provider` to query a single named provider (tavily, brave, exa, \
         firecrawl, serpapi, or the free scholarly providers openalex, arxiv, \
         semantic_scholar) — no fusion, no fallback. Or set `intent` (news, \
         academic, semantic, research, freshness, general, transcript) to have the tool \
         score the configured providers against the query and pick the top \
         recommendation for you — the ranking is surfaced in \
         provider_recommendations (carried on every tool-selected path). \
         When both are None, `strategy` selects: \
         quick (best-scored single keyword provider, live-performance-aware — \
         a provider that keeps failing drops out), web (all, RRF fusion), \
         news (news-capable), deep (all + 2x results + content extraction). \
         Fusion skips providers whose circuit breaker is open (3+ consecutive \
         failures) — skipped providers appear in providers_skipped (not \
         providers_failed; no request was made to a skipped provider, and the \
         breaker's cooldown is 60s). \
         The google_scholar and google_books engines need operator-qualified \
         queries for exact-title lookups (e.g. intitle:\"The Logic of Scientific \
         Discovery\") — plain titles return generic web noise; their results do \
         not expose Google Books view status (full view vs preview). For an \
         exact-title paper lookup, prefer resolve_paper's `title` mode.")]
    pub async fn web_search(
        &self,
        Parameters(mut req): Parameters<SearchRequest>,
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
            // Closed-vocabulary fields cross the LLM boundary: normalize
            // predictable emission noise (surrounding whitespace, one layer
            // of symmetric quotes) once, up front — before validation AND
            // before the cache key, so the quoted and bare forms of the same
            // request share one cache entry (observed live: `"\"quick\""`
            // rejected as an unknown strategy cost agents retries). The
            // `provider` pass-through normalizes here (classification lives
            // in the pool); `intent` parses to its canonical spelling via
            // the shared closed-vocabulary helper (C2).
            req.provider = req.provider.map(|p| normalize_closed_vocab(&p).to_string());
            // Intent is a closed vocabulary, not a free string: the scorer
            // silently maps an unrecognized intent to no-bonus generic
            // ranking, so a typo would read as a successful request routed
            // under the intended lens. The parse normalizes emission noise
            // and rejects unknown values up front, against the same list
            // the input schema publishes (`INTENTS`).
            let intent = match req.intent.as_deref() {
                Some(raw) => Some(
                    parse_closed_vocab_const("intent", raw, crate::research::types::INTENTS)
                        .map_err(McpToolError::from)?,
                ),
                None => None,
            };

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
                    "intent": intent,
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

            // Provider selection + audit ranking. C4: the ranking is
            // surfaced on every path where the tool makes the selection
            // (intent pick, quick pick, fusion) — the audit trail for WHY
            // providers were (or would be) chosen. The intent path picks the
            // top configured recommendation (the former
            // web_recommend_provider + web_search(provider) two-step, folded
            // in); quick picks inside the pool under the same live-merged
            // model; web/news/deep fuse. Explicit-provider calls are the
            // caller's own deliberate override — they carry
            // selected_provider and skip the ranking.
            let mut provider = req.provider.clone();
            let mut provider_recommendations: Vec<ProviderRecommendation> = Vec::new();
            if provider.is_none() {
                provider_recommendations = self.pool.score_providers(&req.query, intent);
                if intent.is_some()
                    && let Some(top) = provider_recommendations.iter().find(|r| r.configured)
                {
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

            let before_domain_filter = compound.results.len();
            compound.results.retain(|result| {
                search_url_in_domains(
                    &result.url,
                    &search_query.include_domains,
                    &search_query.exclude_domains,
                )
            });
            let domain_filter_removed = before_domain_filter - compound.results.len();
            if domain_filter_removed > 0 {
                tracing::warn!(
                    target: "hkask.web",
                    domain_filter_removed,
                    "search provider returned results outside the requested domain boundary"
                );
            }
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
                // zed-kask: D100 (the tool-query-embed design §8.1,
                // operator-approved 2026-11-06; the routed shape by the
                // operator's 2026-11-06 ruling — private mode uses the
                // ollama qwen reranker) — the provenance-routed rerank
                // leg: a private-provenance call reranks through the
                // private leg's LOCAL model ONLY
                // (`kask.private_inference.rerank_model`, injected as
                // HKASK_PRIVATE_RERANK_MODEL — the zed side's local arm
                // serves the `ollama/`-qualified judge protocol over
                // loopback, keyless); the configured cloud reranker never
                // sees private content. Unset/empty is the operator's
                // explicit opt-out → the surfaced skip, the reason naming
                // the boundary and the setting.
                let outcome = if hkask_mcp_server::current_call_provenance()
                    == hkask_types::InferenceProvenance::PrivateInference
                {
                    match self.private_rerank_model.as_deref() {
                        Some(private_model) => {
                            llm_rerank(
                                self.inference_port.as_ref(),
                                &req.query,
                                &mut compound.results,
                                private_model,
                            )
                            .await
                        }
                        None => RerankOutcome {
                            scored: 0,
                            failed: compound.results.len(),
                            first_error: Some(
                                "private-provenance call — the rerank stage is skipped \
                                     (private thread content never reaches the configured \
                                     kask.models.rerank_model); set \
                                     kask.private_inference.rerank_model (injected as \
                                     HKASK_PRIVATE_RERANK_MODEL) to route it locally, or \
                                     leave it empty for the surfaced skip; heuristic order \
                                     kept"
                                    .to_string(),
                            ),
                        },
                    }
                } else {
                    match self.rerank_model.as_deref() {
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
                    }
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

            let metadata = SearchMetadata::from(&compound);
            tracing::info!(
                target: "hkask.web",
                strategy = %metadata.strategy,
                selected_provider = ?selected_provider.as_ref(),
                providers_queried = ?metadata.providers_queried,
                providers_succeeded = ?metadata.providers_succeeded,
                providers_failed = ?metadata.providers_failed,
                providers_skipped = ?metadata.providers_skipped,
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
                providers_skipped: compound.providers_skipped.clone(),
                domain_filter_removed,
                selected_provider,
                provider_recommendations,
                rerank,
            };

            let output = serde_json::to_value(&search_output)
                .unwrap_or_else(|_| serde_json::json!({ "error": "serialization failed" }));

            // Cache only clean, PUBLIC-provenance responses (operator
            // ruling 2026-11-07, dual-axis review Q4): a private call's
            // deep-search output is reordered by the private leg's LOCAL
            // rerank model — caching it would serve a private thread's
            // model-mediated ordering to a later same-query caller within
            // the TTL (the D100 known-interaction class; the write gate
            // removes the blemish — private deep searches are rare and the
            // rerank is loopback, so the re-run cost is one provider
            // search). A compound carrying provider failures — in
            // single-provider mode that is an empty result plus a failure
            // record — must not be cached either: a transient provider
            // failure would otherwise be replayed as a "successful" empty
            // result for the full cache TTL.
            let private_call = hkask_mcp_server::current_call_provenance()
                == hkask_types::InferenceProvenance::PrivateInference;
            if compound.providers_failed.is_empty() && !private_call {
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
                    .map(|result| {
                        run_source_record(
                            &result.url,
                            &result.title,
                            result.published.as_deref(),
                            result.source.as_deref(),
                            result.content_preview.as_deref(),
                            result.description.as_deref(),
                            result.providers.first().cloned(),
                        )
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
                    .map(|result| {
                        run_source_record(
                            &result.url,
                            &result.title,
                            result.published.as_deref(),
                            result.source.as_deref(),
                            result.content_preview.as_deref(),
                            result.description.as_deref(),
                            Some("exa".to_string()),
                        )
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
            // something else. parse_closed_vocab normalizes emission noise
            // and rejects unknown values up front (the shared
            // closed-vocabulary contract, C2).
            let fmt: &str = match format.as_deref() {
                None => "markdown",
                Some(raw) => parse_closed_vocab(
                    "format",
                    raw,
                    &[(&["markdown"], "markdown"), (&["json"], "json")],
                )
                .map_err(McpToolError::from)?,
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
                format: fmt.to_string(),
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
                if validate_extraction(
                    fmt,
                    cached.get("format").and_then(serde_json::Value::as_str),
                    cached.get("content").and_then(serde_json::Value::as_str),
                )
                .is_ok()
                {
                    return Ok(cached);
                }
                tracing::warn!(target: "hkask.web", "Ignoring cached extraction that violates the requested format");
            }

            let extracted = self
                .pool
                .extract(&url, &opts)
                .await
                .map_err(McpToolError::from)?;
            validate_extraction(
                fmt,
                Some(&extracted.format),
                Some(&extracted.content),
            )?;
            // Degradation surfacing (2026-09-30, zk-reference lesson L1): an
            // extraction can succeed HTTP-wise yet capture only page chrome —
            // the origin served a JS shell, a bot-block page, or an empty
            // body (observed: a Cloudflare-gated publisher article extracted
            // as nav chrome + metadata with statusCode 200 and was cached as
            // a valid extraction). A near-empty body on a successful fetch is
            // surfaced as a note, never a bare success — and a degraded
            // extraction is NOT cached, so a retry re-fetches the origin
            // instead of replaying the shell.
            const MIN_SUBSTANTIVE_CONTENT_CHARS: usize = 500;
            let content_chars = extracted.content.trim().chars().count();
            let degraded = content_chars < MIN_SUBSTANTIVE_CONTENT_CHARS;
            let note = degraded.then(|| {
                // A degraded PDF URL: the chain returned a page shell, not
                // PDF text (the local extractor errored and a scraper leg
                // returned chrome — observed 2026-11-01: a 153-char shell
                // for a Locke & Latham PDF). Name the PDF route, not the
                // generic JS-shell guess.
                if crate::research::providers::url_looks_like_pdf(&extracted.url) {
                    return format!(
                        "extraction captured only {content_chars} chars — the URL is a \
                         PDF and the extract chain returned a page shell, not PDF text; \
                         retry, or download the file (e.g. via terminal curl) and run it \
                         through the corpus server's corpus_convert pipeline, which \
                         handles PDFs (with OCR)"
                    );
                }
                // P2 (2026-10-01): when the fetch metadata carries a
                // non-2xx status, name it — a thin 404 body is a gone or
                // moved URL, not a JS shell (observed: a 404 origin
                // surfaced as "36 chars — JS shell / bot-block / empty
                // body" while the metadata held statusCode 404).
                let origin_status = extracted
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("statusCode"))
                    .and_then(|v| {
                        v.as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                    })
                    .filter(|code| !(200..300).contains(code));
                match origin_status {
                    Some(code) => format!(
                        "extraction captured only {content_chars} chars — the origin \
                         returned HTTP {code} (the URL may be gone or moved); the \
                         content may be incomplete (consider the Wayback Machine \
                         or a headless-browser route for this URL)"
                    ),
                    None => format!(
                        "extraction captured only {content_chars} chars — the origin \
                         likely served a JS shell, a bot-block page, or an empty body; \
                         the content may be incomplete (consider the Wayback Machine \
                         or a headless-browser route for this URL)"
                    ),
                }
            });
            if degraded {
                tracing::warn!(
                    target: "hkask.web",
                    url = %url,
                    content_chars,
                    "web_extract captured a near-empty body; surfacing degradation note"
                );
            }
            let extract_output = ExtractOutput {
                url: extracted.url,
                format: extracted.format,
                content: extracted.content,
                metadata: extracted.metadata,
                note,
            };
            let mut output = serde_json::to_value(&extract_output)
                .unwrap_or_else(|_| serde_json::json!({ "error": "serialization failed" }));
            if !degraded {
                self.cache.insert(ckey, output.clone()).await;
            }

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

    // ═══════════════════ Research-run ledger ═══════════════════

    #[tool(
        description = "Begin a research run: opens a server-side ledger that records what the research tools actually return under this run. Returns {run_id, status}. Pass the run_id to web_search / web_extract / web_find_similar to record their results non-repudiably (recorded_by='server'); read the manifest via get_research_run. Use this for bulk-retrieval sweeps too: begin the run first and pass run_id on every search/extract, and the sweep's claims are backed by a server-recorded manifest instead of agent assertion (observed 2026-09-30: a 46-upgrade reference sweep ran without a run and its evidence stayed agent-asserted)."
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
        description = "Mark a research run finished with a terminal status (completed, partial, blocked, or failed). The sources ledger is never mutated: the transition appends to the run's status history (the audit trail) and updates the run's latest status. completed/partial require at least one server-recorded source — finish as blocked or failed when no run-scoped search/extract recorded anything."
    )]
    pub async fn finish_research_run(
        &self,
        Parameters(req): Parameters<FinishResearchRunRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "finish_research_run", async {
            if req.run_id.trim().is_empty() {
                return Err(McpToolError::invalid_argument("run_id must not be empty"));
            }
            if req.status.trim().is_empty() {
                return Err(McpToolError::invalid_argument("status must not be empty"));
            }
            // Status is a closed vocabulary owned by runs.rs
            // (FINISH_STATUSES): parse at the boundary for emission
            // tolerance — the domain guard in finish_research_run stays
            // the owner and receives the canonical spelling.
            let status = parse_closed_vocab_const(
                "status",
                &req.status,
                crate::research::runs::FINISH_STATUSES,
            )
            .map_err(McpToolError::from)?;
            let database = require_research_db!(self);
            let run_id_for_task = req.run_id.clone();
            let status_for_task = status.to_string();
            let note_for_task = req.note.clone();
            let result = spawn_db(database, move |connection| {
                Ok(crate::research::runs::finish_research_run(
                    connection,
                    &run_id_for_task,
                    &status_for_task,
                    note_for_task.as_deref(),
                ))
            })
            .await;
            match result {
                Ok(Ok(Ok(Some(summary)))) => Ok(summary),
                Ok(Ok(Ok(None))) => Err(McpToolError::not_found(format!(
                    "research run '{}' not found",
                    req.run_id
                ))),
                Ok(Ok(Err(error @ crate::research::runs::FinishRunError::InvalidStatus(_)))) => {
                    Err(McpToolError::invalid_argument(error.to_string()))
                }
                Ok(Ok(Err(
                    error @ crate::research::runs::FinishRunError::RequiresServerSource(_),
                ))) => Err(McpToolError::failed_precondition(error.to_string())),
                Ok(Ok(Err(crate::research::runs::FinishRunError::Db(error)))) => {
                    Err(map_db_error(error))
                }
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
            // verification_state is a closed vocabulary owned by runs.rs
            // (VERIFICATION_STATES): boundary tolerance, domain ownership —
            // the guard in annotate_run_source receives the canonical
            // spelling.
            let state = parse_closed_vocab_const(
                "verification_state",
                &verification_state,
                crate::research::runs::VERIFICATION_STATES,
            )
            .map_err(McpToolError::from)?;
            let database = require_research_db!(self);
            let run_id_for_task = run_id.clone();
            let url_for_task = url.clone();
            let state_for_task = state.to_string();
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
                    "verification_state": state,
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

    #[tool(
        description = "Resolve a paper reference to a typed identity. Two modes: (1) identifier — pass `query` with any supported form (DOI, arXiv ID, PMID, PMCID, OpenAlex work ID — bare, prefixed, or registry URL) to parse, normalize, and enrich it; (2) bibliographic — pass `title` with a work's title for Crossref's bibliographic search: up to 3 candidates (DOI, title, year, venue, first author) are surfaced, ranked with exact-title matches first (Crossref relevance within ties), and the top candidate is resolved to the typed identity and enriched via OpenAlex — check the candidates before trusting the resolution (a title search can hit a different work than intended). Both modes return the identifier kind/value, canonical URL, stable ledger key, and OpenAlex metadata (title, authors, year, venue, and the candidate open-access copy URL `oa_pdf_url` when the record holds one — a candidate, not a verified document) — the identity is always returned even when the metadata lookup degrades. Pass run_id to record the resolution into a research run's ledger. Every rejection names what was expected."
    )]
    pub async fn resolve_paper(
        &self,
        Parameters(ResolvePaperRequest {
            query,
            title,
            run_id,
        }): Parameters<ResolvePaperRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "resolve_paper", async {
            // Exactly one of query (identifier) / title (bibliographic) —
            // passing both is ambiguous, neither is empty input. Both
            // rejections name what was expected.
            let (parsed, candidates) = match (query, title) {
                (Some(query), None) => {
                    let query = query.trim().to_string();
                    if query.is_empty() {
                        return Err(McpToolError::invalid_argument(
                            "query must not be empty (pass either query — an identifier — or title — a bibliographic lookup)",
                        ));
                    }
                    (crate::research::paper_id::parse_paper_id(&query)?, None)
                }
                (None, Some(title)) => {
                    let title = title.trim().to_string();
                    if title.is_empty() {
                        return Err(McpToolError::invalid_argument(
                            "title must not be empty (pass either query — an identifier — or title — a bibliographic lookup)",
                        ));
                    }
                    // Crossref bibliographic resolution: candidates are
                    // surfaced in full so the caller can verify the match
                    // (the no-substitution rule) before trusting the top
                    // candidate's resolution. P1 (2026-10-01): exact-title
                    // matches rank first — Crossref's relevance can put a
                    // superstring title above the exact work (observed:
                    // "Computing Machinery and Intelligence Amplification"
                    // outranked Turing's "Computing Machinery and
                    // Intelligence"); the stable sort keeps Crossref's order
                    // within each group, so equally-exact candidates (a
                    // preprint and the published version of the same work)
                    // stay consumer-verifiable via the surfaced list.
                    let mut candidates = self.pool.resolve_title(&title, 3).await.map_err(|e| {
                        // The kind is load-bearing for the reliability ledger:
                        // a Crossref 429 burst (observed 2026-11-02: parallel
                        // run-scoped resolve_paper calls all Crossref-429'd,
                        // the outcome breakdown carried `failed_precondition`
                        // and the sensor degraded to 0.75 on transient
                        // throttling) must surface as external rate limiting,
                        // not as a tool defect. The honest kind per variant:
                        // 429 → `rate_limited` (still degrades — the tool
                        // genuinely failed for the caller — but the breakdown
                        // names the remedy: back off, don't diagnose the
                        // tool); transport failure → `unavailable` (the
                        // config-gap class the ledger's not-tool-fault policy
                        // already excludes from the success-rate math).
                        match e {
                            WebError::RateLimited(detail) => {
                                McpToolError::rate_limited(format!(
                                    "Crossref bibliographic search rate limited: {detail}"
                                ))
                            }
                            WebError::ProviderUnavailable(detail) => {
                                McpToolError::unavailable(format!(
                                    "Crossref bibliographic search unavailable: {detail}"
                                ))
                            }
                            other => McpToolError::failed_precondition(format!(
                                "Crossref bibliographic search failed: {other}"
                            )),
                        }
                    })?;
                    candidates.sort_by_key(|c| {
                        !crate::research::providers::title_matches_exactly(&title, &c.title)
                    });
                    let Some(top) = candidates.first() else {
                        return Ok(serde_json::json!({
                            "mode": "bibliographic",
                            "query_title": title,
                            "note": "no Crossref candidates for this title — the work may be unregistered, or the title may not match any registered bibliographic record",
                        }));
                    };
                    let parsed = crate::research::paper_id::parse_paper_id(&top.doi)?;
                    (parsed, Some(candidates))
                }
                (Some(_), Some(_)) => {
                    return Err(McpToolError::invalid_argument(
                        "pass either query (an identifier) or title (a bibliographic lookup), not both",
                    ));
                }
                (None, None) => {
                    return Err(McpToolError::invalid_argument(
                        "pass either query (an identifier: DOI, arXiv ID, PMID, PMCID, or OpenAlex work ID) or title (a bibliographic lookup)",
                    ));
                }
            };
            let mut result = serde_json::json!({
                "identifier": {
                    "kind": parsed.kind(),
                    "value": parsed.value(),
                },
                "canonical_url": parsed.canonical_url(),
                "stable_key": crate::research::paper_id::stable_paper_key(&parsed),
            });
            if let Some(candidates) = candidates {
                result["mode"] = serde_json::json!("bibliographic");
                result["candidates"] = serde_json::to_value(&candidates)
                    .unwrap_or_else(|_| serde_json::json!("serialization failed"));
            }

            // OpenAlex enrichment — best-effort, surfaced either way: the
            // identity is the deterministic floor and never blocked by the
            // metadata tier. Ok(None) = no record (or no lookup path for the
            // kind); Err = the lookup failed — both surface as notes.
            let metadata = match self.pool.resolve_paper_id(&parsed).await {
                Ok(Some(metadata)) => {
                    result["openalex"] = serde_json::json!({
                        "openalex_id": metadata.openalex_id,
                        "doi": metadata.doi,
                        "title": metadata.title,
                        "publication_year": metadata.publication_year,
                        "authors": metadata.authors,
                        "venue": metadata.venue,
                        "oa_pdf_url": metadata.oa_pdf_url,
                    });
                    Some(metadata)
                }
                Ok(None) => {
                    result["openalex"] = serde_json::json!({
                        "note": "no OpenAlex record for this identifier",
                    });
                    None
                }
                Err(error) => {
                    result["openalex"] = serde_json::json!({
                        "error": error.to_string(),
                    });
                    None
                }
            };

            // Run-ledger recording (C2 composition): the server observed
            // this resolution — record the canonical URL with the metadata
            // (when present) as the audit copy.
            if let Some(run_id) = run_id.as_deref() {
                let record = RunSourceRecord {
                    url: parsed.canonical_url(),
                    provider: Some("openalex".to_string()),
                    title: metadata.as_ref().map(|metadata| metadata.title.clone()),
                    published: metadata.as_ref().and_then(|metadata| {
                        metadata.publication_year.map(|year| year.to_string())
                    }),
                    source: metadata
                        .as_ref()
                        .and_then(|metadata| metadata.venue.clone()),
                    excerpt: metadata.as_ref().map(|metadata| {
                        format!(
                            "openalex {} — {} ({})",
                            metadata.openalex_id,
                            metadata.authors.join(", "),
                            metadata
                                .publication_year
                                .map(|year| year.to_string())
                                .unwrap_or_else(|| "no year".to_string())
                        )
                    }),
                };
                result["run_ledger"] = self.append_run_ledger(run_id, vec![record]).await;
            }

            Ok(result)
        })
        .await
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
            duplication,
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
            // Duplication is a closed vocabulary: parse_closed_vocab
            // normalizes emission noise and rejects unknown values (the
            // shared closed-vocabulary contract, C2).
            let duplication: Option<&str> = match duplication.as_deref() {
                None => None,
                Some(raw) => Some(
                    parse_closed_vocab("duplication", raw, &[(&["semantic"], "semantic")])
                        .map_err(McpToolError::from)?,
                ),
            };

            // Tier-2 duplication (parameter-gated): the caller opts in with
            // duplication="semantic". The deterministic shingle floor runs
            // when the tier is not requested, no model is configured, or the
            // embed call fails — every degradation surfaced with its reason,
            // never silent (the rerank degradation contract).
            // zed-kask: D100 (operator-approved 2026-11-06, the
            // tool-query-embed design's §8 legs) — the provenance-routed
            // duplication tier: a private-provenance call embeds its
            // artifact content through the private leg's LOCAL model only
            // (the model string IS the routing signal — the registry
            // resolves it to the local ollama descriptor; the vectors stay
            // within-batch, so the local model's native width is correct
            // here). Unset/empty is the operator's explicit opt-out → the
            // surfaced shingle-floor degrade naming the private setting —
            // the content NEVER widens to the configured cloud embedder.
            let private_call = hkask_mcp_server::current_call_provenance()
                == hkask_types::InferenceProvenance::PrivateInference;
            let mut duplication_reason: Option<String> = None;
            let semantic_vectors: Option<Vec<(usize, Vec<f32>)>> = match duplication {
                Some("semantic") => {
                    let resolved_model = if private_call {
                        self.private_embedding_model.as_deref()
                    } else {
                        self.embedding_model.as_deref()
                    };
                    match resolved_model {
                        None => {
                            duplication_reason = Some(
                                if private_call {
                                    "private call context: no private embedding model \
                                 configured — set kask.private_inference.embedding_model \
                                 (injected as HKASK_PRIVATE_EMBEDDING_MODEL); the semantic \
                                 tier is skipped for private contexts and the artifact \
                                 content never widens to the configured cloud embedder; \
                                 the deterministic shingle floor ran"
                                } else {
                                    "semantic duplication requested but no embedding model \
                                 configured — set kask.embedding_model (injected as \
                                 HKASK_EMBEDDING_MODEL); the deterministic shingle \
                                 floor ran instead"
                                }
                                .to_string(),
                            );
                            None
                        }
                        Some(model) => {
                            let content_indices: Vec<usize> = artifacts
                                .iter()
                                .enumerate()
                                .filter(|(_, artifact)| artifact.content.is_some())
                                .map(|(index, _)| index)
                                .collect();
                            let texts: Vec<String> = content_indices
                                .iter()
                                .map(|&index| artifacts[index].content.clone().unwrap_or_default())
                                .collect();
                            // Within-batch comparison only — the vectors never
                            // touch a width-bound store, so the model's native
                            // width is correct here (no `dimensions` request).
                            match self.inference_port.embed(model, &texts).await {
                                Ok(batch) if batch.vectors.len() == content_indices.len() => {
                                    // S-01 completion: the embedding call's
                                    // measured usage flows to the executing
                                    // tool's envelope.
                                    hkask_mcp_server::server::record_tool_usage(
                                        batch.usage.clone(),
                                    );
                                    Some(content_indices.into_iter().zip(batch.vectors).collect())
                                }
                                Ok(batch) => {
                                    duplication_reason = Some(format!(
                                        "embedding tier returned {} vectors for {} \
                                         content-bearing artifacts — the deterministic \
                                         shingle floor ran instead",
                                        batch.vectors.len(),
                                        content_indices.len()
                                    ));
                                    None
                                }
                                Err(error) => {
                                    duplication_reason = Some(format!(
                                        "embedding tier failed ({error}) — the \
                                         deterministic shingle floor ran instead"
                                    ));
                                    None
                                }
                            }
                        }
                    }
                }
                _ => None,
            };

            // Deterministic signal computation (not LLM relay — G3 contract).
            // Scoring lives in `score_evidence_set` (pure, unit-tested).
            let scored =
                score_evidence_set(&artifacts, &DEFAULT_PROFILE, semantic_vectors.as_deref());

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

            let mut set_block = serde_json::json!({
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
            });
            if let Some(reason) = duplication_reason {
                set_block["duplication_reason"] = serde_json::json!(reason);
            }

            let mut result = serde_json::json!({
                "question": question,
                "average_confidence": (average_confidence * 100.0).round() / 100.0,
                "artifacts": artifact_reports,
                "set": set_block,
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
        description = "Format sources as an APA 7 reference list. Each source needs a title and a url or doi; authors, published, site_name, the journal fields and retrieved are optional. Deterministic formatting, no model call. What the tool applies: authors inverted with initials (an author given as a string is a group name and is kept as written), '&' before the last of 2 to 20 authors and the first 19 plus the last for 21 or more; the title in the author position when no author is given; '(n.d.)' when no date is given; the site name left out when it is the same as a single group author; a DOI written as https://doi.org/…; 'Retrieved Month D, YYYY, from' only when `retrieved` is given; entries alphabetized by first author, earliest first, with letter suffixes for the same author and year. What it does not do: it does not re-case a title (APA wants sentence case for page and article titles, and that needs knowledge of proper nouns) and it does not look anything up. Italics are markdown asterisks. A `published` value that is not YYYY, YYYY-MM, YYYY-MM-DD or an RFC 3339 timestamp is refused, naming the source: a search result's 'published' can be '4 days ago' or 'January 13, 2026' and must be converted first. Returns {references: [{source_index, reference}], count, notes}; `source_index` is the position in the request, since the list is reordered."
    )]
    pub async fn cite_sources(
        &self,
        Parameters(CiteSourcesRequest { sources }): Parameters<CiteSourcesRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "cite_sources", async {
            if sources.is_empty() {
                return Err(McpToolError::invalid_argument("sources must not be empty"));
            }
            let list = crate::research::apa::reference_list(&sources)
                .map_err(McpToolError::invalid_argument)?;
            Ok(serde_json::json!({
                "count": list.references.len(),
                "references": list.references,
                "notes": list.notes,
            }))
        })
        .await
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
                // DB path is `{kask_data_dir}/mcp/research/research.db`.
                // The DB is plaintext SQLite by recorded design decision
                // (operator ruling 2026-09-29, F-P1): MCP-server data stays
                // unencrypted so this server runs in an independent
                // MCP-server pattern without the zed keychain or
                // HKASK_DB_PASSPHRASE.
                // D28 path resolution through the shared helper — the
                // env override (HKASK_RESEARCH_DB), the per-agent default,
                // the parent self-heal and the warn/info logging are one
                // implementation (the research DB is plaintext by design;
                // only its path resolution is shared).
                let research_db_path = hkask_mcp_server::resolve_server_db_path(
                    "HKASK_RESEARCH_DB",
                    &hkask_types::agent_paths::mcp_server_db("research", "research"),
                );

                // Resolve passphrase via the canonical 2-tier chain
                // (ctx.credentials → resolve_credential which does env → keychain).
                let passphrase = match resolve_db_passphrase(&ctx.credentials) {
                    Ok(passphrase) => Some(passphrase),
                    Err(error) => {
                        tracing::warn!(
                            target = "hkask.research.init",
                            %error,
                            "Falling back to no research database. Research-run tools will be unavailable."
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
                                         research-run tools will be unavailable"
                                    );
                                    e
                                })
                                .ok(),
                            Err(e) => {
                                tracing::warn!(
                                    target = "hkask.research.init",
                                    error = %e,
                                    path = %research_db_path,
                                    "Failed to open research database — research-run tools will be \
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
                inference_port.clone(),
                // The visible settings chain, resolved once at the
                // construction seam (no hidden constant — the operator's
                // no-hidden-models spec).
                hkask_inference::model_constants::rerank_model(),
                // The embedding model for the semantic duplication tier —
                // same construction-seam resolution; unset is a legitimate
                // degraded mode (the shingle floor exists).
                hkask_inference::model_constants::embedding_model(),
                // zed-kask: D100 — the private leg's local embedding model
                // for private-provenance duplication-tier calls (the same
                // construction-seam discipline; unset/empty is the
                // operator's explicit opt-out → the surfaced shingle-floor
                // degrade).
                hkask_inference::model_constants::private_embedding_model(),
                // zed-kask: D100 §8.1 routed shape — the private leg's
                // local rerank model for private-provenance deep-search
                // rerank (the same construction-seam discipline; unset/
                // empty is the operator's explicit opt-out → the surfaced
                // heuristic skip).
                hkask_inference::model_constants::private_rerank_model(),
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
            "HKASK_SEMANTIC_SCHOLAR_API_KEY",
            "Semantic Scholar API key (optional — the provider runs keyless on the free tier's shared ~1 req/s limit; the free key from semanticscholar.org/product/api lifts it)",
        ),
        hkask_mcp_server::db_passphrase_requirement_optional_with(
            "SQLCipher passphrase for the research DB — research-run tools are unavailable without it (the DB opens at its default path; HKASK_RESEARCH_DB only overrides the location)",
        ),
    ]
}

// Pins the registered tool-surface count end-to-end against the router the
// `#[tool_router(server_handler)]` block generates. Adding or removing a
// research tool is an intentional surface change — this pin catches
// accidental drift (a dropped `#[tool]` attribute, a registration change)
// instead of shipping as an undocumented surface change. Mirrors
// the shared `tool_surface_pin!` macro every built-in server carries.
#[cfg(test)]
mod tool_surface_tests {
    use super::{ResearchServer, validate_extraction};

    /// expect: "A JSON request cannot succeed with Markdown, raw PDF, or invalid JSON."
    #[test]
    fn extraction_contract_rejects_wrong_format_and_binary() {
        assert!(validate_extraction("json", Some("markdown"), Some("some text")).is_err());
        assert!(validate_extraction("json", Some("json"), Some("%PDF-1.6 raw bytes")).is_err());
        assert!(validate_extraction("json", Some("json"), Some("not JSON")).is_err());
        assert!(validate_extraction("json", Some("json"), Some("{\"title\":\"ok\"}")).is_ok());
        assert!(validate_extraction("markdown", Some("markdown"), Some("ordinary text")).is_ok());
    }

    /// expect: unsupported strategy and duplication values are not advertised
    /// by the actual MCP tool schemas; optional null remains accepted.
    #[test]
    fn registered_research_tools_constrain_optional_input_values() {
        let tools = ResearchServer::tool_router().list_all();
        for (name, field, values) in [
            (
                "web_search",
                "strategy",
                serde_json::json!(["quick", "web", "semantic", "news", "deep", "research"]),
            ),
            (
                "evaluate_evidence",
                "duplication",
                serde_json::json!(["semantic"]),
            ),
            (
                "web_search",
                "intent",
                serde_json::json!([
                    "news",
                    "academic",
                    "semantic",
                    "research",
                    "freshness",
                    "general",
                    "transcript"
                ]),
            ),
            (
                "web_search",
                "freshness",
                serde_json::json!(["day", "week", "month", "year"]),
            ),
            (
                "web_extract",
                "format",
                serde_json::json!(["markdown", "json"]),
            ),
        ] {
            let tool = tools
                .iter()
                .find(|tool| tool.name == name)
                .expect("tool registered");
            let schema = serde_json::to_value(&tool.input_schema).expect("tool input schema");
            assert_eq!(schema["properties"][field]["anyOf"][0]["enum"], values);
            assert_eq!(schema["properties"][field]["anyOf"][1]["type"], "null");
        }
        // Required closed-vocabulary fields carry the enum directly (no
        // null arm), read from the constant the handler validates against.
        for (name, field, values) in [
            (
                "finish_research_run",
                "status",
                crate::research::runs::FINISH_STATUSES,
            ),
            (
                "annotate_research_run",
                "verification_state",
                crate::research::runs::VERIFICATION_STATES,
            ),
        ] {
            let tool = tools
                .iter()
                .find(|tool| tool.name == name)
                .expect("tool registered");
            let schema = serde_json::to_value(&tool.input_schema).expect("tool input schema");
            assert_eq!(
                schema["properties"][field]["enum"],
                serde_json::json!(values),
                "{name}.{field}"
            );
        }
    }

    /// expect: every tool that takes arguments publishes a closed schema
    /// (`additionalProperties: false`), so a client can see before calling
    /// that an unknown argument is refused. `web_ping` takes none.
    #[test]
    fn every_tool_schema_is_closed_to_unknown_arguments() {
        for tool in ResearchServer::tool_router().list_all() {
            if tool.name == "web_ping" {
                continue;
            }
            let schema = serde_json::to_value(&tool.input_schema).expect("tool input schema");
            assert_eq!(
                schema["additionalProperties"],
                serde_json::json!(false),
                "{} must refuse unknown arguments in its schema",
                tool.name
            );
        }
    }

    include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));
    hkask_mcp_server::tool_surface_pin!(
        ResearchServer::tool_router(),
        "tool_router",
        11,
        tool_surface_is_exactly_11_registered_tools,
    );

    /// expect: the tools removed on 2026-11-12 (operator decisions) are not
    /// served: no `rss_` feed tool, and no `web_browse`.
    #[test]
    fn no_removed_tool_is_registered() {
        let removed: Vec<String> = ResearchServer::tool_router()
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .filter(|name| name.starts_with("rss_") || name == "web_browse")
            .collect();
        assert!(
            removed.is_empty(),
            "removed tools are registered: {removed:?}"
        );
    }

    /// expect: every registered tool's live input schema is strict — no
    /// bare boolean in any schema-valued position — so strict-schema-decoding
    /// providers (Ollama, Gemini) never reject the whole chat completion.
    #[test]
    fn live_router_schemas_are_strict_for_all_registered_tools() {
        let tools = ResearchServer::tool_router().list_all();
        assert_eq!(
            tools.len(),
            11,
            "approved surface: 11 registered tools (pinned by tool_surface_is_exactly_11_registered_tools)"
        );
        for tool in tools {
            let schema =
                serde_json::to_value(&tool.input_schema).expect("router schema serializes");
            let violations = hkask_types::find_boolean_schema_positions(&schema);
            assert!(
                violations.is_empty(),
                "{} live schema contains bare-boolean positions: {violations:?}",
                tool.name
            );
        }
    }
}

/// Source pin (the D31 register style): the response-cache write is
/// provenance-gated — a private call's deep-search output (reordered by
/// the private leg's local rerank) must never populate the shared cache
/// (dual-axis review Q4; operator ruling 2026-11-07).
#[cfg(test)]
mod provenance_gate_tests {
    #[test]
    fn the_cache_write_is_provenance_gated() {
        let source = include_str!("hkask_mcp_research.rs");
        let production = source
            .split("mod tool_surface_tests")
            .next()
            .unwrap_or(source);
        assert!(
            production.contains("current_call_provenance()"),
            "the cache write must read the call provenance"
        );
        assert!(
            production.contains("&& !private_call"),
            "the cache write must be gated on a public call context"
        );
    }
}
