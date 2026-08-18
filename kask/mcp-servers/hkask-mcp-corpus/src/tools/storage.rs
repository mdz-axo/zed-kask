//! Storage and query tools — cache, passage query, similarity.
use crate::helpers::{map_corpus_io_error, map_database_error, map_memory_store_error};
use crate::{
    CorpusServer, IndexedPassage, LLMParameters, McpToolError, MemoryStore, Parameters,
    cosine_similarity, embedding_dim, execute_tool_semantic, json, render_docproc_template, tool,
    tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

#[tool_router(router = storage_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Cache processed document text for reference. Stores content keyed by label in the corpus cache directory (mcp/corpus/cache/)."
    )]
    pub async fn corpus_cache(
        &self,
        Parameters(CacheRequest { content, label }): Parameters<CacheRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "corpus_cache",
            Self::ontology_anchor("corpus_cache"),
            async {
                if content.is_empty() {
                    return Err(McpToolError::invalid_argument("content must not be empty"));
                }

                if label.is_empty() {
                    return Err(McpToolError::invalid_argument("label must not be empty"));
                }

                // D28 — Standardized Artifact Storage. Cache directory lives at
                // `mcp/corpus/cache/`.
                let cache_dir = hkask_types::agent_paths::resolve_under_data_dir(
                    std::path::Path::new("mcp/corpus/cache"),
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
            },
        )
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
        }): Parameters<QueryRequest>,
    ) -> String {
        execute_tool_semantic(self, "corpus_query", Self::ontology_anchor("corpus_query"), async {
            if query.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "query must not be empty",
                ));
            }

            let k = top_k.unwrap_or(5).clamp(1, 50);

            let model_name = crate::default_embedding_model().to_string();

            let query_embedding = match self
                .inference_router
                .embed(&model_name, std::slice::from_ref(&query))
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
                let index = match self.index.lock() {
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
                    return Ok(json!({
                        "query": query,
                        "results": [],
                        "total_indexed": 0,
                        "note": "No passages indexed. Run corpus_chunk with index=true first.",
                    }));
                }

                let mut scored: Vec<(f32, &IndexedPassage)> = index
                    .iter()
                    .map(|p| (cosine_similarity(&query_embedding, &p.embedding), p))
                    .collect();

                scored.sort_by(|a, b| {
                    b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
                });
                scored.truncate(k);

                let results: Vec<serde_json::Value> = scored
                    .iter()
                    .map(|(score, p)| {
                        json!({
                            "text": p.text.clone(),
                            "metadata": p.metadata.clone(),
                            "score": score,
                        })
                    })
                    .collect();

                (results, index.len())
            }; // guard dropped here

            let mut result = json!({
                "query": query,
                "results": results,
                "total_indexed": total_indexed,
            });

            // Optionally generate an LLM-augmented answer
            if generate_answer.unwrap_or(false) && !results.is_empty() {
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
                    max_tokens: 1024,
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
        Parameters(ClearIndexRequest { index_id: _ }): Parameters<ClearIndexRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "corpus_clear_index",
            Self::ontology_anchor("corpus_clear_index"),
            async {
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
            },
        )
        .await
    }

    #[tool(
        description = "Purge QA embeddings and h_mems by entity-ref prefix. Deletes embeddings matching the prefix, then deletes h_mems with matching entity or attribute. Useful for clearing old training data before re-ingesting."
    )]
    pub async fn corpus_purge_qa(&self, Parameters(req): Parameters<PurgeQaRequest>) -> String {
        execute_tool_semantic(self, "corpus_purge_qa", Self::ontology_anchor("corpus_purge_qa"), async {
            let dim = embedding_dim();
            let store = MemoryStore::open(&req.db_path, &req.passphrase, dim)
                .map_err(|e| map_database_error(e, "Cannot open memory DB"))?;

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
pub struct CacheRequest {
    /// Text content to cache.
    pub content: String,
    /// Label/key for the cached entry.
    pub label: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueryRequest {
    /// Natural language question to search for.
    pub query: String,
    /// Number of top results to return (default 5).
    #[serde(default)]
    pub top_k: Option<usize>,
    /// If true, generate an LLM-augmented answer from retrieved passages.
    #[serde(default)]
    pub generate_answer: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClearIndexRequest {
    /// Reserved for future multi-index support.
    #[serde(default)]
    pub index_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PurgeQaRequest {
    /// Entity-ref prefix to purge (e.g. "corpus:researcher:").
    #[serde(default = "default_purge_prefix")]
    pub prefix: String,
    /// Path to the SQLCipher memory DB.
    pub db_path: String,
    /// Passphrase for the memory DB.
    #[serde(default = "default_purge_passphrase")]
    pub passphrase: String,
}

fn default_purge_prefix() -> String {
    "corpus:researcher:".to_string()
}

fn default_purge_passphrase() -> String {
    // Reuses the corpus server's 3-tier resolution chain (ctx.credentials →
    // env → keychain) via `default_corpus_passphrase`, which reads the
    // `OnceLock` set at server construction. See
    // `crate::tools::semantic::set_corpus_db_passphrase`.
    crate::tools::semantic::default_corpus_passphrase()
}
