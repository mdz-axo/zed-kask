#![forbid(unsafe_code)]
#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

pub mod research;

// Re-export service crate modules for test compatibility
pub use crate::research::{cache, db, providers, rss_types, strip_html, types};

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use hkask_mcp_server::server::{
    CredentialRequirement, McpToolError, ServerContext, execute_tool, validate_tool_url,
};
use hkask_types::time::now_rfc3339;
use reqwest::Client;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use rusqlite::Connection;

use crate::research::db::*;
use crate::research::{
    BrowseOutput, BrowseRequest, Continuation, DEFAULT_CACHE_MAX_ENTRIES, DEFAULT_CACHE_TTL_SECS,
    DiscoverRequest, EditTagRequest, ExtractOptions, ExtractOutput, ExtractRequest, FetchRequest,
    FindSimilarOutput, FindSimilarRequest, FindSimilarResultOutput, GetEntriesRequest,
    ImportOpmlRequest, ListSubscriptionsRequest, MAX_CACHE_MAX_ENTRIES, MAX_CACHE_TTL_SECS,
    MAX_INSTRUCTION_LENGTH, MAX_JSON_PROMPT_LENGTH, MAX_JSON_SCHEMA_BYTES, MAX_QUERY_LENGTH,
    MAX_URL_LENGTH, MarkReadRequest, PingOutput, RateLimiter, ResponseCache, SearchMetadata,
    SearchOutput, SearchQuery, SearchRequest, SearchResultOutput, SearchStrategy, SubscribeRequest,
    UnreadCountRequest, UnsubscribeRequest, WebSearchPort, build_provider_pool, cache_key,
    discover_feeds, fetch_feed,
};

// ── Constants ──

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_PAGE_SIZE: usize = 20;
const MAX_PAGE_SIZE: usize = 100;
const RATE_LIMIT_MAX_REQUESTS: u32 = 30;
const RATE_LIMIT_WINDOW_SECS: u64 = 60;

// ── ResearchServer ──

hkask_mcp_server::mcp_server!(
    pub struct ResearchServer {
        pub pool: Arc<dyn WebSearchPort>,
        pub cache: Arc<ResponseCache>,
        pub rate_limiter: RateLimiter,
        pub rss_db: Option<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>>,
        pub rss_client: Client,
    }
);

impl ResearchServer {
    /// Record a tool call as a narrative experience in the agent's memory.
    ///
    /// Generates a literal chat stream entry and sends it to the daemon for
    /// dual encoding (episodic + semantic). Fire-and-forget — failures are
    /// logged but never block the tool response.
    pub fn record_experience(
        &self,
        tool: &str,
        input_summary: &str,
        outcome: &str,
        detail: serde_json::Value,
    ) {
        if let Some(ref daemon) = self.daemon {
            let value = serde_json::json!({
                "tool": tool,
                "input": input_summary,
                "outcome": outcome,
                "detail": detail,
                "timestamp": now_rfc3339(),
            });
            let daemon_clone = daemon.clone();
            let userpod = self.userpod.clone();
            let tool_name = tool.to_string();
            tokio::spawn(async move {
                match daemon_clone
                    .store_experience(&userpod, "mcp_session", "observed", &value, Some(0.85))
                    .await
                {
                    Ok(hkask_mcp_server::DaemonResponse::StoreResponse {
                        stored: true, ..
                    }) => {
                        tracing::debug!(
                            target: "hkask.mcp.research.memory",
                            tool = %tool_name,
                            "Experience stored via daemon"
                        );
                    }
                    Ok(other) => {
                        tracing::warn!(
                            target: "hkask.mcp.research.memory",
                            tool = %tool_name,
                            response = ?other,
                            "Unexpected daemon response for store_experience"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "hkask.mcp.research.memory",
                            tool = %tool_name,
                            error = %e,
                            "Failed to store experience via daemon"
                        );
                    }
                }
            });
        }
    }
}

// ── RSS helpers ──

pub fn spawn_db<F, T>(
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
            Ok(Err(e)) => Err(McpToolError::internal(e.to_string())),
            Err(e) => Err(McpToolError::internal(format!("Task error: {}", e))),
        }
    };
}

