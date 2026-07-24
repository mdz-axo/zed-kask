//! Semantic extraction tools — QA generation, h_mem extraction, embedding.
//!
//! This module is the router host for the `semantic_router` tool group.
//! Helpers live in submodules:
//! - `qa` — QA response parsing, batch writer, model resolution
//! - `triples` — RDF predicate → 5W1H dimension mapping
//! - `ontology_io` — tagged-chunks JSONL readers
//!
//! The `#[tool_router]` macro requires all `#[tool]` methods to be on a single
//! `impl DocProcServer` block, so the tool methods stay here in `mod.rs`.

mod ontology_io;
mod qa;
mod triples;

use crate::*;
use ontology_io::{read_ontology_namespaces, read_ontology_tags_annotated};
use qa::{BatchQaPrompt, parse_qa_response, write_qa_result};
use schemars::JsonSchema;
use serde::Deserialize;
use std::io::Write;

/// Failure-rate threshold (percent) above which embedding and QA-batch runs
/// report `degraded` outcome. Matches the threshold used by `docproc_tag_chunks`.
/// A run exceeding this rate indicates systemic issues (model unavailable,
/// rate limiting, adversarial input) and must not be reported as `success`.
const DEGRADED_FAILURE_THRESHOLD: usize = 10;

/// Maximum LLM retry attempts for batch QA generation. Matches the 3-attempt
/// pattern used by `docproc_tag_chunks` and `docproc_extract_triples`.
const QA_BATCH_MAX_RETRIES: u32 = 3;

// Content safety guard — mandatory at every LLM boundary (OWASP LLM01/02/04/06).
// The output pipeline (secret stripping) is ALWAYS active — secrets must never
// enter shared memory (P3.1 floor). The input pipeline (prompt injection / role
// override) protects interactive agent boundaries from untrusted user input.
// For the docproc corpus curation pipeline, which processes operator-curated
// literature rather than untrusted user input, the operator may disable input
// scanning via `HKASK_ENABLE_CONTENT_GUARD=false`. Defaults to enabled.
pub(crate) static GUARD: std::sync::LazyLock<hkask_guard::ContentGuard> =
    std::sync::LazyLock::new(|| {
        hkask_guard::ContentGuard::mandatory(&hkask_guard::GuardConfig::default())
    });

/// Whether input-guard scanning is active for the docproc corpus pipeline.
///
/// Read once per process from `HKASK_ENABLE_CONTENT_GUARD`. Unset or any value
/// other than `false`/`0`/`off`/`no` leaves it enabled (safe default). The output
/// guard (`scan_output`) is always invoked regardless of this flag — secrets
/// must never enter shared memory.
pub(crate) static INPUT_GUARD_ENABLED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
    !matches!(
        std::env::var("HKASK_ENABLE_CONTENT_GUARD")
            .ok()
            .map(|v| v.to_ascii_lowercase())
            .as_deref(),
        Some("false" | "0" | "off" | "no")
    )
});

// Re-export helpers used by other tool modules (corpus.rs imports these) and
// make them available within this module via the module path.
pub(crate) use ontology_io::read_ontology_tags;
pub(crate) use qa::configured_qa_model;
pub(crate) use triples::predicate_to_dimension;

