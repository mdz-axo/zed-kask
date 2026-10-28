//! Storage and query tools — cache, passage query, similarity.
use crate::index::{IndexedPassage, Retrieval, RetrievedPassage};
use crate::{
    CorpusServer, LLMParameters, McpToolError, Parameters, execute_tool, json,
    render_docproc_template, tool, tool_router,
};
use hkask_mcp_server::server::{map_io_error, write_contained};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[tool_router(router = storage_tools_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Cache processed document text for reference. Stores content keyed by label in the corpus cache directory (corpus-mcp/cache/ under the visible artifacts dir, ~/Documents/zk-data/)."
    )]
    pub async fn corpus_cache(
        &self,
        Parameters(CacheRequest {
            content,
            label,
            origin_provider,
        }): Parameters<CacheRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_cache", async {
            if content.is_empty() {
                return Err(McpToolError::invalid_argument("content must not be empty"));
            }

            if label.is_empty() {
                return Err(McpToolError::invalid_argument("label must not be empty"));
            }

            // Canonical storage route. Cache directory lives at
            // `{artifacts_dir}/corpus-mcp/cache/` (visible under
            // ~/Documents/zk-data/).
            let cache_dir = hkask_types::agent_paths::resolve_under_artifacts_dir(
                &hkask_types::agent_paths::mcp_artifacts_subdir("corpus", "cache"),
            );

            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                return Err(map_io_error(
                    e,
                    &format!("Failed to create cache directory '{}'", cache_dir.display()),
                ));
            }

            // Sanitize label for filesystem
            let safe_label: String = label
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let cache_target = cache_dir.join(format!("{}.md", safe_label));
            let cache_target_str = cache_target
                .to_str()
                .ok_or_else(|| McpToolError::invalid_argument("cache path is not valid UTF-8"))?;
            let cache_path = write_contained(cache_target_str, content.as_bytes())?;

            // Provenance sidecar (task c1d65229): the originating search
            // provider travels with the cached file so `chunk_directory`
            // can stamp it on chunk rows; Brave-derived content is then
            // excluded from QA export paths that could otherwise become
            // model-training material outside this system (Brave API ToS
            // clause xiii). Fail closed — a lost sidecar breaks the downstream
            // license exclusion.
            let provider = origin_provider
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(provider) = provider {
                let sidecar = cache_dir.join(format!("{}.origin.json", safe_label));
                let sidecar_str = sidecar.to_str().ok_or_else(|| {
                    McpToolError::invalid_argument("sidecar path is not valid UTF-8")
                })?;
                let body = serde_json::to_vec(&json!({
                    "label": &label,
                    "origin_provider": provider,
                }))
                .map_err(|e| McpToolError::internal(format!("Serialize origin sidecar: {e}")))?;
                write_contained(sidecar_str, &body)?;
            }
            let mut result = json!({
                "label": label,
                "path": cache_path.display().to_string(),
                "size_bytes": content.len(),
            });
            if let Some(provider) = provider {
                result[hkask_types::corpus::ORIGIN_PROVIDER_FIELD] = json!(provider);
                result["origin_sidecar"] = json!(
                    cache_dir
                        .join(format!("{}.origin.json", safe_label))
                        .display()
                        .to_string()
                );
            }
            Ok(result)
        })
        .await
    }

    pub fn storage_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        // Replace the generated direct-call route, retaining its exact metadata
        // and direct method API. No second tool or transport layer is installed.
        Self::storage_tools_router()
            .with_route((Self::corpus_query_tool_attr(), Self::corpus_query_routed))
    }

    fn corpus_query_routed(
        &self,
        parameters: Parameters<QueryRequest>,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, McpToolError>> + Send + '_>,
    > {
        Box::pin(async move {
            if !parameters.0.query.trim_start().starts_with("(list") {
                return self.corpus_query(parameters).await;
            }
            #[cfg(test)]
            let parameters = {
                let mut parameters = parameters;
                parameters.0.lisp_probe = context.extensions.get::<Arc<LispQueryProbe>>().cloned();
                if let Some(probe) = &parameters.0.lisp_probe {
                    *probe.context.lock().expect("context") = Some(context.clone());
                }
                parameters
            };
            self.corpus_query_inner(parameters, Some(context)).await
        })
    }

    /// Query the in-memory vector index for top-k relevant passages.
    ///
    /// expect: Answers use retrieved original text even when result text is hidden.
    /// [P8] Motivating: grounded answers independent of output projection.
    /// pre: plain or Lisp query; post: generation receives normalized question and
    /// usable passages, or answer_error reports why grounding was unavailable.
    #[tool(
        description = "Search indexed passages by plain natural-language or Lisp query. db_path queries the named durable database through disk KNN only when the in-memory index is empty; a nonempty index (same-process published passages) ignores db_path. include_text defaults to false and controls only returned results, not grounded answer context. Missing stored text and model-mismatched rows (stored under a different embedding model than the query) are skipped and surfaced with counts; no usable context yields answer_error without generation."
    )]
    pub async fn corpus_query(
        &self,
        parameters: Parameters<QueryRequest>,
    ) -> Result<String, McpToolError> {
        self.corpus_query_inner(parameters, None).await
    }

    async fn corpus_query_inner(
        &self,
        Parameters(QueryRequest {
            query,
            top_k,
            generate_answer,
            include_text,
            min_score,
            db_path,
            #[cfg(test)]
            lisp_probe,
        }): Parameters<QueryRequest>,
        context: Option<rmcp::service::RequestContext<rmcp::RoleServer>>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_query", async {
            if query.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "query must not be empty",
                ));
            }

            // Parse the query string. When it starts with `(list`, it's a
            // Lisp S-expression specifying query options. Otherwise it's a
            // plain natural-language query.
            let (nl_query, k, include_text_flag, min_score_val, gen_answer) =
                if query.trim_start().starts_with("(list") {
                    // The guard precedes spawn: dropping the handler signals even
                    // a queued worker. Interpreter Rc state stays on that worker.
                    let cancellation = Arc::new(AtomicBool::new(false));
                    let _cancel = LispQueryCancellation {
                        flag: cancellation.clone(),
                        #[cfg(test)]
                        probe: lisp_probe.clone(),
                    };
                    let expression = query.clone();
                    let worker = tokio::task::spawn_blocking(move || {
                        #[cfg(test)]
                        if let Some(probe) = &lisp_probe {
                            probe.enter(&cancellation);
                        }
                        let result = parse_lisp_query(&expression, cancellation);
                        #[cfg(test)]
                        if let Some(probe) = &lisp_probe {
                            probe.finished(&result);
                        }
                        result
                    });
                    let joined = async {
                        worker.await.map_err(|error| {
                            McpToolError::internal(format!("Lisp query worker failed: {error}"))
                        })?
                    };
                    // rmcp signals, rather than drops, its detached handler.
                    // Observe that signal only while Lisp is pending; subsequent
                    // inference/retrieval keeps its existing detached lifecycle.
                    if let Some(context) = context {
                        tokio::select! {
                            biased;
                            _ = context.ct.cancelled() => {
                                return Err(McpToolError::unavailable("Lisp query request cancelled"));
                            }
                            result = joined => result?,
                        }
                    } else {
                        joined.await?
                    }
                } else {
                    (
                        query.clone(),
                        top_k.unwrap_or(5).clamp(1, 50),
                        include_text.unwrap_or(false),
                        min_score.unwrap_or(0.0),
                        generate_answer.unwrap_or(false),
                    )
                };

            let k = k.clamp(1, 50);
            // Fail-visible: no configured embedding model is a typed error
            // naming the setting — never a hidden constant.
            let model_name = crate::default_embedding_model().ok_or_else(|| {
                McpToolError::permission_denied(
                    "no embedding model configured — set \\
                     kask.models.embedding_model (injected as \\
                     HKASK_EMBEDDING_MODEL); kask never falls back to a \\
                     hidden code constant",
                )
            })?;

            let query_batch = match self
                .inference_router
                .embed_with_dimensions(
                    &model_name,
                    std::slice::from_ref(&nl_query),
                    Some(crate::embedding_dim() as u32),
                )
                .await
            {
                Ok(batch) => batch,
                Err(e) => {
                    return Err(McpToolError::unavailable(format!(
                        "Query embedding failed: {}",
                        e
                    )));
                }
            };
            let query_embedding = query_batch
                .vectors
                .into_iter()
                .next()
                .unwrap_or_default();
            // S-01 completion: the query embedding call's measured usage
            // flows to the executing tool's envelope.
            hkask_mcp_server::server::record_tool_usage(
                query_batch.usage.clone(),
                query_batch.cost_usd,
            );

            if query_embedding.is_empty() {
                return Err(McpToolError::unavailable(
                    "Query embedding returned empty vector",
                ));
            }

            let ram = self.index.retrieve(
                &query_embedding,
                k,
                min_score_val,
                &model_name,
                query_batch.actual_model.as_deref(),
            )?;
            // A nonempty in-memory index — same-process published passages,
            // ephemeral or durable — owns the query; db_path is ignored
            // (pinned by retrieval_origin_isolation_and_path_aliases). An
            // empty index with a db_path queries the named durable database
            // through the disk KNN, read-only, per query — never by hydrating
            // its every passage into RAM. At zk-ref-open scale (279,730
            // passages, 3.3 GB) hydration held ~1.5 GB of permanent server RSS
            // (measured 2026-10-06) to make repeat scans instant, while the
            // disk path costs one read-only open (~180ms KDF) and one vec0
            // KNN (~3s) per query — noise beside the embed call every query
            // already pays.
            let db_path_ignored = db_path.is_some() && ram.total_indexed > 0;
            let retrieval = match db_path.as_deref() {
                Some(path) if ram.total_indexed == 0 => durable_retrieval(
                    path,
                    &query_embedding,
                    k,
                    min_score_val,
                    &model_name,
                    query_batch.actual_model.as_deref(),
                )?,
                _ => ram,
            };
            let crate::index::Retrieval {
                matches,
                total_indexed,
                missing_text,
                dimension_mismatch,
                model_mismatch,
            } = retrieval;
            let results: Vec<_> = matches.iter().map(|matched| matched.project(include_text_flag)).collect();

            let mut result = json!({
                "query": nl_query,
                "results": results,
                "total_indexed": total_indexed,
            });

            if total_indexed == 0 {
                result["note"] = json!("Index empty. Provide db_path to query a durable corpus database, or embed passages.");
            }
            if db_path_ignored {
                result["note"] = json!(format!(
                    "db_path ignored: the in-memory index owns this query (passages published \
                     this process: {total_indexed}). Call corpus_clear_index before \
                     db_path-directed queries."
                ));
            }
            if missing_text > 0 {
                result["missing_passage_text"] = json!(missing_text);
                result["note"] = json!("Some persisted embeddings have no passage_text (legacy or non-passage rows); omitted from answer context. Re-embed original sources to restore grounding.");
            }
            if dimension_mismatch > 0 {
                result["dimension_mismatch"] = json!(dimension_mismatch);
                result["note"] = json!("Some stored embeddings have a different vector length than the query and were skipped; the configured embedding model (or HKASK_EMBEDDING_DIM) likely changed. Re-embed the corpus to restore retrieval.");
            }
            if model_mismatch > 0 {
                result["model_mismatch"] = json!(model_mismatch);
                result["note"] = json!("Some stored embeddings were embedded under a different model than the query and were skipped; the configured embedding model changed. Re-embed the corpus to restore retrieval.");
            }
            let context = matches.iter().filter_map(|matched| matched.passage.text.as_deref())
                .filter(|text| !text.trim().is_empty()).collect::<Vec<_>>().join("\n\n");
            if gen_answer && context.is_empty() {
                result["answer_error"] = json!("No usable passage text retrieved; cannot generate a grounded answer. Re-embed original sources if persisted passage_text is missing.");
            } else if gen_answer {
                let context = crate::guard_content(&context);

                // C10: Load prompt from registry template, fall back to inline if unavailable
                let mut vars = std::collections::HashMap::new();
                vars.insert("context", context.clone());
                vars.insert("question", nl_query.clone());
                let prompt = render_docproc_template("rag-answer", &vars);
                let prompt = if prompt.is_empty() {
                    format!(
                        "{CONTENT_GUARD_INSTRUCTION}Answer the following question based on the provided context. If the context doesn't contain enough information, say so.\n\n\
                         Context:\n{context}\n\n\
                         Question: {nl_query}\n\n\
                         Answer:",
                        CONTENT_GUARD_INSTRUCTION = crate::CONTENT_GUARD_INSTRUCTION
                    )
                } else {
                    prompt
                };

                let params = LLMParameters {
                    temperature: 0.3,
                    ..Default::default()
                };

                match self.inference_router.generate(&prompt, &params, None).await {
                    Ok(response) => {
                        crate::helpers::record_usage(&response);
                        result["answer"] = json!(response.text);
                        result["answer_tokens"] = json!(response.usage.total_tokens);
                    }
                    Err(e) => {
                        result["answer_error"] = json!(format!("{}", e));
                    }
                }
            }

            Ok(result)
        })
        .await
    }

    #[tool(
        description = "Clear all in-memory passages and cancel pending publications. Does not delete stored DB embeddings. Call before selecting a different DB with corpus_query's db_path — a nonempty in-memory index ignores db_path."
    )]
    pub async fn corpus_clear_index(
        &self,
        Parameters(ClearIndexRequest {}): Parameters<ClearIndexRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_clear_index", async {
            let cleared = self.index.clear()?;
            Ok(json!({"cleared": cleared}))
        })
        .await
    }

    #[tool(
        description = "Purge embeddings and h_mems by entity-ref prefix in the named DB. Invalidates matching warm passages and cancels overlapping pending publications; other DBs and ephemeral passages survive. Deletions are not atomic across entities: partial failures return errors, including h_mem failures after embeddings were removed."
    )]
    pub async fn corpus_purge_qa(
        &self,
        Parameters(req): Parameters<PurgeQaRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_purge_qa", async {
            let passphrase = crate::helpers::resolve_corpus_passphrase()?;
            // Purge targets an existing DB — a typo'd path must refuse,
            // not create an empty DB and report a silent zero-purged
            // no-op (CO-02).
            crate::helpers::ensure_db_exists(&req.db_path)?;
            self.index.purge(&req.db_path, &passphrase, &req.prefix)
        })
        .await
    }
}