/// Require RSS database, returning an Err if not configured.
macro_rules! require_rss_db {
    ($self:expr) => {
        match &$self.rss_db {
            Some(db) => db.clone(),
            None => {
                return Err(McpToolError::unavailable(
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
    pub async fn web_ping(&self) -> String {
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
            Ok(serde_json::to_value(&output).expect("PingOutput serialization is infallible"))
        })
        .await
    }

    #[tool(
        description = "Search the web with RRF fusion across providers. Strategy selects providers: quick (single keyword), web (all), news (news-capable), deep (all + 2x results + content extraction on top results)"
    )]
    pub async fn web_search(&self, Parameters(req): Parameters<SearchRequest>) -> String {
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

            let strat = req
                .strategy
                .as_deref()
                .and_then(|s| s.parse::<SearchStrategy>().ok())
                .unwrap_or(SearchStrategy::Quick);

            let num_results = req.num_results.unwrap_or(10).min(50);

            let freshness = req
                .freshness
                .as_deref()
                .and_then(|f| f.parse::<crate::research::types::Freshness>().ok());

            let fingerprint = self.pool.provider_fingerprint();
            let ckey = cache_key(
                &strat.to_string(),
                &req.query,
                &serde_json::json!({ "num_results": num_results, "freshness": freshness }),
                &fingerprint,
            );

            if let Some(cached) = self.cache.get(&ckey).await {
                self.record_experience(
                    "web_search",
                    &req.query,
                    "cache_hit",
                    serde_json::json!({"results": "served from cache"}),
                );
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
                .search(&search_query, strat)
                .await
                .map_err(McpToolError::from)?;

            compound.results.truncate(num_results as usize);

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
            };

            let metadata = SearchMetadata::from(&compound);
            tracing::info!(
                target: "hkask.web",
                strategy = %metadata.strategy,
                providers_queried = ?metadata.providers_queried,
                providers_succeeded = ?metadata.providers_succeeded,
                providers_failed = ?metadata.providers_failed,
                total_before_dedup = metadata.total_before_dedup,
                duplicates_removed = metadata.duplicates_removed,
                top_rrf_scores = ?metadata.top_rrf_scores,
                "Regulation web_search metadata"
            );

            let output = serde_json::to_value(&search_output)
                .unwrap_or_else(|_| serde_json::json!({ "error": "serialization failed" }));

            self.cache.insert(ckey, output.clone()).await;

            self.record_experience(
                "web_search",
                &req.query,
                "success",
                serde_json::json!({
                    "results_count": search_output.count,
                    "strategy": search_output.strategy,
                    "top_result": search_output.results.first().map(|r| r.title.clone()),
                }),
            );

            Ok(output)
        })
        .await
    }

    #[tool(description = "Find pages similar to a given URL using Exa findSimilar")]
    pub async fn web_find_similar(
        &self,
        Parameters(FindSimilarRequest { url, num_results }): Parameters<FindSimilarRequest>,
    ) -> String {
        execute_tool(self, "web_find_similar", async {
            self.rate_limiter.check("web_find_similar")?;

            validate_tool_url(&url)?;

            let num = num_results.unwrap_or(5).min(20);

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
    ) -> String {
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

            validate_tool_url(&url)?;

            let fmt = format.unwrap_or_else(|| "markdown".to_string());
            let opts = ExtractOptions {
                format: fmt.clone(),
                json_prompt,
                json_schema,
                main_content_only: main_content_only.unwrap_or(true),
                wait_for_ms: wait_for_ms.unwrap_or(0),
            };

            let fingerprint = self.pool.provider_fingerprint();
            let cache_params =
                serde_json::json!({ "format": fmt, "main_content_only": opts.main_content_only });
            let ckey = cache_key("extract", &url, &cache_params, &fingerprint);

            if let Some(cached) = self.cache.get(&ckey).await {
                self.record_experience(
                    "web_extract",
                    &url,
                    "cache_hit",
                    serde_json::json!({"format": fmt}),
                );
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

            self.record_experience(
                "web_extract",
                &url,
                if json_result.is_ok() {
                    "success"
                } else {
                    "error"
                },
                serde_json::json!({"format": fmt}),
            );

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
    ) -> String {
        execute_tool(self, "web_browse", async {
            self.rate_limiter.check("web_browse")?;

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

            validate_tool_url(&url)?;

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
    ) -> String {
        execute_tool(self, "rss_subscribe", async {
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
    ) -> String {
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
    ) -> String {
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
    ) -> String {
        execute_tool(self, "rss_fetch", async {
            let db = require_rss_db!(self);
            let sid = stream_id.clone();
            let lookup = spawn_db(db, move |conn| resolve_feed_with_headers(conn, &sid)).await;

            let (feed_url, cached_etag, cached_lm) = match lookup {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => {
                    return Err(McpToolError::not_found(e.to_string()));
                }
                Err(e) => {
                    return Err(McpToolError::internal(format!("Task error: {}", e)));
                }
            };

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
    ) -> String {
        execute_tool(self, "rss_get_entries", async {
            let db = require_rss_db!(self);
            let limit = (count.unwrap_or(DEFAULT_PAGE_SIZE as u32) as usize).min(MAX_PAGE_SIZE);
            let offset = continuation_token
                .as_ref()
                .and_then(|t| {
                    let bytes = base64::engine::general_purpose::STANDARD.decode(t).ok()?;
                    serde_json::from_slice::<Continuation>(&bytes).ok()
                })
                .map(|c| c.offset)
                .unwrap_or(0);

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
    ) -> String {
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
    ) -> String {
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
    ) -> String {
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
    pub async fn rss_export_opml(&self) -> String {
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
    ) -> String {
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
    ) -> String {
        execute_tool(self, "rss_discover_feeds", async {
            validate_tool_url(&url)?;
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
    pub async fn rss_edit_tag(&self, Parameters(req): Parameters<EditTagRequest>) -> String {
        execute_tool(self, "rss_edit_tag", async {
            let db = require_rss_db!(self);
            let result = spawn_db(db, move |conn| edit_tags(conn, &req)).await;
            handle_db_result!(result, |v| v)
        })
        .await
    }
}

// ── Entry point ──

/// Run the research MCP server (used by binary target).
pub async fn run(
    userpod: String,
    daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    dotenvy::dotenv().ok();

    let dotenv = hkask_mcp_server::load_dotenv();

    hkask_mcp_server::run_server_with_preloaded(
        "hkask-mcp-research",
        SERVER_VERSION,
        |ctx: ServerContext| {
            let parse_env_u64 =
                |k: &str| ctx.credentials.get(k).and_then(|s| s.parse::<u64>().ok());
            let parse_env_usize =
                |k: &str| ctx.credentials.get(k).and_then(|s| s.parse::<usize>().ok());

            let pool = build_provider_pool(&ctx.credentials).map_err(|e| {
                hkask_mcp_server::McpError::UnexpectedResponse {
                    context: "research server init".into(),
                    detail: e.to_string(),
                }
            })?;

            let cache_ttl = parse_env_u64("HKASK_WEB_CACHE_TTL_SECS")
                .map(|s| s.min(MAX_CACHE_TTL_SECS))
                .unwrap_or(DEFAULT_CACHE_TTL_SECS);
            let cache_max = parse_env_usize("HKASK_WEB_CACHE_MAX_ENTRIES")
                .map(|s| s.min(MAX_CACHE_MAX_ENTRIES))
                .unwrap_or(DEFAULT_CACHE_MAX_ENTRIES);

            let rss_db = ctx
                .open_database_with_extensions("HKASK_RSS_DB", db::RSS_SCHEMA_DDL)
                .ok()
                .and_then(|db| db.sqlite_pool().ok());

            let rss_client = Client::builder()
                .user_agent(format!("hkask-mcp-research/{}", SERVER_VERSION))
                .build()
                .map_err(|e| {
                    hkask_mcp_server::McpError::from(std::io::Error::other(e.to_string()))
                })?;

            Ok(ResearchServer::new(
                ctx.webid,
                userpod.clone(),
                daemon_client.clone(),
                Arc::new(pool),
                Arc::new(ResponseCache::new(
                    cache_max,
                    Duration::from_secs(cache_ttl),
                )),
                RateLimiter::new(RATE_LIMIT_MAX_REQUESTS, RATE_LIMIT_WINDOW_SECS),
                rss_db,
                rss_client,
            ))
        },
        credential_requirements(),
        dotenv,
    )
    .await
}

pub fn credential_requirements() -> Vec<CredentialRequirement> {
    let opt = CredentialRequirement::optional;
    vec![
        opt("HKASK_BRAVE_API_KEY", "Brave Search API key"),
        opt("HKASK_FIRECRAWL_API_KEY", "Firecrawl API key"),
        opt("HKASK_TAVILY_API_KEY", "Tavily API key"),
        opt("HKASK_SERPAPI_API_KEY", "SerpAPI key"),
        opt("HKASK_EXA_API_KEY", "Exa API key"),
        opt("HKASK_BROWSERBASE_API_KEY", "Browserbase API key"),
        opt("HKASK_WEB_CACHE_TTL_SECS", "Cache TTL seconds"),
        opt("HKASK_WEB_CACHE_MAX_ENTRIES", "Max cache entries"),
        opt(
            "HKASK_RSS_DB",
            "Path to the RSS reader SQLite database (RSS tools unavailable if absent)",
        ),
        opt(
            "HKASK_DB_PASSPHRASE",
            "Passphrase for SQLCipher encryption (required if HKASK_RSS_DB is set)",
        ),
    ]
}
