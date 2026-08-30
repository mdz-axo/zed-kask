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
    BrowseOutput, BrowseRequest, CiteSourcesRequest, CiteStyle, Continuation,
    DEFAULT_CACHE_MAX_ENTRIES, DEFAULT_CACHE_TTL_SECS, DeleteSyntheticRequest, DiscoverRequest,
    EditTagRequest, EvaluateEvidenceRequest, ExtractOptions, ExtractOutput, ExtractRequest,
    FetchRequest, FetchSyntheticRequest, FindSimilarOutput, FindSimilarRequest,
    FindSimilarResultOutput, GetEntriesRequest, ImportOpmlRequest, ListSubscriptionsRequest,
    MAX_CACHE_MAX_ENTRIES, MAX_CACHE_TTL_SECS, MAX_INSTRUCTION_LENGTH, MAX_JSON_PROMPT_LENGTH,
    MAX_JSON_SCHEMA_BYTES, MAX_QUERY_LENGTH, MAX_URL_LENGTH, MarkReadRequest, PingOutput,
    ProviderProfileOutput, RateLimiter, RecommendProviderOutput, RecommendProviderRequest,
    RerankInfo, ResponseCache, SearchMetadata, SearchOutput, SearchQuery, SearchRequest,
    SearchResultOutput, SearchStrategy, SubscribeRequest, SynthesizeRequest, UnreadCountRequest,
    UnsubscribeRequest, WebSearchPort, build_provider_pool, cache_key, discover_feeds, fetch_feed,
    llm_rerank, provider_profile,
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
        pub rss_db: Option<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>>,
        pub rss_client: Client,
        /// Inference port for the deep strategy's templated LLM rerank.
        /// Resolved via `hkask_inference::resolve_inference_port()` — a
        /// `LazyInferencePort` that bridges to zed's LanguageModelRegistry
        /// over `HKASK_INFERENCE_SOCKET` on each call.
        pub inference_port: Arc<dyn hkask_types::InferencePort>,
    }
);

// ── RSS helpers ──

