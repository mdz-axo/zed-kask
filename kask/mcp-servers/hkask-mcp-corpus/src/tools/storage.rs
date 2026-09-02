//! Storage and query tools — cache, passage query, similarity.
use crate::helpers::{map_corpus_io_error, map_memory_store_error};
use crate::{
    CorpusServer, IndexedPassage, LLMParameters, McpToolError, Parameters, cosine_similarity,
    execute_tool, json, render_docproc_template, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

#[tool_router(router = storage_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Cache processed document text for reference. Stores content keyed by label in the corpus cache directory (corpus-mcp/cache/ under the visible artifacts dir, ~/Documents/zk-data/)."
    )]
    pub async fn corpus_cache(
        &self,
        Parameters(CacheRequest { content, label }): Parameters<CacheRequest>,
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
                return Err(map_corpus_io_error(
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
            let cache_path = cache_dir.join(format!("{}.md", safe_label));

            match std::fs::write(&cache_path, &content) {
                Ok(()) => {
                    let result = json!({
                        "label": label,
                        "path": cache_path.display().to_string(),
                        "size_bytes": content.len(),
                    });
                    Ok(result)
                }
                Err(e) => Err(map_corpus_io_error(
                    e,
                    &format!("Failed to write cache file '{}'", cache_path.display()),
                )),
            }
        })
        .await
    }

    #[tool(
        description = "Query the in-memory vector index for passages relevant to a natural language question. Embeds the query, computes cosine similarity against indexed passages, and returns top-k results. Optionally generates an LLM-augmented answer from retrieved context."
    )]
    /// Query the in-memory vector index for top-k relevant passages.
    ///
    /// # Availability over consistency on poisoned lock
    ///
    /// If the index mutex is poisoned (a prior holder panicked), this method
    /// recovers the inner state via `into_inner()` and serves the query
    /// against possibly-half-mutated state rather than returning an error.
    /// This is a deliberate availability-over-consistency choice: a poisoned
    /// lock typically indicates a panic during a non-mutating read path, so
    /// the index contents are likely intact, and refusing the query would
    /// take the corpus offline for every subsequent caller until restart.
    /// The two sibling recovery sites (`corpus_clear_index`, `corpus_purge_qa`)
    /// immediately overwrite the index, so they are safe under the same
    /// recovery. A panic during `corpus_chunk`'s incremental insert would
    /// leave a partially-updated index; this method would still serve from
    /// it — the worst case is stale or incomplete results, not corruption
    /// (the index is rebuilt from the source JSONL on restart).
    pub async fn corpus_query(
        &self,
        Parameters(QueryRequest {
            query,
            top_k,
            generate_answer,
            include_text,
            min_score,
            db_path,
            passphrase,
        }): Parameters<QueryRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_query", async {
            if query.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "query must not be empty",
                ));
            }

            // Parse the query string. When it starts with `(list`, it's a
            // Lisp S-expression specifying query options. Otherwise it's a
            // plain natural-language query (backward compatible).
            let (nl_query, k, include_text_flag, min_score_val, gen_answer) =
                if query.trim_start().starts_with("(list") {
                    parse_lisp_query(&query)?
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
            let model_name = crate::default_embedding_model().to_string();

            let query_embedding = match self
                .inference_router
                .embed(&model_name, std::slice::from_ref(&nl_query))
                .await
            {
                Ok(v) => v.into_iter().next().unwrap_or_default(),
                Err(e) => {
                    return Err(McpToolError::unavailable(format!(
                        "Query embedding failed: {}",
                        e
                    )));
                }
            };

            if query_embedding.is_empty() {
                return Err(McpToolError::unavailable(
                    "Query embedding returned empty vector",
                ));
            }

            // Search the index (scoped to drop guard before any await)
            let (results, total_indexed) = {
                let mut index = match self.index.lock() {
                    Ok(i) => i,
                    Err(poisoned) => {
                        tracing::warn!(
                            target: "hkask.mcp.corpus",
                            error = %poisoned,
                            "index lock poisoned — recovering inner state"
                        );
                        poisoned.into_inner()
                    }
                };
                if index.is_empty() {
                    let Some(db_path) = db_path.as_deref() else {
                        return Ok(json!({
                            "query": query,
                            "results": [],
                            "total_indexed": 0,
                            "note": "Index empty after restart. Provide db_path to search the memory DB.",
                        }));
                    };
                    let passphrase = passphrase
                        .unwrap_or_else(crate::helpers::default_corpus_passphrase);
                    if passphrase.is_empty() {
                        return Err(McpToolError::permission_denied(
                            "HKASK_DB_PASSPHRASE not configured — corpus_query requires the DB passphrase. \
                             Set it via the keychain (kask://credentials/hkask_db_passphrase) or environment variable."
                        ));
                    }
                    let store = crate::helpers::open_memory_store(db_path, &passphrase)?;
                    // A failed count must not read as "zero embeddings" —
                    // a DB outage would masquerade as an empty corpus and
                    // return success with no results.
                    let total = store.embedding_count().map_err(|e| {
                        McpToolError::internal(format!("embedding count read failed: {e}")) // rr0044-ok: infra-db-failure
                    })?;
                    if total == 0 {
                        return Ok(json!({
                            "query": query,
                            "results": [],
                            "total_indexed": 0,
                        }));
                    }
                    // Hydrate the in-memory index from the DB so subsequent
                    // queries return full passage text without re-opening the
                    // DB. This loads all embeddings + text in one pass.
                    let all_embeddings = store
                        .all_embeddings_with_text()
                        .map_err(|e| McpToolError::internal(format!("DB hydration failed: {e}")))?; // rr0044-ok: infra-db-failure
                    let mut hydrated: Vec<IndexedPassage> = Vec::with_capacity(all_embeddings.len());
                    for (entity_ref, vector, passage_text) in all_embeddings {
                        let text = passage_text.unwrap_or_default();
                        hydrated.push(IndexedPassage {
                            text: text.clone(),
                            metadata: json!({"entity_ref": entity_ref}),
                            embedding: vector,
                        });
                    }
                    tracing::info!(
                        target: "hkask.mcp.corpus",
                        hydrated = hydrated.len(),
                        "In-memory index hydrated from DB"
                    );
                    // Search the hydrated passages in-memory.
                    let results = search_passages(
                        &hydrated,
                        &query_embedding,
                        k,
                        min_score_val,
                        include_text_flag,
                    );
                    // Move hydrated passages into the persistent in-memory index
                    // so subsequent queries skip the DB hydration pass.
                    let hydrated_count = hydrated.len();
                    index.extend(hydrated);
                    (results, hydrated_count)
                } else {
                    let results = search_passages(
                        &index,
                        &query_embedding,
                        k,
                        min_score_val,
                        include_text_flag,
                    );

                    (results, index.len())
                }
            }; // guard dropped here

            let mut result = json!({
                "query": nl_query,
                "results": results,
                "total_indexed": total_indexed,
            });

            // Optionally generate an LLM-augmented answer
            if gen_answer && !results.is_empty() {
                let context: String = results
                    .iter()
                    .map(|r| r["text"].as_str().unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let context = crate::guard_content(&context);

                // C10: Load prompt from registry template, fall back to inline if unavailable
                let mut vars = std::collections::HashMap::new();
                vars.insert("context", context.clone());
                vars.insert("question", query.clone());
                let prompt = render_docproc_template("rag-answer", &vars);
                let prompt = if prompt.is_empty() {
                    format!(
                        "{CONTENT_GUARD_INSTRUCTION}Answer the following question based on the provided context. If the context doesn't contain enough information, say so.\n\n\
                         Context:\n{context}\n\n\
                         Question: {query}\n\n\
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
        description = "Clear the in-memory vector index. Call this when starting a new document set to avoid cross-document contamination in query results."
    )]
    pub async fn corpus_clear_index(
        &self,
        Parameters(ClearIndexRequest {}): Parameters<ClearIndexRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_clear_index", async {
            let mut index = match self.index.lock() {
                Ok(i) => i,
                Err(poisoned) => {
                    tracing::warn!(
                        target: "hkask.mcp.corpus",
                        error = %poisoned,
                        "index lock poisoned — recovering inner state"
                    );
                    poisoned.into_inner()
                }
            };
            let cleared = index.len();
            index.clear();
            Ok(json!({"cleared": cleared}))
        })
        .await
    }

    #[tool(
        description = "Purge QA embeddings and h_mems by entity-ref prefix. Deletes embeddings matching the prefix, then deletes h_mems with matching entity or attribute. Useful for clearing old training data before re-ingesting."
    )]
    pub async fn corpus_purge_qa(
        &self,
        Parameters(req): Parameters<PurgeQaRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_purge_qa", async {
            if req.passphrase.is_empty() {
                return Err(McpToolError::permission_denied(
                    "HKASK_DB_PASSPHRASE not configured — corpus_purge_qa requires the DB passphrase. \
                     Set it via the keychain (kask://credentials/hkask_db_passphrase) or environment variable."
                ));
            }
            let store = crate::helpers::open_memory_store(&req.db_path, &req.passphrase)?;

            let embeddings_before = match store.embedding_count() {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.corpus",
                        error = %e,
                        "Failed to read embedding_count before purge — returning 0 (signal stale)"
                    );
                    0
                }
            };

            // Purge embeddings with matching entity_ref prefix
            let purged_embeddings = store
                .purge_by_prefix(&req.prefix)
                .map_err(|e| map_memory_store_error(e, "Purge embeddings failed"))?;

            // Purge all h_mems by entity prefix — assertions, training_qa_pairs,
            // and any other attributes — so stale data from a previous pipeline
            // run doesn't pollute the new run.
            let mut purged_h_mems = 0usize;
            let mut h_mem_errors = 0usize;

            let h_mems = store
                .h_mems_by_entity_prefix(&req.prefix)
                .map_err(|e| map_memory_store_error(e, "Query h_mems by prefix failed"))?;
            for h_mem in &h_mems {
                match store.delete_h_mem(&h_mem.id) {
                    Ok(()) => purged_h_mems += 1,
                    Err(_) => h_mem_errors += 1,
                }
            }

            let embeddings_after = match store.embedding_count() {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.corpus",
                        error = %e,
                        "Failed to read embedding_count after purge — returning 0 (signal stale)"
                    );
                    0
                }
            };

            let result = json!({
                "prefix": req.prefix,
                "embeddings_before": embeddings_before,
                "embeddings_purged": purged_embeddings,
                "embeddings_after": embeddings_after,
                "h_mems_purged": purged_h_mems,
                "h_mem_errors": h_mem_errors,
            });
            Ok(result)
        })
        .await
    }
}

