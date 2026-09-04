//! Semantic extraction tools — QA generation, h_mem extraction, embedding.
//!
//! This module is the router host for the `semantic_router` tool group.
//! Helpers live in submodules:
//! - `qa` — QA response parsing, batch writer, model resolution
//! - `assertions` — RDF predicate → 5W1H dimension mapping
//! - `ontology_io` — tagged-chunks JSONL readers
//!
//! The `#[tool_router]` macro requires all `#[tool]` methods to be on a single
//! `impl CorpusServer` block, so the tool methods stay here in `semantic.rs`.

mod assertions;
pub(crate) mod batch_api;
mod ontology_io;
pub(crate) mod qa;

use crate::batch::{
    ADAPTIVE_CONCURRENCY_FLOOR, AdaptiveLimiter, BatchOutcome, MAX_RETRIES, retry_with_backoff,
};
use crate::helpers::default_corpus_passphrase;
use crate::services::assertions::{AssertionsRequest, AssertionsService};
use crate::{
    Arc, CorpusServer, IndexedPassage, McpToolError, Mutex, Parameters, default_embedding_model,
    default_owner, execute_tool, extract_json_from_response, json, read_jsonl, tool, tool_router,
};
use ontology_io::read_ontology_tags_annotated;
use qa::parse_qa_response;
use schemars::JsonSchema;
use serde::Deserialize;

// Re-export helpers used by other tool modules (corpus.rs imports these) and
// make them available within this module via the module path.
pub(crate) use assertions::{
    abstract_namespace_tag_key, assertion_confidence, predicate_to_dimension,
};
pub(crate) use ontology_io::read_ontology_namespaces;
pub(crate) use ontology_io::read_ontology_tags;
pub(crate) use qa::configured_qa_model;