pub(crate) fn spawn_db<F, T>(
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
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

/// Require RSS database, returning an Err if not configured.
macro_rules! require_rss_db {
    ($self:expr) => {
        match &$self.rss_db {
            Some(db) => db.clone(),
            None => {
                return Err(McpToolError::permission_denied(
                    "RSS database not configured. Set HKASK_RSS_DB and HKASK_DB_PASSPHRASE.",
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
         firecrawl, serpapi) — no fusion, no fallback. Use web_recommend_provider \
         to pick deliberately. When `provider` is None, `strategy` selects: \
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
                }),
                &fingerprint,
            );

            if let Some(cached) = self.cache.get(&ckey).await {
                return Ok(cached);
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
                .search(&search_query, strat, req.provider.as_deref())
                .await
                .map_err(McpToolError::from)?;

            compound.results.truncate(num_results as usize);

            // Deep strategy rerank stage: ONE templated rerank call
            // carrying all candidates as documents, routed through the
            // inference IPC bridge to the provider's rerank endpoint
            // (default `OpenRouter/qwen/qwen3-reranker-8b`, override via
            // `HKASK_RERANK_MODEL`). Every degraded outcome (call failed,
            // or documents missing from the response) is surfaced in
            // `rerank` — never a silent fallback.
            let rerank = if strat == SearchStrategy::Deep && compound.results.len() >= 2 {
                let outcome = llm_rerank(
                    self.inference_port.as_ref(),
                    &req.query,
                    &mut compound.results,
                )
                .await;
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
            // provider was selected (explicit override or quick strategy).
            let selected_provider = if req.provider.is_some() || strat == SearchStrategy::Quick {
                compound
                    .providers_succeeded
                    .first()
                    .cloned()
                    .or_else(|| req.provider.clone())
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

            Ok(output)
        })
        .await
    }

    #[tool(
        description = "Recommend a web search provider for a query. Scores each \
         configured provider (tavily, brave, exa, firecrawl, serpapi) against the \
         query + optional intent (news, academic, semantic, freshness, general, \
         transcript) using cost, latency, strengths/weaknesses, and capability match. \
         Returns ranked recommendations with rationale. Call this before web_search \
         when unsure which provider fits — then set the `provider` field on \
         web_search to the recommended kind for a deliberate single-provider call."
    )]
    pub async fn web_recommend_provider(
        &self,
        Parameters(req): Parameters<RecommendProviderRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "web_recommend_provider", async {
            self.rate_limiter.check("web_recommend_provider")?;

            if req.query.is_empty() {
                return Err(McpToolError::invalid_argument("query must not be empty"));
            }
            if req.query.len() > MAX_QUERY_LENGTH {
                return Err(McpToolError::invalid_argument(format!(
                    "query exceeds maximum length of {} characters",
                    MAX_QUERY_LENGTH
                )));
            }

            let recommendations = self.pool.score_providers(&req.query, req.intent.as_deref());
            let recommended = recommendations
                .iter()
                .find(|r| r.configured)
                .map(|r| r.kind.clone());

            let output = RecommendProviderOutput {
                query: req.query,
                intent: req.intent,
                recommendations,
                recommended,
            };

            Ok(serde_json::to_value(&output)
                .unwrap_or_else(|_| serde_json::json!({ "error": "serialization failed" })))
        })
        .await
    }

    #[tool(description = "Find pages similar to a given URL using Exa findSimilar")]
    pub async fn web_find_similar(
        &self,
        Parameters(FindSimilarRequest { url, num_results }): Parameters<FindSimilarRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "web_find_similar", async {
            self.rate_limiter.check("web_find_similar")?;

            validate_tool_url_with_dns(&url).await?;

            let num = num_results.unwrap_or(5).min(20);

            // Not cached: web_find_similar skips the response cache (unlike
            // web_search and web_extract). A find-similar result is sensitive
            // to the source URL's evolving neighbourhood and stale quickly.
            tracing::debug!(target: "hkask.web", "web_find_similar cache miss (not cached)");

            self.pool
                .find_similar(&url, num)
                .await
                .map(|output| {
                    let results: Vec<FindSimilarResultOutput> = output
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
                                semantic_score: output.semantic_scores.get(&key).copied(),
                                content_preview: output.content_previews.get(&key).cloned(),
                            }
                        })
                        .collect();

                    let fs_output = FindSimilarOutput {
                        source_url: url,
                        count: results.len(),
                        results,
                    };

                    serde_json::to_value(&fs_output)
                        .unwrap_or_else(|_| serde_json::json!({ "error": "serialization failed" }))
                })
                .map_err(McpToolError::from)
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

            let fmt = format.unwrap_or_else(|| "markdown".to_string());
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

            if let Some(cached) = self.cache.get(&ckey).await {
                return Ok(cached);
            }

            let json_result = self
                .pool
                .extract(&url, &opts)
                .await
                .map(|result| {
                    let output = ExtractOutput {
                        url: result.url,
                        format: result.format,
                        content: result.content,
                        metadata: result.metadata,
                    };
                    serde_json::to_value(&output)
                        .unwrap_or_else(|_| serde_json::json!({ "error": "serialization failed" }))
                })
                .map_err(McpToolError::from);

            if let Ok(ref json) = json_result {
                self.cache.insert(ckey, json.clone()).await;
            }

            json_result
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
            let db = require_rss_db!(self);

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
            let db = require_rss_db!(self);

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
            let db = require_rss_db!(self);
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
            let db = require_rss_db!(self);
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

            let db = require_rss_db!(self);
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
            let db = require_rss_db!(self);
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
            let db = require_rss_db!(self);
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
            let db = require_rss_db!(self);
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
            let db = require_rss_db!(self);
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
            let db = require_rss_db!(self);
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
            let db = require_rss_db!(self);
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
            match discover_feeds(&self.rss_client, &url).await {
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
            let db = require_rss_db!(self);
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
            let db = require_rss_db!(self);

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

    #[tool(
        description = "Re-extract from a synthetic feed's source URL and insert new entries. Called by rss_fetch for synthetic feeds, or directly to refresh a synthetic feed."
    )]
    pub async fn rss_fetch_synthetic(
        &self,
        Parameters(req): Parameters<FetchSyntheticRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "rss_fetch_synthetic", async {
            self.rate_limiter.check("rss_fetch_synthetic")?;
            let db = require_rss_db!(self);

            // Resolve the feed URL from the stream_id.
            let sid = req.stream_id.clone();
            let lookup = spawn_db(db, move |conn| resolve_feed_with_headers(conn, &sid)).await;
            let (feed_url, _etag, _lm) = match lookup {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => return Err(McpToolError::not_found(e.to_string())),
                Err(e) => return Err(map_join_error(e, "rss fetch task failed")),
            };

            // Check if this is a synthetic feed.
            if !feed_url.starts_with("synthetic://") {
                return Err(McpToolError::invalid_argument(
                    "not a synthetic feed; use rss_fetch instead",
                ));
            }

            self.fetch_synthetic_inner(&req.stream_id, feed_url).await
        })
        .await
    }

    /// Shared synthetic feed fetch logic. Called by both `rss_fetch` (when it
    /// detects a `synthetic://` URL) and `rss_fetch_synthetic` (direct call).
    /// The `feed_url` must already be resolved and start with `synthetic://`.
    async fn fetch_synthetic_inner(
        &self,
        stream_id: &str,
        feed_url: String,
    ) -> Result<serde_json::Value, McpToolError> {
        let db = require_rss_db!(self);

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
            let db = require_rss_db!(self);
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
                let db = require_rss_db!(self);

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

    // ═══════════════════ Evidence evaluation ═══════════════════

    #[tool(
        description = "Evaluate retrieved evidence against a research question. Scores each artifact on recency, source credibility, corroboration, and counter-evidence. Emits SEPIO-anchored confidence and corroboration links. Use after web_search/web_extract to assess evidence quality before synthesis."
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
            // Corroboration: count artifacts sharing the same source domain.
            let mut source_counts: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            for a in &artifacts {
                if let Some(src) = &a.source {
                    *source_counts.entry(src.as_str()).or_default() += 1;
                }
            }

            let evaluations: Vec<serde_json::Value> = artifacts
                .iter()
                .map(|a| {
                    let corroboration = a
                        .source
                        .as_deref()
                        .and_then(|s| source_counts.get(s))
                        .copied()
                        .unwrap_or(1);
                    // Recency: presence of a published date is a positive signal.
                    let has_date = a.published.is_some();
                    // Confidence: deterministic composite — corroboration +
                    // recency + has_content. Not an LLM score.
                    let has_content = a.content.is_some();
                    let confidence = (corroboration as f64 * 0.3)
                        + (if has_date { 0.2 } else { 0.0 })
                        + (if has_content { 0.2 } else { 0.0 })
                        + 0.3; // base confidence
                    let confidence = confidence.min(1.0);

                    serde_json::json!({
                        "url": a.url,
                        "title": a.title,
                        "confidence": (confidence * 100.0).round() / 100.0,
                        "corroboration_count": corroboration,
                        "has_published_date": has_date,
                        "has_content": has_content,
                        "SEPIO:0000167": format!("{:.2}", confidence),
                        "SEPIO:0000440": if corroboration > 1 {
                            serde_json::Value::String(format!(
                                "{} independent sources on same domain",
                                corroboration
                            ))
                        } else {
                            serde_json::Value::Null
                        },
                    })
                })
                .collect();

            // Overall assessment: the question's evidence base.
            let total = evaluations.len();
            let avg_confidence: f64 = if total > 0 {
                evaluations
                    .iter()
                    .filter_map(|e| e.get("confidence").and_then(|c| c.as_f64()))
                    .sum::<f64>()
                    / total as f64
            } else {
                0.0
            };

            let mut result = serde_json::json!({
                "question": question,
                "artifacts_evaluated": total,
                "average_confidence": (avg_confidence * 100.0).round() / 100.0,
                "evaluations": evaluations,
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

            let rss_db = {
                // Databases live in the internal data dir (the ONLY thing that
                // lives there — artifact files and outputs go to the visible
                // artifacts dir under {server}-mcp/{artifact-type}/). Default
                // DB path is `{kask_data_dir}/mcp/research/rss.db`, resolved
                // via `resolve_under_data_dir`. Override via `HKASK_RSS_DB`.
                let rss_db_path = std::env::var("HKASK_RSS_DB").ok().unwrap_or_else(|| {
                    let default_path = hkask_types::agent_paths::resolve_under_data_dir(
                        &hkask_types::agent_paths::mcp_server_db("research", "rss"),
                    );
                    if let Some(parent) = default_path.parent() {
                        if let Err(error) = std::fs::create_dir_all(parent) {
                            tracing::warn!(
                                target: "hkask.research.init",
                                path = %default_path.display(),
                                %error,
                                "Failed to create default RSS DB directory \
                                 — the subsequent DB open will surface the failure"
                            );
                        }
                    }
                    tracing::info!(
                        target: "hkask.research.init",
                        path = %default_path.display(),
                        "Using default RSS database path (HKASK_RSS_DB not set)"
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
                            "Falling back to no RSS database. RSS tools will be unavailable."
                        );
                        None
                    }
                };

                match passphrase {
                    Some(passphrase) => {
                        match hkask_storage::Database::open_with_extensions(
                            &rss_db_path,
                            &passphrase,
                            db::RSS_SCHEMA_DDL,
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
                                        path = %rss_db_path,
                                        "RSS database opened but pool extraction failed — \
                                         RSS tools will be unavailable"
                                    );
                                    e
                                })
                                .ok(),
                            Err(e) => {
                                tracing::warn!(
                                    target = "hkask.research.init",
                                    error = %e,
                                    path = %rss_db_path,
                                    "Failed to open RSS database — RSS tools will be \
                                     unavailable. Check HKASK_RSS_DB path and \
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
                rss_db,
                rss_client,
                inference_port.clone(),
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
            "Passphrase for SQLCipher encryption (required if HKASK_RSS_DB is set)",
        ),
    ]
}