// ── Request structs ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CacheRequest {
    /// Text content to cache.
    pub content: String,
    /// Label/key for the cached entry.
    pub label: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct QueryRequest {
    /// Natural language question to search for, OR a Lisp S-expression
    /// specifying query options.
    ///
    /// When the string starts with `(list`, it is parsed as a Lisp
    /// S-expression via `hkask_lisp::eval_sandboxed_with_budget`.
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
    /// plain natural-language query (backward compatible with the original
    /// `query` + `top_k` + `generate_answer` parameters).
    pub query: String,
    /// Number of top results to return (default 5). Ignored when `query`
    /// is a Lisp S-expression (use `"top-k"` in the expression instead).
    #[serde(default)]
    pub top_k: Option<usize>,
    /// If true, generate an LLM-augmented answer from retrieved passages.
    /// Ignored when `query` is a Lisp S-expression.
    #[serde(default)]
    pub generate_answer: Option<bool>,
    /// If true, include passage text in results. Only valid when `query`
    /// is a Lisp S-expression (use `"include-text"` in the expression).
    /// When `query` is a plain string, text is included by default when
    /// the index is hydrated from the DB.
    #[serde(default)]
    pub include_text: Option<bool>,
    /// Minimum score threshold (0.0–1.0). Only return matches with score
    /// >= this value. Only valid when `query` is a Lisp S-expression.
    #[serde(default)]
    pub min_score: Option<f32>,
    /// Path to the memory DB for vector search when the in-memory index is empty
    /// (e.g. after server restart). The in-memory index is populated by `corpus_embed`.
    #[serde(default)]
    pub db_path: Option<String>,
    /// Passphrase for the memory DB. Defaults to `HKASK_DB_PASSPHRASE`.
    #[serde(default)]
    pub passphrase: Option<String>,
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
fn parse_lisp_query(expr: &str) -> Result<(String, usize, bool, f32, bool), McpToolError> {
    let result =
        hkask_lisp::eval_sandboxed_with_budget(expr, &serde_json::Value::Null, 100_000, 100)
            .map_err(|e| {
                McpToolError::invalid_argument(format!("Invalid Lisp query expression: {e}"))
            })?;

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
                        "\"include-text\" value must be a boolean (t or nil)",
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
                        "\"generate-answer\" value must be a boolean (t or nil)",
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

/// Search a slice of indexed passages by cosine similarity to a query
/// embedding, with optional min-score filtering and text inclusion.
///
/// Extracted from `corpus_query` where the same score → sort → filter →
/// truncate → serialize logic was duplicated between the hydrated-from-DB
/// path and the in-memory-index path.
fn search_passages(
    passages: &[IndexedPassage],
    query_embedding: &[f32],
    k: usize,
    min_score: f32,
    include_text: bool,
) -> Vec<serde_json::Value> {
    let mut scored: Vec<(f32, &IndexedPassage)> = passages
        .iter()
        .map(|p| (cosine_similarity(query_embedding, &p.embedding), p))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    if min_score > 0.0 {
        scored.retain(|(score, _)| *score >= min_score);
    }
    scored.truncate(k);
    scored
        .iter()
        .map(|(score, p)| {
            let mut entry = json!({
                "metadata": p.metadata.clone(),
                "score": score,
            });
            if include_text {
                entry["text"] = json!(p.text.clone());
            }
            entry
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IndexedPassage;

    fn make_passage(text: &str, embedding: Vec<f32>) -> IndexedPassage {
        IndexedPassage {
            text: text.to_string(),
            metadata: json!({"entity_ref": "test:chunk:1"}),
            embedding,
        }
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
    /// Passphrase for the memory DB.
    #[serde(default = "crate::helpers::default_corpus_passphrase")]
    pub passphrase: String,
}

fn default_purge_prefix() -> String {
    "corpus:researcher:".to_string()
}
