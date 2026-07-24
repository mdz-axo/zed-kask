//! Storage and query tools — cache, passage query, similarity.
use crate::*;
use schemars::JsonSchema;
use serde::Deserialize;

#[tool_router(router = storage_router, vis = "pub")]
impl DocProcServer {
    #[tool(
        description = "Cache processed document text for reference. Stores content keyed by label in the docproc cache directory (~/.config/hkask/docproc-cache/)."
    )]
    pub async fn docproc_cache(
        &self,
        Parameters(CacheRequest { content, label }): Parameters<CacheRequest>,
    ) -> String {
        execute_tool(self, "docproc_cache", async {
            if content.is_empty() {
                return Err(McpToolError::invalid_argument("content must not be empty"));
            }

            if label.is_empty() {
                return Err(McpToolError::invalid_argument("label must not be empty"));
            }

            // Resolve cache directory
            let cache_dir = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("hkask")
                .join("docproc-cache");

            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                return Err(McpToolError::internal(format!(
                    "Failed to create cache directory '{}': {}",
                    cache_dir.display(),
                    e
                )));
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
                    self.record_experience("docproc_cache", &label, "success", result.clone());
                    Ok(result)
                }
                Err(e) => Err(McpToolError::internal(format!(
                    "Failed to write cache file '{}': {}",
                    cache_path.display(),
                    e
                ))),
            }
        })
        .await
    }

    #[tool(
        description = "Query the in-memory vector index for passages relevant to a natural language question. Embeds the query, computes cosine similarity against indexed passages, and returns top-k results. Optionally generates an LLM-augmented answer from retrieved context."
    )]
    pub async fn docproc_query(
        &self,
        Parameters(QueryRequest {
            query,
            top_k,
            generate_answer,
        }): Parameters<QueryRequest>,
    ) -> String {
        execute_tool(self, "docproc_query", async {
            if query.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "query must not be empty",
                ));
            }

            let k = top_k.unwrap_or(5).clamp(1, 50);

            // Embed the query
            let Some(ref emb_router) = self.embedding_router else {
                return Err(McpToolError::failed_precondition(
                    "Embedding router not configured — cannot embed query",
                ));
            };

            let model_name = std::env::var("HKASK_EMBEDDING_MODEL")
                .unwrap_or_else(|_| "DI/Qwen/Qwen3-Embedding-0.6B".to_string());

            let query_embedding = match emb_router
                .embed_sentences(&model_name, &[query.as_str()])
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
                    Err(e) => {
                        return Err(McpToolError::internal(format!(
                            "Index lock error: {}",
                            e
                        )));
                    }
                };
                if index.is_empty() {
                    return Ok(json!({
                        "query": query,
                        "results": [],
                        "total_indexed": 0,
                        "note": "No passages indexed. Run docproc_chunk with index=true first.",
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

                // C10: Load prompt from registry template, fall back to inline if unavailable
                let mut vars = std::collections::HashMap::new();
                vars.insert("context", context.clone());
                vars.insert("question", query.clone());
                let prompt = render_docproc_template("rag-answer", &vars);
                let prompt = if prompt.is_empty() {
                    format!(
                        "Answer the following question based on the provided context. If the context doesn't contain enough information, say so.\n\n\
                         Context:\n{context}\n\n\
                         Question: {query}\n\n\
                         Answer:"
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

            self.record_experience("docproc_query", &query, "success", result.clone());
            Ok(result)
        })
        .await
    }

    #[tool(
        description = "Clear the in-memory vector index. Call this when starting a new document set to avoid cross-document contamination in query results."
    )]
    pub async fn docproc_clear_index(
        &self,
        Parameters(ClearIndexRequest { index_id: _ }): Parameters<ClearIndexRequest>,
    ) -> String {
        execute_tool(self, "docproc_clear_index", async {
            let mut index = match self.index.lock() {
                Ok(i) => i,
                Err(e) => {
                    return Err(McpToolError::internal(format!("Index lock error: {}", e)));
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
    pub async fn docproc_purge_qa(&self, Parameters(req): Parameters<PurgeQaRequest>) -> String {
        execute_tool(self, "docproc_purge_qa", async {
            let dim = embedding_dim();
            let semantic =
                SemanticMemory::open(&req.db_path, &req.passphrase, dim).map_err(|e| {
                    McpToolError::failed_precondition(format!("Cannot open memory DB: {e}"))
                })?;

            let embeddings_before = semantic.embedding_count().unwrap_or(0);

            // Purge embeddings with matching entity_ref prefix
            let purged_embeddings = semantic
                .purge_by_prefix(&req.prefix)
                .map_err(|e| McpToolError::internal(format!("Purge embeddings failed: {e}")))?;

            // Purge h_mems — old schema (entity="corpus:qa") vs new schema (entity starts with prefix)
            let mut purged_h_mems = 0usize;
            let mut h_mem_errors = 0usize;

            if req.prefix == "corpus:qa" {
                // Old schema: entity is exactly "corpus:qa"
                let h_mems = semantic
                    .query_deduped(&req.prefix)
                    .map_err(|e| McpToolError::internal(format!("Query h_mems failed: {e}")))?;
                for h_mem in &h_mems {
                    match semantic.delete_h_mem(&h_mem.id) {
                        Ok(()) => purged_h_mems += 1,
                        Err(_) => h_mem_errors += 1,
                    }
                }
            } else {
                // New schema: query by attribute "training_qa_pair" and filter by entity prefix
                let h_mems = semantic
                    .query_by_attribute("training_qa_pair")
                    .map_err(|e| McpToolError::internal(format!("Query h_mems failed: {e}")))?;
                for h_mem in &h_mems {
                    if h_mem.entity.starts_with(&req.prefix) {
                        match semantic.delete_h_mem(&h_mem.id) {
                            Ok(()) => purged_h_mems += 1,
                            Err(_) => h_mem_errors += 1,
                        }
                    }
                }
            }

            let embeddings_after = semantic.embedding_count().unwrap_or(0);

            let result = json!({
                "prefix": req.prefix,
                "embeddings_before": embeddings_before,
                "embeddings_purged": purged_embeddings,
                "embeddings_after": embeddings_after,
                "h_mems_purged": purged_h_mems,
                "h_mem_errors": h_mem_errors,
            });
            self.record_experience("docproc_purge_qa", &req.prefix, "success", result.clone());
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
    /// Entity-ref prefix to purge (e.g. "corpus:qa" for old schema, "training:qa:" for new).
    #[serde(default = "default_purge_prefix")]
    pub prefix: String,
    /// Path to the SQLCipher memory DB.
    pub db_path: String,
    /// Passphrase for the memory DB.
    #[serde(default = "default_purge_passphrase")]
    pub passphrase: String,
}

fn default_purge_prefix() -> String {
    "corpus:qa".to_string()
}

fn default_purge_passphrase() -> String {
    "hkask-default-passphrase-2024".to_string()
}