#[tool_router(router = semantic_router, vis = "pub")]
impl DocProcServer {
    #[tool(
        description = "Generate QA pairs from text chunks. Accepts a single chunk (text) or multiple chunks (texts) for cross-reference synthesis. Uses Bloom's taxonomy levels. Multi-chunk mode (texts) generates QAs that require synthesizing across all passages with source citation."
    )]
    pub async fn docproc_generate_qa(
        &self,
        Parameters(GenerateQaRequest {
            text: _text,
            texts: _texts,
            chunk_id,
            bloom_levels,
            model,
        }): Parameters<GenerateQaRequest>,
    ) -> String {
        execute_tool(self, "docproc_generate_qa", async {
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
                    self.record_experience("docproc_generate_qa", &chunk_id, "success", result.clone());
                    Ok(result)
                }
                Err(e) => Err(McpToolError::unavailable(format!("QA generation failed: {}", e))),
            }
        })
        .await
    }

    #[tool(
        description = "Batch-generate QA pairs from multiple text chunks. Same pipeline as docproc_generate_qa (Bloom taxonomy, ContentGuard, templates). Uses configurable concurrency for parallel LLM calls. Reads prompts from prompts_jsonl (one JSON per line: chunk_ref, qa_type, system, user) and writes generated QAs to the output JSONL file. Returns a summary (total + written counts)."
    )]
    pub async fn docproc_generate_qa_batch(
        &self,
        Parameters(GenerateQaBatchRequest {
            prompts_jsonl,
            output,
            concurrency,
            model,
        }): Parameters<GenerateQaBatchRequest>,
    ) -> String {
        execute_tool(self, "docproc_generate_qa_batch", async {
            // Read prompts from JSONL file (file-only mode)
            let content = std::fs::read_to_string(&prompts_jsonl).map_err(|e| {
                McpToolError::invalid_argument(format!(
                    "Cannot read prompts_jsonl '{}': {e}",
                    prompts_jsonl
                ))
            })?;
            let mut prompts_vec: Vec<BatchQaPrompt> = Vec::new();
            for (i, line) in content.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let v: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                    McpToolError::invalid_argument(format!(
                        "prompts_jsonl line {} is not valid JSON: {e}",
                        i + 1
                    ))
                })?;
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
            let file = std::fs::File::create(&output).map_err(|e| {
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
                    // B5 fix: retry with exponential backoff (3 attempts) — matches the
                    // pattern in docproc_tag_chunks and docproc_extract_triples.
                    // Without this, a transient network error or rate limit would
                    // cause a permanent gap in the QA training set.
                    let mut attempts = 0u32;
                    let response = loop {
                        match router
                            .generate_with_model(&prompt_text, &params, selected_model.as_deref(), None)
                            .await
                        {
                            Ok(resp) => break resp,
                            Err(e) => {
                                attempts += 1;
                                if attempts >= QA_BATCH_MAX_RETRIES {
                                    tracing::warn!(
                                        target: "hkask.mcp.docproc.qa_batch",
                                        chunk_id = %prompt.chunk_id,
                                        attempts = attempts,
                                        error = %e,
                                        "QA generation failed after {} retries",
                                        QA_BATCH_MAX_RETRIES
                                    );
                                    let result = json!({"chunk_id": prompt.chunk_id, "error": format!("LLM failed after {} retries: {}", QA_BATCH_MAX_RETRIES, e)});
                                    failed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    write_qa_result(&result, &output_writer, &write_count);
                                    return;
                                }
                                let backoff = std::time::Duration::from_secs(2u64.pow(attempts) * 5);
                                tracing::warn!(
                                    target: "hkask.mcp.docproc.qa_batch",
                                    chunk_id = %prompt.chunk_id,
                                    attempt = attempts,
                                    backoff_secs = backoff.as_secs(),
                                    error = %e,
                                    "QA generation retry — backing off"
                                );
                                tokio::time::sleep(backoff).await;
                            }
                        }
                    };
                    // Process the successful response — same logic as before,
                    // but now guaranteed to have a response (or we returned above).
                    let output_scan = GUARD.scan_output(&response.text);
                    let content = output_scan.output.content(&response.text);
                    match parse_qa_response(&extract_json_from_response(content), &levels, None) {
                        Ok(qa_response) => {
                            // Write one JSONL line per QA pair in envelope format
                            // (matches what docproc_ingest_qa's parse_qa_record expects)
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
            let failure_pct = if total == 0 { 0 } else { (failed * 100) / total };
            let outcome = if failure_pct >= DEGRADED_FAILURE_THRESHOLD {
                "degraded"
            } else {
                "success"
            };
            if outcome == "degraded" {
                tracing::warn!(
                    target: "hkask.mcp.docproc.qa_batch",
                    failed = failed,
                    total = total,
                    failure_pct = failure_pct,
                    threshold_pct = DEGRADED_FAILURE_THRESHOLD,
                    "QA batch run degraded — failure rate exceeds threshold"
                );
            }
            self.record_experience(
                "docproc_generate_qa_batch",
                &format!("batch: {} prompts", total),
                outcome,
                result.clone(),
            );
            Ok(result)
        }).await
    }

    #[tool(
        description = "Extract RDF h_mems (subject, predicate, object) from text using the inference engine. Uses the canonical classifier model (HKASK_CLASSIFIER_MODEL, default Qwen3-235B-A22B-Instruct on DeepInfra) with 3-attempt retry. Reads chunks from chunks_jsonl, processes them concurrently, and stores triples as h_mems in the memory DB with entity=entity_ref from each chunk. When tagged_jsonl is provided, ontology tags from the tagging step are injected to guide predicate selection (GOLEM for narrative, schema.org for expository). Returns a summary (total_chunks, succeeded, failed, h_mems_stored)."
    )]
    pub async fn docproc_extract_triples(
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
        execute_tool(self, "docproc_extract_triples", async {
            self.extract_triples_batch(
                &chunks_jsonl,
                tagged_jsonl.as_deref(),
                max_triples,
                &db_path,
                &passphrase,
                &owner,
                concurrency,
            )
            .await
        })
        .await
    }

    /// Batch extract h_mems from chunks JSONL with concurrent LLM calls.
    ///
    /// Opens the DB once and shares it across all concurrent tasks via `Arc<SemanticMemory>`.
    /// Each chunk gets a 3-attempt retry with backoff. Triples are stored as h_mems
    /// with `entity = chunk.entity_ref`.
    ///
    /// When `tagged_jsonl` is provided, ontology tags from the tagging step are
    /// read and injected into the extraction prompt per-chunk, so the LLM uses
    /// the appropriate predicates (GOLEM for narrative, schema.org for expository).
    #[allow(clippy::too_many_arguments)]
    async fn extract_triples_batch(
        &self,
        chunks_path: &str,
        tagged_jsonl: Option<&str>,
        max_triples: usize,
        db_path: &str,
        passphrase: &str,
        owner: &str,
        concurrency: usize,
    ) -> Result<serde_json::Value, McpToolError> {
        let content = std::fs::read_to_string(chunks_path).map_err(|e| {
            McpToolError::invalid_argument(format!(
                "Cannot read chunks_jsonl '{}': {e}",
                chunks_path
            ))
        })?;

        // Parse chunks: each line has entity_ref and text
        let mut chunks: Vec<(String, String)> = Vec::new();
        for (i, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                McpToolError::invalid_argument(format!(
                    "chunks_jsonl line {} is not valid JSON: {e}",
                    i + 1
                ))
            })?;
            let entity_ref = v
                .get("entity_ref")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let chunk_text = v
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if entity_ref.is_empty() || chunk_text.is_empty() {
                tracing::warn!(
                    target: "hkask.mcp.docproc.triples",
                    line = i + 1,
                    "Skipping chunk with empty entity_ref or text"
                );
                continue;
            }
            chunks.push((entity_ref, chunk_text));
        }

        let total_chunks = chunks.len();
        if total_chunks == 0 {
            return Err(McpToolError::invalid_argument(
                "chunks_jsonl contains no valid chunks",
            ));
        }

        // Read ontology tags from tagged_jsonl (if provided) to inject into
        // extraction prompts. Maps entity_ref → formatted ontology context.
        let ontology_map: std::collections::HashMap<String, String> =
            if let Some(tagged_path) = tagged_jsonl {
                read_ontology_tags(tagged_path)?
            } else {
                std::collections::HashMap::new()
            };
        let ontology_map = Arc::new(ontology_map);

        // Read ontology namespace sets per chunk (M4 fix). Used to cross-check
        // that a triple's predicate namespace was actually tagged for the
        // chunk before bypassing the text-containment hallucination guard.
        // Without this, any `golem:`/`eso:`/`fibo:`/`pko:` predicate bypasses
        // the guard regardless of whether the chunk was tagged with that
        // ontology — allowing the LLM to emit abstract-namespace predicates
        // for chunks where that ontology was never detected.
        let namespace_map: std::collections::HashMap<String, std::collections::HashSet<String>> =
            if let Some(tagged_path) = tagged_jsonl {
                read_ontology_namespaces(tagged_path)?
            } else {
                std::collections::HashMap::new()
            };
        let namespace_map = Arc::new(namespace_map);

        // Open DB once, share across concurrent tasks
        let dim = embedding_dim();
        let semantic = Arc::new(
            hkask_memory::SemanticMemory::open(db_path, passphrase, dim).map_err(|e| {
                McpToolError::failed_precondition(format!("Cannot open memory DB: {e}"))
            })?,
        );
        let webid = owner_webid(owner);
        let classifier = hkask_inference::model_constants::classifier_model();
        // Namespace is fixed to "doc" for corpus chunk extraction (no longer a request field).
        let ns = "doc".to_string();

        let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
        let router = Arc::clone(&self.inference_router);
        let succeeded = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let failed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h_mems_stored = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let mut handles = Vec::with_capacity(total_chunks);
        for (entity_ref, chunk_text) in chunks {
            let router = Arc::clone(&router);
            let sem = Arc::clone(&sem);
            let semantic = Arc::clone(&semantic);
            let classifier = classifier.clone();
            let ns = ns.clone();
            let succeeded = Arc::clone(&succeeded);
            let failed = Arc::clone(&failed);
            let h_mems_stored = Arc::clone(&h_mems_stored);
            let ontology_map = Arc::clone(&ontology_map);
            let namespace_map = Arc::clone(&namespace_map);

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await;

                // Build prompt from registry template
                let ontology_context = ontology_map.get(&entity_ref).cloned().unwrap_or_default();
                // Namespace set for this chunk (M4 cross-check). Empty if no
                // tagged_jsonl was provided or the chunk has no ontology tags.
                let chunk_namespaces = namespace_map.get(&entity_ref).cloned().unwrap_or_default();
                let mut vars: std::collections::HashMap<&str, String> =
                    std::collections::HashMap::new();
                vars.insert("limit", max_triples.to_string());
                vars.insert("namespace", ns.clone());
                vars.insert("text", chunk_text.clone());
                vars.insert("ontology_context", ontology_context.clone());
                let prompt = render_docproc_template("extract-hmems", &vars);
                let prompt = if prompt.is_empty() {
                    // Fallback: includes GOLEM predicates and ontology context if available
                    let ontology_hint = if ontology_context.is_empty() {
                        String::new()
                    } else {
                        format!("

Ontology tags for this passage: {ontology_context}
Use GOLEM predicates (golem:hasCharacter, golem:hasEvent, golem:hasTheme, golem:illustrates, etc.) for narrative passages and standard RDF predicates (schema:author, rdf:type, etc.) for expository passages.")
                    };
                    format!(
                        "Extract up to {max_triples} factual RDF triples from the following text.

First, classify the passage as narrative (story, characters, literary devices) or expository (concepts, analysis, arguments). Then extract triples using the appropriate predicates:
  - Expository: schema:author, schema:mentions, rdf:type, fibo:returnOnCapital, etc.
  - Narrative: golem:hasCharacter, golem:hasEvent, golem:hasTheme, golem:illustrates, golem:metaphorFor, etc.

Each triple: (subject, predicate, object, confidence). Prefix subjects with '{ns}:'.{ontology_hint}

Text:
{chunk_text}

Respond in JSON format: {{\"h_mems\": [{{\"subject\": \"...\", \"predicate\": \"...\", \"object\": \"...\", \"confidence\": 0.95}}]}}"
                    )
                } else {
                    prompt
                };

                // Input guard — operator may disable via HKASK_ENABLE_CONTENT_GUARD
                if *INPUT_GUARD_ENABLED {
                    let input_scan = GUARD.scan_input(&prompt);
                    if !input_scan.passed {
                        tracing::warn!(
                            target: "hkask.mcp.docproc.triples",
                            entity = %entity_ref,
                            "Input guard rejected extraction prompt"
                        );
                        failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                }

                let params = LLMParameters {
                    temperature: 0.1,
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

                // 3-attempt retry with backoff
                let mut attempts = 0u32;
                let response = loop {
                    match router
                        .generate_with_model(&prompt, &params, Some(&classifier), None)
                        .await
                    {
                        Ok(resp) => break resp,
                        Err(e) => {
                            attempts += 1;
                            if attempts >= 3 {
                                tracing::warn!(
                                    target: "hkask.mcp.docproc.triples",
                                    entity = %entity_ref,
                                    error = %e,
                                    "HMem extraction failed after 3 retries"
                                );
                                failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                return;
                            }
                            let backoff = std::time::Duration::from_secs(2u64.pow(attempts) * 5);
                            tracing::warn!(
                                target: "hkask.mcp.docproc.triples",
                                entity = %entity_ref,
                                attempt = attempts,
                                backoff_secs = backoff.as_secs(),
                                error = %e,
                                "HMem extraction retry — backing off"
                            );
                            tokio::time::sleep(backoff).await;
                        }
                    }
                };

                // Output guard + JSON extraction
                let output_scan = GUARD.scan_output(&response.text);
                let content = output_scan.output.content(&response.text);
                if !output_scan.passed {
                    tracing::warn!(
                        target: "reg.guard",
                        entity = %entity_ref,
                        violations = ?output_scan.violations.iter().map(|v| &v.scanner).collect::<Vec<_>>(),
                        "Output guard violations in h_mem extraction — content may be sanitized"
                    );
                }
                let cleaned = extract_json_from_response(content);
                let h_mems: serde_json::Value = match serde_json::from_str(&cleaned) {
                    Ok(v) => v,
                    Err(_) => {
                        tracing::warn!(
                            target: "hkask.mcp.docproc.triples",
                            entity = %entity_ref,
                            "LLM response was not valid JSON"
                        );
                        failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                };

                // Store triples as h_mems — preserve subject in value for knowledge graph
                let mut stored = 0usize;
                if let Some(arr) = h_mems.get("h_mems").and_then(|v| v.as_array()) {
                    for triple in arr {
                        let subject = triple.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                        let predicate = triple
                            .get("predicate")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let object = triple.get("object").cloned().unwrap_or(json!(null));
                        let raw_confidence = triple
                            .get("confidence")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.8);
                        let dimension = predicate_to_dimension(predicate);

                        // Gap 5: Hallucination verification — check if subject and
                        // object strings appear in the chunk text. Skip the check
                        // for abstract-namespace predicates (golem/eso/fibo/pko)
                        // where interpretive concepts are expected. Cap at 0.5
                        // (not 0.3 — too aggressive).
                        //
                        // M4 fix: the bypass only applies if the predicate's
                        // namespace was actually tagged for this chunk. Without
                        // this cross-check, the LLM could emit any `golem:`/
                        // `eso:`/`fibo:`/`pko:` predicate to bypass the guard
                        // for chunks where that ontology was never detected —
                        // allowing hallucinated triples to enter the knowledge
                        // graph at full LLM-reported confidence.
                        let pred_ns = predicate.split(':').next().unwrap_or("").to_lowercase();
                        let is_abstract_ns = matches!(
                            pred_ns.as_str(),
                            "golem" | "eso" | "fibo" | "pko" | "epistemic" | "omc" | "other"
                        );
                        let namespace_tagged =
                            !chunk_namespaces.is_empty() && chunk_namespaces.contains(&pred_ns);
                        let is_abstract = is_abstract_ns && namespace_tagged;
                        let confidence = if is_abstract {
                            raw_confidence
                        } else {
                            let text_lower = chunk_text.to_lowercase();
                            let subj_clean = subject
                                .strip_prefix("doc:")
                                .unwrap_or(subject)
                                .to_lowercase();
                            let subj_in_text =
                                !subj_clean.is_empty() && text_lower.contains(&subj_clean);
                            let obj_str = match &object {
                                serde_json::Value::String(s) => s.to_lowercase(),
                                _ => String::new(),
                            };
                            let obj_in_text = obj_str.is_empty() || text_lower.contains(&obj_str);
                            if (!subj_in_text || !obj_in_text) && raw_confidence > 0.5 {
                                let reason = if is_abstract_ns && !namespace_tagged {
                                    format!(
                                        "abstract namespace '{}' not in chunk ontology tags {:?} — confidence capped at 0.5",
                                        pred_ns, chunk_namespaces
                                    )
                                } else {
                                    "Triple subject/object not found in chunk text — confidence capped at 0.5".to_string()
                                };
                                tracing::warn!(
                                    target: "hkask.mcp.docproc.triples",
                                    entity = %entity_ref,
                                    subject = %subject,
                                    predicate = %predicate,
                                    "{reason}"
                                );
                                0.5
                            } else {
                                raw_confidence
                            }
                        };

                        // Store subject + object in value so build_prompts can format
                        // triples as "subject --predicate--> object" with confidence.
                        let value = json!({
                            "subject": subject,
                            "object": object,
                        });
                        let h_mem = hkask_storage::HMem::new(&entity_ref, predicate, value, webid)
                            .with_visibility(hkask_types::Visibility::Public)
                            .with_confidence(confidence)
                            .with_dimension(dimension);
                        match semantic.store(h_mem) {
                            Ok(()) => stored += 1,
                            Err(e) => {
                                tracing::warn!(
                                    target: "hkask.mcp.docproc.triples",
                                    entity = %entity_ref,
                                    error = %e,
                                    "Failed to store triple h_mem"
                                );
                            }
                        }
                    }
                }

                h_mems_stored.fetch_add(stored, std::sync::atomic::Ordering::Relaxed);
                succeeded.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            });
            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }

        let succeeded = succeeded.load(std::sync::atomic::Ordering::Relaxed);
        let failed = failed.load(std::sync::atomic::Ordering::Relaxed);
        let h_mems_stored = h_mems_stored.load(std::sync::atomic::Ordering::Relaxed);

        let result = json!({
            "total_chunks": total_chunks,
            "succeeded": succeeded,
            "failed": failed,
            "h_mems_stored": h_mems_stored,
        });
        self.record_experience(
            "docproc_extract_triples",
            &format!("batch: {} chunks", total_chunks),
            "success",
            result.clone(),
        );
        Ok(result)
    }

    #[tool(
        description = "Generate ontology-anchored embedding vectors for corpus chunks. Uses the configured embedding model via the inference router. Reads chunks from chunks_jsonl (entity_ref, source, text, word_count per line). When tagged_jsonl is provided, ontology tags are prepended to chunk text before embedding (per INSTRUCTOR, Su et al. 2023), producing vectors that encode both content and ontological classification. Batch-embeds in groups of batch_size and stores each vector in the memory DB. Returns a summary (total, embedded, failed, model) — no inline vectors."
    )]
    pub async fn docproc_embed(
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
        execute_tool(self, "docproc_embed", async {
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
        let Some(ref emb_router) = self.embedding_router else {
            return Err(McpToolError::failed_precondition(
                "Embedding router not configured — inference config may be missing",
            ));
        };

        let content = std::fs::read_to_string(chunks_path).map_err(|e| {
            McpToolError::invalid_argument(format!(
                "Cannot read chunks_jsonl '{}': {e}",
                chunks_path
            ))
        })?;

        // Parse chunks: each line has entity_ref, source, text, word_count
        let mut chunks: Vec<(String, String)> = Vec::new(); // (entity_ref, text)
        for (i, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line).map_err(|e| {
                McpToolError::invalid_argument(format!(
                    "chunks_jsonl line {} is not valid JSON: {e}",
                    i + 1
                ))
            })?;
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

        let model_name = model.unwrap_or_else(|| {
            std::env::var("HKASK_EMBEDDING_MODEL")
                .unwrap_or_else(|_| "DI/Qwen/Qwen3-Embedding-0.6B".to_string())
        });

        let dim = embedding_dim();
        let semantic =
            hkask_memory::SemanticMemory::open(db_path, passphrase, dim).map_err(|e| {
                McpToolError::failed_precondition(format!("Cannot open memory DB: {e}"))
            })?;

        let mut embedded = 0usize;
        let mut failed = 0usize;
        let batch = batch_size.max(1);

        for chunk_batch in chunks.chunks(batch) {
            let batch_texts: Vec<&str> = chunk_batch.iter().map(|c| c.1.as_str()).collect();
            // Retry with backoff (3 attempts) — same pattern as tag_chunks and extract_triples
            let vectors = {
                let mut attempts = 0u32;
                loop {
                    match emb_router.embed_sentences(&model_name, &batch_texts).await {
                        Ok(v) => break v,
                        Err(e) => {
                            attempts += 1;
                            if attempts >= 3 {
                                failed += chunk_batch.len();
                                tracing::warn!(
                                    target: "hkask.mcp.docproc.embed",
                                    batch_size = chunk_batch.len(),
                                    attempts = attempts,
                                    error = %e,
                                    "Batch embedding failed after 3 retries"
                                );
                                break Vec::new();
                            }
                            let backoff = std::time::Duration::from_secs(2u64.pow(attempts) * 5);
                            tracing::warn!(
                                target: "hkask.mcp.docproc.embed",
                                attempt = attempts,
                                backoff_secs = backoff.as_secs(),
                                error = %e,
                                "Embedding retry — backing off"
                            );
                            tokio::time::sleep(backoff).await;
                        }
                    }
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
        // B2 fix: report degraded outcome when failure rate exceeds threshold.
        // The old code unconditionally reported "success", masking silent batch
        // drops that created holes in the embedding index — holes that degrade
        // the KNN scaffold used by build_prompts.
        let failure_pct = if total == 0 {
            0
        } else {
            (failed * 100) / total
        };
        let outcome = if failure_pct >= DEGRADED_FAILURE_THRESHOLD {
            "degraded"
        } else {
            "success"
        };
        if outcome == "degraded" {
            tracing::warn!(
                target: "hkask.mcp.docproc.embed",
                failed = failed,
                total = total,
                failure_pct = failure_pct,
                threshold_pct = DEGRADED_FAILURE_THRESHOLD,
                "Embedding run degraded — failure rate exceeds threshold"
            );
        }
        self.record_experience(
            "docproc_embed",
            &format!("batch: {} chunks", total),
            outcome,
            result.clone(),
        );
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
    /// Optional provider-prefixed generation model (for example, `OR/openai/gpt-5.6-terra`).
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
    /// Path to tagged chunks JSONL (from docproc_tag_chunks). When provided,
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
    /// Path to tagged chunks JSONL (from docproc_tag_chunks). When provided,
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
