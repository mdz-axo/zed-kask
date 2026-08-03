//! Semantic extraction tools — QA generation, h_mem extraction, embedding.
//!
//! This module is the router host for the `semantic_router` tool group.
//! Helpers live in submodules:
//! - `qa` — QA response parsing, batch writer, model resolution
//! - `triples` — RDF predicate → 5W1H dimension mapping
//! - `ontology_io` — tagged-chunks JSONL readers
//!
//! The `#[tool_router]` macro requires all `#[tool]` methods to be on a single
//! `impl CorpusServer` block, so the tool methods stay here in `mod.rs`.

mod ontology_io;
mod qa;
mod triples;

use crate::batch::{BatchOutcome, MAX_RETRIES, retry_with_backoff};
use crate::services::triples::{TriplesRequest, TriplesService};
use crate::*;
use ontology_io::read_ontology_tags_annotated;
use qa::{BatchQaPrompt, parse_qa_response, write_qa_result};
use schemars::JsonSchema;
use serde::Deserialize;
use std::io::Write;

// Re-export the shared content guard (now in `crate::guard`) for backward
// compatibility with callers that historically imported it from
// `crate::tools::semantic`. This also brings `GUARD` / `INPUT_GUARD_ENABLED`
// into scope for the tool methods below.
pub(crate) use crate::guard::{GUARD, INPUT_GUARD_ENABLED};