#[tool_router(router = semantic_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Generate QA pairs from text chunks. Accepts a single chunk (text) or multiple chunks (texts) for cross-reference synthesis. Uses Bloom's taxonomy levels. Multi-chunk mode (texts) generates QAs that require synthesizing across all passages with source citation."
    )]
    pub async fn corpus_generate_qa(
        &self,
        Parameters(GenerateQaRequest {
            text: _text,
            texts: _texts,
            chunk_id,
            bloom_levels,
            model,
        }): Parameters<GenerateQaRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_generate_qa", async {
            let cross_ref_passages = _texts.as_ref().filter(|texts| !texts.is_empty());
            let is_cross_ref = cross_ref_passages.is_some();
            let single_text = _text.unwrap_or_default();

            if !is_cross_ref && single_text.is_empty() {
                return Err(McpToolError::invalid_argument("text must not be empty (or set texts for cross-reference mode)"));
            }
            if chunk_id.is_empty() {
                return Err(McpToolError::invalid_argument("chunk_id must not be empty"));
            }

            let levels = bloom_levels.unwrap_or_else(crate::services::qa_pipeline::default_bloom_levels);
            let levels_str = levels.join(", ");

            let (prompt, template_source) = if let Some(passages) = cross_ref_passages {
                let formatted = crate::services::qa_pipeline::format_cross_reference_prompt(
                    &levels_str,
                    &chunk_id,
                    passages,
                );
                (formatted.text, formatted.template_source)
            } else {
                let single_text = crate::guard_content(&single_text);
                let formatted = crate::services::qa_pipeline::format_single_chunk_prompt(
                    &levels_str,
                    &chunk_id,
                    &single_text,
                );
                (formatted.text, formatted.template_source)
            };
            let selected_model = configured_qa_model(model);

            let params = crate::services::qa_pipeline::qa_llm_parameters();

            match self
                .inference_router
                .generate_with_model(&prompt, &params, selected_model.as_deref(), None)
                .await
            {
                Ok(response) => {
                    let content = &response.text;
                    let qa_response = parse_qa_response(
                        &extract_json_from_response(content),
                        &levels,
                        is_cross_ref.then(|| cross_ref_passages.map_or(0, Vec::len)),
                    )
                    .map_err(|e| McpToolError::internal(e.to_string()))?; // rr0044-ok: parse-llm-output
                    let result = json!({
                        "chunk_id": chunk_id,
                        "bloom_levels": levels,
                        "cross_reference": is_cross_ref,
                        "qa_pairs": qa_response.qa_pairs,
                        "provenance": {
                            "generator_model": selected_model.as_deref().unwrap_or("router_default"),
                            "generator_parameters": params,
                            "prompt_template": template_source,
                            "source_chunk_ref": chunk_id,
                        },
                        "tokens_used": response.usage.total_tokens,
                    });
                    Ok(result)
                }
                Err(e) => Err(McpToolError::unavailable(format!("QA generation failed: {}", e))),
            }
        })
        .await
    }

    #[tool(
        description = "Batch-generate QA pairs from multiple text chunks. Same pipeline as corpus_generate_qa (Bloom taxonomy, templates). Uses configurable concurrency for parallel LLM calls. Reads prompts from prompts_jsonl (one JSON per line: chunk_ref, qa_type, system, user) and writes generated QAs to the output JSONL file. Returns a summary (total + written counts)."
    )]
    pub async fn corpus_generate_qa_batch(
        &self,
        Parameters(GenerateQaBatchRequest {
            prompts_jsonl,
            output,
            concurrency,
            model,
        }): Parameters<GenerateQaBatchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_generate_qa_batch", async {
            crate::services::qa_batch::QaBatchService::new(Arc::clone(&self.inference_router))
                .generate_qa_batch(crate::services::qa_batch::QaBatchRequest {
                    prompts_jsonl,
                    output,
                    concurrency,
                    model,
                })
                .await
        })
        .await
    }

    #[tool(
        description = "Extract assertions (subject, predicate, object) from corpus chunks using the inference engine. Uses the canonical classifier model (HKASK_CLASSIFIER_MODEL, default GLM-5.2 on OpenRouter) with 3-attempt retry. Reads chunks from chunks_jsonl, processes them concurrently, and stores each assertion as a chunk-anchored h_mem (entity=entity_ref, attribute=predicate, value={subject, object}) in the memory DB. When tagged_jsonl is provided, ontology tags from the tagging step are injected to guide predicate selection (GOLEM for narrative, schema.org for expository). Returns a summary (total_chunks, succeeded, failed, h_mems_stored)."
    )]
    pub async fn corpus_extract_assertions(
        &self,
        Parameters(ExtractAssertionsRequest {
            chunks_jsonl,
            tagged_jsonl,
            db_path,
            passphrase,
            max_assertions,
            owner,
            concurrency,
        }): Parameters<ExtractAssertionsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_extract_assertions", async {
            AssertionsService::new(Arc::clone(&self.inference_router))
                .extract(AssertionsRequest {
                    chunks_jsonl,
                    tagged_jsonl,
                    max_assertions,
                    db_path,
                    passphrase,
                    owner,
                    concurrency,
                })
                .await
        })
        .await
    }

    #[tool(
        description = "Generate ontology-anchored embedding vectors for corpus chunks. Uses the configured embedding model via the inference router. Reads chunks from chunks_jsonl (entity_ref, source, text, word_count per line). When tagged_jsonl is provided, ontology tags are prepended to chunk text before embedding (per INSTRUCTOR, Su et al. 2023), producing vectors that encode both content and ontological classification. Batch-embeds in groups of batch_size and stores each vector in the memory DB. Returns a summary (total, embedded, failed, model) — no inline vectors."
    )]
    pub async fn corpus_embed(
        &self,
        Parameters(EmbedRequest {
            chunks_jsonl,
            tagged_jsonl,
            db_path,
            passphrase,
            model,
            batch_size,
        }): Parameters<EmbedRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_embed", async {
            self.embed_batch_from_jsonl(
                &chunks_jsonl,
                tagged_jsonl.as_deref(),
                model,
                &db_path,
                &passphrase,
                batch_size,
            )
            .await
        })
        .await
    }

    /// Batch embed chunks from a JSONL file with configurable batch size.
    ///
    /// Reads chunks (entity_ref, source, text, word_count per line), batch-embeds
    /// in groups of `batch_size` **concurrently** using a semaphore-gated
    /// `tokio::spawn` per batch, stores each vector in the DB, and returns a
    /// summary. Concurrent batch processing prevents MCP context server timeouts
    /// on large corpora (33K+ chunks) — the previous sequential loop took
    /// minutes, exceeding the ~30s MCP timeout.
    async fn embed_batch_from_jsonl(
        &self,
        chunks_path: &str,
        tagged_jsonl: Option<&str>,
        model: Option<String>,
        db_path: &str,
        passphrase: &str,
        batch_size: usize,
    ) -> Result<serde_json::Value, McpToolError> {
        // Parse chunks: each line has entity_ref, source, text, word_count
        let chunks_values = read_jsonl::<serde_json::Value>(chunks_path, "chunks_jsonl")?;
        let mut chunks: Vec<(String, String)> = Vec::new(); // (entity_ref, text)
        for (i, v) in chunks_values.iter().enumerate() {
            let entity_ref = v
                .get("entity_ref")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let text = v
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if entity_ref.is_empty() || text.is_empty() {
                tracing::warn!(
                    target: "hkask.mcp.docproc.embed",
                    line = i + 1,
                    "Skipping chunk with empty entity_ref or text"
                );
                continue;
            }
            let text = hkask_memory::text_chunking::sanitize_text(&text);
            chunks.push((entity_ref, text));
        }

        let total = chunks.len();
        if total == 0 {
            return Err(McpToolError::invalid_argument(
                "chunks_jsonl contains no valid chunks",
            ));
        }

        // Read ontology tags from tagged_jsonl (if provided) and prepend as
        // annotations to chunk text before embedding. This produces
        // ontology-anchored embeddings per INSTRUCTOR (Su et al., 2023).
        // Format: "[golem: metaphor, narrative | pko: analysis] <chunk text>"
        let tag_map: std::collections::HashMap<String, String> =
            if let Some(tagged_path) = tagged_jsonl {
                let map = read_ontology_tags_annotated(tagged_path)?;
                tracing::info!(
                    target: "hkask.mcp.docproc.embed",
                    tags_loaded = map.len(),
                    "Ontology tag annotations loaded for ontology-anchored embedding"
                );
                map
            } else {
                std::collections::HashMap::new()
            };

        // Prepend tag annotations to chunk text for ontology-anchored embedding.
        // Chunks without tags get a neutral [unclassified] prefix to maintain
        // consistent token structure across all embeddings.
        if !tag_map.is_empty() {
            for (entity_ref, text) in chunks.iter_mut() {
                let annotation = tag_map
                    .get(entity_ref)
                    .map(|s| s.as_str())
                    .unwrap_or("[unclassified] ");
                text.insert_str(0, annotation);
            }
        }

        let model_name = model.unwrap_or_else(|| default_embedding_model().to_string());

        let store = crate::helpers::open_memory_store(db_path, passphrase)?;

        let batch = batch_size.max(1);
        let batches: Vec<Vec<(String, String)>> =
            chunks.chunks(batch).map(|c| c.to_vec()).collect();
        let num_batches = batches.len();

        // Concurrent embedding: one task per batch, gated by the ADAPTIVE
        // limiter (AIMD: floor 2, +1 per success, halve per failure). The
        // ceiling is HKASK_MAX_CONCURRENCY — injected from
        // KaskGeneralSettings.max_concurrency (default 96, configurable via
        // the settings UI General page), the same ceiling the zed-side
        // inference port uses. The ramp means an embedding provider with
        // lower capacity is probed, not stampeded.
        let limiter = AdaptiveLimiter::new(crate::max_concurrency(), ADAPTIVE_CONCURRENCY_FLOOR);
        let router = Arc::clone(&self.inference_router);
        let store = Arc::new(store);
        let model_name = Arc::new(model_name.clone());

        let embedded = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let failed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // Collect (entity_ref, text, embedding) for in-memory index
        // population after all batches complete.
        let indexed_passages: Arc<Mutex<Vec<IndexedPassage>>> =
            Arc::new(Mutex::new(Vec::with_capacity(total)));

        let mut join_set = tokio::task::JoinSet::new();

        for (batch_idx, chunk_batch) in batches.into_iter().enumerate() {
            let limiter = limiter.clone();
            let router = Arc::clone(&router);
            let store = Arc::clone(&store);
            let model_name = Arc::clone(&model_name);
            let embedded = Arc::clone(&embedded);
            let failed = Arc::clone(&failed);
            let indexed_passages = Arc::clone(&indexed_passages);
            let batch_len = chunk_batch.len();

            join_set.spawn(async move {
                let slot = limiter.acquire().await;

                let batch_texts: Vec<String> = chunk_batch.iter().map(|c| c.1.clone()).collect();
                let vectors = match retry_with_backoff(
                    MAX_RETRIES,
                    "hkask.mcp.docproc.embed",
                    &format!("batch {batch_idx} of {batch_len}"),
                    || router.embed(&model_name, &batch_texts),
                )
                .await
                {
                    Ok(v) => {
                        slot.report_success();
                        v
                    }
                    Err(e) => {
                        slot.report_failure();
                        tracing::warn!(
                            target: "hkask.mcp.docproc.embed",
                            batch = batch_idx,
                            error = %e,
                            "Batch {batch_idx} failed after retries"
                        );
                        failed.fetch_add(batch_len, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                };

                if vectors.is_empty() {
                    failed.fetch_add(batch_len, std::sync::atomic::Ordering::Relaxed);
                    return;
                }

                for (c, vector) in chunk_batch.iter().zip(vectors.iter()) {
                    if let Err(e) = store.store_embedding(&c.0, vector, &model_name, Some(&c.1)) {
                        failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if failed.load(std::sync::atomic::Ordering::Relaxed) <= 5 {
                            tracing::warn!(
                                target: "hkask.mcp.docproc.embed",
                                entity = %c.0,
                                error = %e,
                                "Failed to store embedding"
                            );
                        }
                        continue;
                    }
                    embedded.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    // Collect for in-memory index.
                    if let Ok(mut idx) = indexed_passages.lock() {
                        idx.push(IndexedPassage {
                            text: c.1.clone(),
                            metadata: json!({"entity_ref": &c.0}),
                            embedding: vector.clone(),
                        });
                    }
                }
            });
        }

        // Wait for all batch tasks to complete.
        while let Some(res) = join_set.join_next().await {
            if let Err(e) = res {
                tracing::warn!(
                    target: "hkask.mcp.docproc.embed",
                    error = %e,
                    "Embedding batch task join failed"
                );
            }
        }

        let embedded = embedded.load(std::sync::atomic::Ordering::Relaxed);
        let failed = failed.load(std::sync::atomic::Ordering::Relaxed);

        // Populate the in-memory vector index.
        let mut passages = indexed_passages.lock().unwrap_or_else(|e| e.into_inner());
        let passages = std::mem::take(&mut *passages);
        if !passages.is_empty() {
            let mut index = self.index.lock().unwrap_or_else(|e| e.into_inner());
            let count = passages.len();
            index.extend(passages);
            tracing::info!(
                target: "hkask.mcp.docproc.embed",
                indexed = index.len(),
                "In-memory index populated with {count} passages"
            );
        }

        tracing::info!(
            target: "hkask.mcp.docproc.embed",
            total, embedded, failed, num_batches, ceiling = crate::max_concurrency(),
            "Embedding complete"
        );

        let result = json!({
            "total": total,
            "embedded": embedded,
            "failed": failed,
            "model": model_name,
        });
        let outcome = BatchOutcome::from_counts(failed, total);
        outcome.log_if_degraded("hkask.mcp.docproc.embed", "Embedding");
        Ok(result)
    }
}

// ── Request structs ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GenerateQaRequest {
    /// Single chunk text (mutually exclusive with texts for multi-chunk cross-reference)
    #[serde(default)]
    pub text: Option<String>,
    /// Multiple chunks for cross-reference QA generation (RA-DIT method).
    /// When set, generates QAs that require synthesizing across all passages.
    #[serde(default)]
    pub texts: Option<Vec<String>>,
    pub chunk_id: String,
    #[serde(default)]
    pub bloom_levels: Option<Vec<String>>,
    /// Optional provider-prefixed generation model (for example, `OpenRouter/openai/gpt-5.6-terra`).
    /// When absent, uses `HKASK_QA_MODEL`, then `HKASK_DEFAULT_MODEL`.
    #[serde(default)]
    pub model: Option<String>,
}