// ── Request structs ────────────────────────────────────────────────────────

/// Query a durable corpus database through the disk KNN — the empty-index
/// `db_path` path of `corpus_query`. Read-only and per-query: the same vec0
/// MATCH search the federated recall path runs in production, with the same
/// model gate (mismatched rows excluded and counted, never silently ranked
/// cross-model). Nothing is published into the in-memory index — the database
/// is opened and searched per call, so no corpus-sized RAM cache accrues and
/// switching databases needs no clear between queries.
fn durable_retrieval(
    db_path: &str,
    query: &[f32],
    k: usize,
    min_score: f32,
    query_model: &str,
    query_actual_model: Option<&str>,
) -> Result<Retrieval, McpToolError> {
    // CO-02: a typo'd path must refuse, never leave a stray empty DB behind it.
    crate::helpers::ensure_db_exists(db_path)?;
    let passphrase = crate::helpers::resolve_corpus_passphrase()?;
    let database =
        hkask_storage::Database::open_read_only(db_path, &passphrase).map_err(|error| {
            McpToolError::failed_precondition(format!(
                "Cannot open the corpus database '{db_path}' read-only: {error}"
            ))
        })?;
    let pool = database
        .sqlite_pool()
        .map_err(|error| McpToolError::internal(format!("Corpus database pool failed: {error}")))?;
    let driver: std::sync::Arc<dyn hkask_storage::DatabaseDriver> =
        std::sync::Arc::new(hkask_storage::SqliteDriver::new_labeled(pool, db_path));
    let store = hkask_storage::EmbeddingStore::from_driver(
        std::sync::Arc::clone(&driver),
        crate::embedding_dim(),
    )
    .map_err(|error| McpToolError::internal(format!("Embedding store failed: {error}")))?;
    let total_indexed = store
        .count()
        .map_err(|error| McpToolError::internal(format!("Embedding count failed: {error}")))?;
    let missing_text: i64 = driver
        .query(
            "SELECT COUNT(*) FROM embeddings WHERE passage_text IS NULL OR trim(passage_text) = ''",
            &[],
        )
        .map_err(|error| McpToolError::internal(format!("Missing-text count failed: {error}")))?
        .first()
        .map(|row| row.get_int(0))
        .transpose()
        .map_err(|error| {
            McpToolError::internal(format!("Missing-text count read failed: {error}"))
        })?
        .unwrap_or(0);
    let outcome = store
        .search(query, k, query_model, query_actual_model)
        .map_err(|error| match error {
            hkask_storage::EmbeddingError::DimensionMismatch { expected, actual } => {
                McpToolError::unavailable(format!(
                    "Query embedding has dimension {actual} but the stored embeddings have \
                     dimension {expected}; no stored passage can match. The configured embedding \
                     model or HKASK_EMBEDDING_DIM changed — re-embed the corpus."
                ))
            }
            error => McpToolError::internal(format!("Disk KNN search failed: {error}")),
        })?;
    let mut matches: Vec<RetrievedPassage> = outcome
        .results
        .into_iter()
        .map(|result| RetrievedPassage {
            // vec0 cosine distance: score = 1 − distance, the same similarity
            // the in-memory scan reports (and the federated path's
            // relevance_score).
            score: (1.0 - result.distance) as f32,
            passage: IndexedPassage {
                text: result
                    .embedding
                    .passage_text
                    .filter(|text| !text.trim().is_empty()),
                metadata: json!({"entity_ref": result.embedding.entity_ref}),
                embedding: result.embedding.vector,
                model: result.embedding.model,
            },
        })
        .collect();
    // The vec0 KNN has no threshold parameter; apply the caller's floor
    // post hoc, the same filter the in-memory scan applies.
    if min_score > 0.0 {
        matches.retain(|matched| matched.score >= min_score);
    }
    Ok(Retrieval {
        total_indexed,
        missing_text: usize::try_from(missing_text)
            .map_err(|_| McpToolError::internal("missing-text count exceeds usize"))?,
        // The disk schema fixes one dimension per database (vec0 float[N]);
        // a query of another width fails the search above instead of
        // producing a partial mismatch count.
        dimension_mismatch: 0,
        model_mismatch: outcome.excluded_model_mismatch,
        matches,
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CacheRequest {
    /// Text content to cache.
    pub content: String,
    /// Label/key for the cached entry.
    pub label: String,
    /// Originating search provider for search-derived content (e.g. "brave").
    /// Recorded in a `{stem}.origin.json` sidecar next to the cached file so
    /// chunking can stamp it on chunk rows; Brave-derived content is excluded
    /// from QA export paths that could otherwise become model-training material
    /// outside this system (Brave API ToS clause xiii).
    #[serde(default)]
    pub origin_provider: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct QueryRequest {
    /// Natural language question to search for, OR a Lisp S-expression
    /// specifying query options.
    ///
    /// When the string starts with `(list`, it is parsed as a Lisp
    /// S-expression via `hkask_lisp::eval_sandboxed_with_cancellation`.
    /// The result is an association list (alist) of key-value pairs:
    ///
    /// ```lisp
    /// (list (list "query" "investment philosophy")
    ///       (list "top-k" 5)
    ///       (list "include-text" t)
    ///       (list "min-score" 0.6))
    /// ```
    ///
    /// Supported keys:
    /// - `"query"` (string, required) — the natural language query
    /// - `"top-k"` (int, default 5) — number of top results to return
    /// - `"include-text"` (bool, default false) — include passage text in results
    /// - `"min-score"` (float, default 0.0) — only return matches with score >= this
    /// - `"generate-answer"` (bool, default false) — generate LLM answer from results
    ///
    /// When the string does NOT start with `(list`, it is treated as a
    /// plain natural-language query using the separate option parameters.
    pub query: String,
    /// Number of top results to return (default 5). Ignored when `query`
    /// is a Lisp S-expression (use `"top-k"` in the expression instead).
    #[serde(default)]
    pub top_k: Option<usize>,
    /// If true, generate an LLM-augmented answer from retrieved passages.
    /// Ignored when `query` is a Lisp S-expression.
    #[serde(default)]
    pub generate_answer: Option<bool>,
    /// Include passage text in returned results (default false). Answer generation
    /// always retains usable text internally. For Lisp use `"include-text"`.
    #[serde(default)]
    pub include_text: Option<bool>,
    /// Minimum score threshold (0.0–1.0). Only return matches with score
    /// >= this value. For Lisp use `"min-score"`.
    #[serde(default)]
    pub min_score: Option<f32>,
    /// Path to the memory DB for vector search when the in-memory index is empty
    /// (e.g. after server restart). Ignored on a nonempty index; it does not switch DBs.
    #[serde(default)]
    pub db_path: Option<String>,
    #[cfg(test)]
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) lisp_probe: Option<Arc<LispQueryProbe>>,
}

/// Parse a Lisp S-expression query string into query options.
///
/// The input must start with `(list` and evaluate to an association list
/// (alist) — a JSON array of 2-element arrays. Each pair is `(key, value)`.
///
/// Supported keys:
/// - `"query"` (string, required) — the natural language query
/// - `"top-k"` (int, default 5) — number of top results to return
/// - `"include-text"` (bool, default false) — include passage text in results
/// - `"min-score"` (float, default 0.0) — only return matches with score >= this
/// - `"generate-answer"` (bool, default false) — generate LLM answer from results
///
/// Returns `(query, top_k, include_text, min_score, generate_answer)`.
fn parse_lisp_query(
    expr: &str,
    cancellation: Arc<AtomicBool>,
) -> Result<(String, usize, bool, f32, bool), McpToolError> {
    let result = hkask_lisp::eval_sandboxed_with_cancellation(
        expr,
        &serde_json::Value::Null,
        100_000,
        100,
        cancellation,
    )
    .map_err(|e| McpToolError::invalid_argument(format!("Invalid Lisp query expression: {e}")))?;

    // The result can be either a JSON object (when `list` of pairs is
    // evaluated — the interpreter converts 2-element lists to key-value pairs)
    // or a JSON array of pairs. Handle both.
    let alist: Vec<serde_json::Value> = if let Some(obj) = result.as_object() {
        obj.iter().map(|(k, v)| json!([k, v])).collect()
    } else if let Some(arr) = result.as_array() {
        arr.to_vec()
    } else {
        return Err(McpToolError::invalid_argument(
            "Lisp query expression must evaluate to an association list (array of pairs) or object",
        ));
    };

    let mut nl_query = String::new();
    let mut top_k = 5usize;
    let mut include_text = false;
    let mut min_score = 0.0f32;
    let mut generate_answer = false;

    for pair in &alist {
        let arr = pair.as_array().ok_or_else(|| {
            McpToolError::invalid_argument(
                "Each element in the Lisp query alist must be a 2-element array",
            )
        })?;
        if arr.len() != 2 {
            return Err(McpToolError::invalid_argument(format!(
                "Each element in the Lisp query alist must be a 2-element array, got {} elements",
                arr.len()
            )));
        }
        let key = arr[0].as_str().ok_or_else(|| {
            McpToolError::invalid_argument(
                "First element of each query alist pair must be a string key",
            )
        })?;
        let value = &arr[1];
        match key {
            "query" => {
                nl_query = value
                    .as_str()
                    .ok_or_else(|| {
                        McpToolError::invalid_argument("\"query\" value must be a string")
                    })?
                    .to_string();
            }
            "top-k" => {
                top_k = value.as_u64().ok_or_else(|| {
                    McpToolError::invalid_argument("\"top-k\" value must be an integer")
                })? as usize;
            }
            "include-text" => {
                include_text = value.as_bool().ok_or_else(|| {
                    McpToolError::invalid_argument(
                        "\"include-text\" value must be a boolean (true/t or false)",
                    )
                })?;
            }
            "min-score" => {
                min_score = value.as_f64().ok_or_else(|| {
                    McpToolError::invalid_argument("\"min-score\" value must be a number")
                })? as f32;
            }
            "generate-answer" => {
                generate_answer = value.as_bool().ok_or_else(|| {
                    McpToolError::invalid_argument(
                        "\"generate-answer\" value must be a boolean (true/t or false)",
                    )
                })?;
            }
            _ => {
                return Err(McpToolError::invalid_argument(format!(
                    "Unknown query option key: '{key}'. Supported keys: \
                     query, top-k, include-text, min-score, generate-answer"
                )));
            }
        }
    }

    if nl_query.is_empty() {
        return Err(McpToolError::invalid_argument(
            "Lisp query expression must include a \"query\" key with a string value",
        ));
    }

    Ok((nl_query, top_k, include_text, min_score, generate_answer))
}

/// Owner drop requests cooperative stop; it never synchronously joins a worker.
struct LispQueryCancellation {
    flag: Arc<AtomicBool>,
    #[cfg(test)]
    probe: Option<Arc<LispQueryProbe>>,
}

impl Drop for LispQueryCancellation {
    fn drop(&mut self) {
        self.flag.store(true, Ordering::Release);
        #[cfg(test)]
        if let Some(probe) = &self.probe {
            probe.dropped.notify_one();
        }
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct LispQueryProbe {
    pub(crate) entered: tokio::sync::Notify,
    pub(crate) dropped: tokio::sync::Notify,
    pub(crate) exited: tokio::sync::Notify,
    pub(crate) release: std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    pub(crate) flag: std::sync::Mutex<Option<Arc<AtomicBool>>>,
    pub(crate) worker_thread: std::sync::Mutex<Option<std::thread::ThreadId>>,
    pub(crate) error: std::sync::Mutex<Option<String>>,
    pub(crate) context: std::sync::Mutex<Option<rmcp::service::RequestContext<rmcp::RoleServer>>>,
}

#[cfg(test)]
impl LispQueryProbe {
    fn enter(&self, flag: &Arc<AtomicBool>) {
        *self.flag.lock().expect("flag") = Some(flag.clone());
        *self.worker_thread.lock().expect("worker thread") = Some(std::thread::current().id());
        self.entered.notify_one();
        if let Some(release) = self.release.lock().expect("release").take() {
            release
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("release real worker");
        }
    }

    fn finished(&self, result: &Result<(String, usize, bool, f32, bool), McpToolError>) {
        *self.error.lock().expect("error") = result.as_ref().err().map(ToString::to_string);
        // The controlled evaluator has returned: invocation teardown precedes
        // this acknowledgment. Engine weak-scope probes belong to H6A.
        self.exited.notify_one();
    }
}

impl RetrievedPassage {
    fn project(&self, include_text: bool) -> serde_json::Value {
        let mut result = json!({"metadata": self.passage.metadata, "score": self.score});
        if self.passage.text.is_none() {
            result["text_available"] = json!(false);
        }
        if include_text {
            result["text"] = json!(self.passage.text);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::IndexedPassage;

    fn make_passage(text: &str, embedding: Vec<f32>) -> IndexedPassage {
        IndexedPassage {
            text: Some(text.to_string()),
            metadata: json!({"entity_ref": "test:chunk:1"}),
            embedding,
            model: "test-model".to_string(),
        }
    }

    fn search_passages(
        passages: &[IndexedPassage],
        query: &[f32],
        k: usize,
        min_score: f32,
        include_text: bool,
    ) -> Vec<serde_json::Value> {
        crate::index::search_passages(passages.iter(), query, k, min_score)
            .iter()
            .map(|matched| matched.project(include_text))
            .collect()
    }

    // ── search_passages ──────────────────────────────────────────────────

    #[test]
    fn search_returns_results_sorted_by_score() {
        let passages = vec![
            make_passage("low match", vec![0.0, 1.0]), // orthogonal → 0.0
            make_passage("high match", vec![1.0, 0.0]), // parallel → 1.0
            make_passage("medium match", vec![1.0, 1.0]), // 45° → ~0.707
        ];
        let query = vec![1.0, 0.0];
        let results = search_passages(&passages, &query, 3, 0.0, false);
        assert_eq!(results.len(), 3);
        // Highest score first
        assert!(results[0]["score"].as_f64().unwrap() > results[1]["score"].as_f64().unwrap());
        assert!(results[1]["score"].as_f64().unwrap() > results[2]["score"].as_f64().unwrap());
    }

    #[test]
    fn search_top_k_truncates() {
        let passages = vec![
            make_passage("a", vec![0.9, 0.0]),
            make_passage("b", vec![0.8, 0.0]),
            make_passage("c", vec![0.7, 0.0]),
        ];
        let query = vec![1.0, 0.0];
        let results = search_passages(&passages, &query, 2, 0.0, false);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_min_score_filters() {
        let passages = vec![
            make_passage("orthogonal", vec![0.0, 1.0]), // score 0.0
            make_passage("parallel", vec![1.0, 0.0]),   // score 1.0
        ];
        let query = vec![1.0, 0.0];
        let results = search_passages(&passages, &query, 10, 0.5, false);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["metadata"]["entity_ref"], "test:chunk:1");
    }

    #[test]
    fn search_include_text_adds_text_field() {
        let passages = vec![make_passage("secret text", vec![1.0, 0.0])];
        let query = vec![1.0, 0.0];
        let with_text = search_passages(&passages, &query, 1, 0.0, true);
        assert!(with_text[0].get("text").is_some());
        assert_eq!(with_text[0]["text"], "secret text");

        let without_text = search_passages(&passages, &query, 1, 0.0, false);
        assert!(without_text[0].get("text").is_none());
    }

    #[test]
    fn search_empty_passages_returns_empty() {
        let results = search_passages(&[], &[1.0, 0.0], 5, 0.0, false);
        assert!(results.is_empty());
    }

    fn parse_lisp_query(expr: &str) -> Result<(String, usize, bool, f32, bool), McpToolError> {
        super::parse_lisp_query(expr, Arc::new(AtomicBool::new(false)))
    }

    // ── parse_lisp_query ────────────────────────────────────────────────

    #[test]
    fn parse_lisp_query_basic() {
        let expr = r#"(list (list "query" "investment philosophy") (list "top-k" 3) (list "include-text" t))"#;
        let (query, k, include_text, min_score, gen_answer) =
            parse_lisp_query(expr).expect("should parse");
        assert_eq!(query, "investment philosophy");
        assert_eq!(k, 3);
        assert!(include_text);
        assert_eq!(min_score, 0.0);
        assert!(!gen_answer);
    }

    #[test]
    fn parse_lisp_query_with_min_score() {
        let expr = r#"(list (list "query" "test") (list "min-score" 0.7) (list "top-k" 10))"#;
        let (_, k, _, min_score, _) = parse_lisp_query(expr).expect("should parse");
        assert_eq!(k, 10);
        assert!((min_score - 0.7).abs() < 0.001);
    }

    #[test]
    fn parse_lisp_query_missing_query_returns_error() {
        let expr = r#"(list (list "top-k" 5))"#;
        assert!(parse_lisp_query(expr).is_err());
    }

    #[test]
    fn parse_lisp_query_unknown_key_returns_error() {
        let expr = r#"(list (list "query" "test") (list "unknown-key" 42))"#;
        assert!(parse_lisp_query(expr).is_err());
    }

    #[test]
    fn parse_lisp_query_generate_answer() {
        let expr = r#"(list (list "query" "test") (list "generate-answer" t))"#;
        let (_, _, _, _, gen_answer) = parse_lisp_query(expr).expect("should parse");
        assert!(gen_answer);
    }

    // ── parse_lisp_query properties (testing protocol property layer, Batch 4) ──

    use proptest::prelude::*;

    proptest! {
        /// Hypothesis: every well-formed query alist round-trips — generated
        /// query text, top-k, min-score, and true flags parse back exactly.
        /// Explicit false stays boolean; nil remains JSON null and is rejected.
        #[test]
        fn parse_lisp_query_round_trips_generated_requests(
            query in "[a-z ]{1,40}",
            top_k in 1u64..=1000,
            include_text in any::<bool>(),
            gen_answer in any::<bool>(),
            min_score in 0.0f64..=1.0,
        ) {
            let mut expr = format!(
                "(list (list \"query\" \"{query}\") (list \"top-k\" {top_k}) (list \"min-score\" {min_score})"
            );
            expr.push_str(&format!(" (list \"include-text\" {include_text})"));
            expr.push_str(&format!(" (list \"generate-answer\" {gen_answer})"));
            expr.push(')');
            let (q, k, inc, ms, generate) =
                parse_lisp_query(&expr).expect("well-formed query alist parses");
            prop_assert_eq!(q, query);
            prop_assert_eq!(k, top_k as usize);
            prop_assert_eq!(inc, include_text);
            prop_assert_eq!(generate, gen_answer);
            prop_assert!((ms - min_score as f32).abs() < 1e-5);
        }

        /// Hypothesis: parsing is total — arbitrary input either parses or
        /// returns a typed error. The sandboxed evaluator's budgets bound
        /// evaluation and the alist walker never panics.
        #[test]
        fn parse_lisp_query_is_total_over_arbitrary_text(input in ".*") {
            let _ = parse_lisp_query(&input);
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ClearIndexRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PurgeQaRequest {
    /// Entity-ref prefix to purge (e.g. "corpus:researcher:").
    #[serde(default = "default_purge_prefix")]
    pub prefix: String,
    /// Path to the SQLCipher memory DB.
    pub db_path: String,
}

fn default_purge_prefix() -> String {
    "corpus:researcher:".to_string()
}