// Re-export helpers used by other tool modules (corpus.rs imports these) and
// make them available within this module via the module path.
pub(crate) use ontology_io::read_ontology_namespaces;
pub(crate) use ontology_io::read_ontology_tags;
pub(crate) use qa::configured_qa_model;
pub(crate) use triples::{predicate_to_dimension, triple_confidence};

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
    ) -> String {
        execute_tool(self, "corpus_generate_qa", async {
            let is_cross_ref = _texts.as_ref().is_some_and(|t| !t.is_empty());
            let single_text = _text.unwrap_or_default();

            if !is_cross_ref && single_text.is_empty() {
                return Err(McpToolError::invalid_argument("text must not be empty (or set texts for cross-reference mode)"));
            }
            if chunk_id.is_empty() {
                return Err(McpToolError::invalid_argument("chunk_id must not be empty"));
            }

            let levels = bloom_levels
                .unwrap_or_else(|| vec!["factual".to_string(), "conceptual".to_string()]);
            let levels_str = levels.join(", ");

            let (prompt, template_source) = if is_cross_ref {
                let passages = _texts.as_ref().unwrap();
                let mut text = String::new();
                for (i, p) in passages.iter().enumerate() {
                    text.push_str(&format!("[Passage {}]\n{}\n\n", i + 1, p));
                }
                (
                    format!(
                        "You are synthesizing knowledge across {} passages.\n\nGenerate question-answer pairs at these Bloom's taxonomy levels: {levels_str}.\n\nThe questions should require synthesizing information from MULTIPLE passages — compare, contrast, diagnose patterns, or trace causal connections across sources.\n\nFor each QA, cite which passages support the answer (e.g., 'Per Passage 1, ... while Passage 2 notes ...').\n\nPassages (chunk group {chunk_id}):\n{text}\n\nRespond in JSON: {{\"qa_pairs\": [{{\"question\": \"...\", \"answer\": \"...\", \"bloom_level\": \"...\", \"sources\": [1, 3]}}]}}",
                        passages.len()
                    ),
                    "inline-cross-reference",
                )
            } else {
                let mut vars: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
                vars.insert("levels", levels_str.clone());
                vars.insert("chunk_id", chunk_id.clone());
                vars.insert("text", single_text.clone());
                let tpl = render_docproc_template("generate-qa", &vars);
                if tpl.is_empty() {
                    (
                        format!(
                            "Based on the following text, generate question-answer pairs at these Bloom's taxonomy levels: {levels_str}.\n\nText (chunk {chunk_id}):\n{single_text}\n\nFor each level, provide question, answer, and bloom_level.\nRespond in JSON: {{\"qa_pairs\": [{{\"question\": \"...\", \"answer\": \"...\", \"bloom_level\": \"...\"}}]}}"
                        ),
                        "inline-fallback",
                    )
                } else {
                    (tpl, "registry/templates/docproc/generate-qa.j2")
                }
            };
            let selected_model = configured_qa_model(model);

            let params = LLMParameters {
                temperature: 0.3,
                top_p: 0.95,
                max_tokens: 4096,
                frequency_penalty: 0.0,
                presence_penalty: 0.0,
                top_k: 0,
                min_p: 0.0,
                typical_p: 0.0,
                disable_thinking: true,
                ..Default::default()
            };

            // P3.1: input guard — scan prompt before model invocation. The output
            // guard (secret stripping) is always active; input scanning guards
            // interactive boundaries from untrusted input. The corpus pipeline
            // may disable it via HKASK_ENABLE_CONTENT_GUARD (curated literature).
            if *INPUT_GUARD_ENABLED {
                let input_scan = GUARD.scan_input(&prompt);
                if !input_scan.passed {
                    let violations: Vec<String> = input_scan.violations.iter()
                        .map(|v| format!("{}: {}", v.scanner, v.description))
                        .collect();
                    return Err(McpToolError::invalid_argument(format!(
                        "Input guard rejected prompt: {}", violations.join("; ")
                    )));
                }
            }

            match self
                .inference_router
                .generate_with_model(&prompt, &params, selected_model.as_deref(), None)
                .await
            {
                Ok(response) => {
                    let output_scan = GUARD.scan_output(&response.text);
                    let content = output_scan.output.content(&response.text);
                    if !output_scan.passed {
                        tracing::warn!(
                            target: "reg.guard",
                            violations = ?output_scan.violations.iter().map(|v| &v.scanner).collect::<Vec<_>>(),
                            "Output guard violations in QA generation — content may be sanitized"
                        );
                    }
                    let qa_response = parse_qa_response(
                        &extract_json_from_response(content),
                        &levels,
                        is_cross_ref.then(|| _texts.as_ref().map_or(0, Vec::len)),
                    )
                    .map_err(|e| McpToolError::internal(e.to_string()))?;
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
        description = "Batch-generate QA pairs from multiple text chunks. Same pipeline as corpus_generate_qa (Bloom taxonomy, ContentGuard, templates). Uses configurable concurrency for parallel LLM calls. Reads prompts from prompts_jsonl (one JSON per line: chunk_ref, qa_type, system, user) and writes generated QAs to the output JSONL file. Returns a summary (total + written counts)."
    )]
    pub async fn corpus_generate_qa_batch(
        &self,
        Parameters(GenerateQaBatchRequest {
            prompts_jsonl,
            output,
            concurrency,
            model,
        }): Parameters<GenerateQaBatchRequest>,
    ) -> String {
        execute_tool(self, "corpus_generate_qa_batch", async {
            // Read prompts from JSONL file (file-only mode)
            let prompts_values =
                read_jsonl::<serde_json::Value>(&prompts_jsonl, "prompts_jsonl")?;
            let mut prompts_vec: Vec<BatchQaPrompt> = Vec::new();
            for v in prompts_values {
                // Map build_prompts output fields to BatchQaPrompt:
                // chunk_ref -> chunk_id, system+user -> text, qa_type -> bloom_levels
                let chunk_id = v
                    .get("chunk_ref")
                    .and_then(|v| v.as_str())
                    .or_else(|| v.get("chunk_id").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();
                let system = v.get("system").and_then(|v| v.as_str()).unwrap_or("");
                let user = v.get("user").and_then(|v| v.as_str()).unwrap_or("");
                let text = if system.is_empty() && user.is_empty() {
                    v.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string()
                } else {
                    format!("{system}\n\n{user}")
                };
                let bloom_levels = v
                    .get("qa_type")
                    .and_then(|v| v.as_str())
                    .map(|qt| vec![qt.to_string()])
                    .or_else(|| {
                        v.get("bloom_levels").and_then(|v| v.as_array()).map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect()
                        })
                    });
                let source = v
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let concepts = v
                    .get("concepts")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                prompts_vec.push(BatchQaPrompt {
                    text,
                    chunk_id,
                    bloom_levels,
                    source,
                    concepts,
                });
            }

            if prompts_vec.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "prompts_jsonl contains no valid prompts",
                ));
            }

            let selected_model = configured_qa_model(model);
            let total = prompts_vec.len();

            // Concurrent processing with configurable semaphore
            let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
            let router = Arc::clone(&self.inference_router);

            // Output file writer (with incremental flush every 10 completions)
            let output_path = crate::path_safety::contain_for_write(&output)?;
            let file = std::fs::File::create(&output_path).map_err(|e| {
                McpToolError::internal(format!(
                    "Cannot create output file '{}': {e}",
                    output
                ))
            })?;
            let output_writer = Arc::new(Mutex::new(std::io::BufWriter::new(file)));
            let write_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            // B5 fix: track failed prompts so the outcome can be classified as
            // degraded when the failure rate exceeds the threshold.
            let failed_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

            let mut handles = Vec::with_capacity(total);
            for prompt in prompts_vec {
                let router = Arc::clone(&router);
                let sem = Arc::clone(&sem);
                let selected_model = selected_model.clone();
                let output_writer = Arc::clone(&output_writer);
                let write_count = Arc::clone(&write_count);
                let failed_count = Arc::clone(&failed_count);

                let handle = tokio::spawn(async move {
                    let _permit = sem.acquire().await;

                    let levels = prompt.bloom_levels.clone().unwrap_or_else(|| vec!["factual".to_string(), "conceptual".to_string()]);
                    let levels_str = levels.join(", ");
                    let mut vars: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
                    vars.insert("levels", levels_str.clone());
                    vars.insert("chunk_id", prompt.chunk_id.clone());
                    vars.insert("text", prompt.text.clone());
                    let tpl = render_docproc_template("generate-qa", &vars);
                    let (prompt_text, template_source) = if tpl.is_empty() {
                        (
                            format!("Based on the following text, generate question-answer pairs at these Bloom's taxonomy levels: {levels_str}.\n\nText (chunk {chunk_id}):\n{text}\n\nFor each level, provide question, answer, and bloom_level.\nRespond in JSON: {{\"qa_pairs\": [{{\"question\": \"...\", \"answer\": \"...\", \"bloom_level\": \"...\"}}]}}",
                                levels_str = levels_str,
                                chunk_id = prompt.chunk_id,
                                text = prompt.text
                            ),
                            "inline-fallback",
                        )
                    } else {
                        (tpl, "registry/templates/docproc/generate-qa.j2")
                    };
                    if *INPUT_GUARD_ENABLED {
                        let input_scan = GUARD.scan_input(&prompt_text);
                        if !input_scan.passed {
                            failed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let result = json!({"chunk_id": prompt.chunk_id, "error": "Input guard rejected"});
                            write_qa_result(&result, &output_writer, &write_count);
                            return;
                        }
                    }
                    let params = LLMParameters { temperature: 0.3, top_p: 0.95, max_tokens: 4096, frequency_penalty: 0.0, presence_penalty: 0.0, top_k: 0, min_p: 0.0, typical_p: 0.0, disable_thinking: true, ..Default::default() };
                    let response = match retry_with_backoff(
                        MAX_RETRIES,
                        "hkask.mcp.docproc.qa_batch",
                        &prompt.chunk_id,
                        || router.generate_with_model(&prompt_text, &params, selected_model.as_deref(), None),
                    )
                    .await
                    {
                        Ok(resp) => resp,
                        Err(e) => {
                            let result = json!({"chunk_id": prompt.chunk_id, "error": format!("LLM failed after {} retries: {}", MAX_RETRIES, e)});
                            failed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            write_qa_result(&result, &output_writer, &write_count);
                            return;
                        }
                    };
                    // Process the successful response — same logic as before,
                    // but now guaranteed to have a response (or we returned above).
                    let output_scan = GUARD.scan_output(&response.text);
                    let content = output_scan.output.content(&response.text);
                    match parse_qa_response(&extract_json_from_response(content), &levels, None) {
                        Ok(qa_response) => {
                            // Write one JSONL line per QA pair in envelope format
                            // (matches what corpus_ingest_qa's parse_qa_record expects)
                            for pair in qa_response.qa_pairs {
                                let result = json!({
                                    "chunk_ref": prompt.chunk_id,
                                    "source": prompt.source,
                                    "qa_type": pair.bloom_level,
                                    "response": {
                                        "instruction": pair.question,
                                        "output": pair.answer,
                                        "type": pair.bloom_level,
                                        "concepts": prompt.concepts,
                                    },
                                    "provenance": {
                                        "generator_model": selected_model.as_deref().unwrap_or("router_default"),
                                        "prompt_template": template_source,
                                        "source_chunk_ref": prompt.chunk_id,
                                    },
                                    "tokens_used": response.usage.total_tokens,
                                });
                                write_qa_result(&result, &output_writer, &write_count);
                            }
                        }
                        Err(e) => {
                            failed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let result = json!({
                                "chunk_id": prompt.chunk_id,
                                "error": format!("QA response rejected: {e}"),
                            });
                            write_qa_result(&result, &output_writer, &write_count);
                        }
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                let _ = handle.await;
            }

            {
                let mut w = output_writer.lock().unwrap();
                let _ = w.flush();
            }
            let written = write_count.load(std::sync::atomic::Ordering::Relaxed);
            let failed = failed_count.load(std::sync::atomic::Ordering::Relaxed);
            let result = json!({
                "total": total,
                "written": written,
                "failed": failed,
                "output": output,
            });
            // B5 fix: report degraded outcome when failure rate exceeds threshold.
            let outcome = BatchOutcome::from_counts(failed, total);
            outcome.log_if_degraded("hkask.mcp.docproc.qa_batch", "QA batch");
            Ok(result)
        }).await
    }

    #[tool(
        description = "Extract RDF h_mems (subject, predicate, object) from text using the inference engine. Uses the canonical classifier model (HKASK_CLASSIFIER_MODEL, default Qwen3-235B-A22B-Instruct on DeepInfra) with 3-attempt retry. Reads chunks from chunks_jsonl, processes them concurrently, and stores triples as h_mems in the memory DB with entity=entity_ref from each chunk. When tagged_jsonl is provided, ontology tags from the tagging step are injected to guide predicate selection (GOLEM for narrative, schema.org for expository). Returns a summary (total_chunks, succeeded, failed, h_mems_stored)."
    )]
    pub async fn corpus_extract_triples(
        &self,
        Parameters(ExtractTriplesRequest {
            chunks_jsonl,
            tagged_jsonl,
            db_path,
            passphrase,
            max_triples,
            owner,
            concurrency,
        }): Parameters<ExtractTriplesRequest>,
    ) -> String {
        execute_tool(self, "corpus_extract_triples", async {
            TriplesService::new(Arc::clone(&self.inference_router))
                .extract(TriplesRequest {
                    chunks_jsonl,
                    tagged_jsonl,
                    max_triples,
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
    ) -> String {
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
    /// in groups of `batch_size`, stores each vector + text/provenance h_mem in the
    /// DB, and returns a summary (no inline vectors — too large for 33K chunks).
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

        let model_name = model.unwrap_or_else(hkask_inference::model_constants::embedding_model);

        let dim = embedding_dim();
        let semantic =
            hkask_memory::SemanticMemory::open(db_path, passphrase, dim).map_err(|e| {
                McpToolError::failed_precondition(format!("Cannot open memory DB: {e}"))
            })?;

        let mut embedded = 0usize;
        let mut failed = 0usize;
        let batch = batch_size.max(1);

        for chunk_batch in chunks.chunks(batch) {
            let batch_texts: Vec<String> = chunk_batch.iter().map(|c| c.1.clone()).collect();
            let vectors = match retry_with_backoff(
                MAX_RETRIES,
                "hkask.mcp.docproc.embed",
                &format!("batch of {}", chunk_batch.len()),
                || self.inference_router.embed(&model_name, &batch_texts),
            )
            .await
            {
                Ok(v) => v,
                Err(_) => {
                    failed += chunk_batch.len();
                    Vec::new()
                }
            };
            if vectors.is_empty() {
                continue;
            }
            for (c, vector) in chunk_batch.iter().zip(vectors.iter()) {
                // Store embedding vector only — text and provenance h_mems were
                // removed as orphans (no downstream pipeline tool consumed them).
                if let Err(e) = semantic.store_embedding(&c.0, vector, &model_name) {
                    failed += 1;
                    if failed <= 5 {
                        tracing::warn!(
                            target: "hkask.mcp.docproc.embed",
                            entity = %c.0,
                            error = %e,
                            "Failed to store embedding"
                        );
                    }
                    continue;
                }
                embedded += 1;
            }
        }

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
pub struct GenerateQaRequest {
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
pub struct GenerateQaBatchRequest {
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
    4
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExtractTriplesRequest {
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
    #[serde(default = "default_docproc_passphrase")]
    pub passphrase: String,
    /// Maximum h_mems to extract per chunk (default 15).
    #[serde(default = "default_max_triples")]
    pub max_triples: usize,
    /// Owner persona for stored h_mems (e.g. "john-brooks").
    #[serde(default = "default_owner")]
    pub owner: String,
    /// Max concurrent LLM calls for batch processing (default 64).
    #[serde(default = "default_triples_concurrency")]
    pub concurrency: usize,
}

fn default_max_triples() -> usize {
    15
}

fn default_triples_concurrency() -> usize {
    64
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
    #[serde(default = "default_docproc_passphrase")]
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

/// Default passphrase for the docproc memory DB.
///
/// `tools::storage::default_purge_passphrase` is private to that module, so this
/// module-local default mirrors it for `ExtractTriplesRequest` and `EmbedRequest`.
fn default_docproc_passphrase() -> String {
    // Env-driven with a dev fallback: production sets HKASK_DB_PASSPHRASE;
    // local dev (env unset) falls back to the shared dev passphrase so the
    // corpus pipeline runs without extra config. The pipeline YAML no longer
    // hardcodes the passphrase per-step (F12 — no hardcoded secrets).
    std::env::var("HKASK_DB_PASSPHRASE")
        .unwrap_or_else(|_| "hkask-default-passphrase-2024".to_string())
}