/// A single prompt spec parsed from prompts_jsonl for batch QA generation.
/// Internal to the batch tool — not part of the public request schema.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GenerateQaBatchRequest {
    /// Path to prompts JSONL file (one JSON per line with chunk_ref, qa_type, system, user).
    pub prompts_jsonl: String,
    /// Output path for generated QAs JSONL.
    pub output: String,
    /// Max concurrent LLM calls.
    #[serde(default = "default_batch_concurrency")]
    pub concurrency: usize,
    /// Optional provider-prefixed generation model for every prompt in this batch.
    #[serde(default)]
    pub model: Option<String>,
}

fn default_batch_concurrency() -> usize {
    crate::max_concurrency()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ExtractAssertionsRequest {
    /// Path to chunks JSONL for batch processing. Reads (entity_ref, text) per line.
    pub chunks_jsonl: String,
    /// Path to tagged chunks JSONL (from corpus_tag_chunks). When provided,
    /// ontology tags are injected into the extraction prompt so the LLM uses
    /// the appropriate predicates (GOLEM for narrative, schema.org for expository).
    #[serde(default)]
    pub tagged_jsonl: Option<String>,
    /// Path to the SQLCipher memory DB for h_mem storage.
    pub db_path: String,
    /// Passphrase for the memory DB.
    #[serde(default = "default_corpus_passphrase")]
    pub passphrase: String,
    /// Maximum h_mems to extract per chunk (default 15).
    #[serde(default = "default_max_assertions")]
    pub max_assertions: usize,
    /// Owner persona for stored h_mems (e.g. "john-brooks").
    #[serde(default = "default_owner")]
    pub owner: String,
    /// Max concurrent LLM calls for batch processing (default 64).
    #[serde(default = "default_assertions_concurrency")]
    pub concurrency: usize,
}

fn default_max_assertions() -> usize {
    15
}

fn default_assertions_concurrency() -> usize {
    crate::max_concurrency()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmbedRequest {
    /// Path to chunks JSONL (entity_ref, source, text, word_count per line).
    pub chunks_jsonl: String,
    /// Path to tagged chunks JSONL (from corpus_tag_chunks). When provided,
    /// ontology tags are prepended to chunk text before embedding, producing
    /// ontology-anchored embeddings (per INSTRUCTOR, Su et al. 2023).
    /// Requires tag to run before embed.
    #[serde(default)]
    pub tagged_jsonl: Option<String>,
    /// Path to the SQLCipher memory DB for vector storage.
    pub db_path: String,
    /// Passphrase for the memory DB.
    #[serde(default = "default_corpus_passphrase")]
    pub passphrase: String,
    /// Embedding model to use. If not set, uses the configured default.
    #[serde(default)]
    pub model: Option<String>,
    /// Batch size for embedding API calls (default 50).
    #[serde(default = "default_embed_batch_size")]
    pub batch_size: usize,
}

fn default_embed_batch_size() -> usize {
    50
}
