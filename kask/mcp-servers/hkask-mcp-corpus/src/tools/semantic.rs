//! Semantic extraction tools — QA generation, h_mem extraction, embedding.
//!
//! This module is the router host for the `semantic_router` tool group.
//! Helpers live in submodules:
//! - `qa` — QA response parsing and model resolution
//! - `assertions` — RDF predicate → 5W1H dimension mapping
//! - `ontology_io` — tagged-chunks JSONL readers
//!
//! The `#[tool_router]` macro requires all `#[tool]` methods to be on a single
//! `impl CorpusServer` block, so the tool methods stay here in `semantic.rs`.

mod assertions;

mod ontology_io;

use crate::batch::{
    ADAPTIVE_CONCURRENCY_FLOOR, AdaptiveLimiter, BatchOutcome, MAX_RETRIES, retry_with_backoff,
};
use crate::helpers::resolve_corpus_passphrase;
use crate::services::assertions::{AssertionsRequest, AssertionsService};
use crate::{
    Arc, CorpusServer, McpToolError, Parameters, default_embedding_model, default_owner,
    execute_tool, json, read_jsonl, tool, tool_router,
};
use ontology_io::read_ontology_tags_annotated;

use schemars::JsonSchema;
use serde::Deserialize;

// Re-export helpers used by other tool modules (corpus.rs imports these) and
// make them available within this module via the module path.
pub(crate) use assertions::{
    abstract_namespace_tag_key, assertion_confidence, predicate_to_dimension,
};
pub(crate) use ontology_io::read_ontology_namespaces;
pub(crate) use ontology_io::read_ontology_tags;

#[derive(Default)]
struct EmbeddingIdentityState {
    actual_model: Option<String>,
    confirmed_batches: usize,
    missing_batches: usize,
}

impl EmbeddingIdentityState {
    fn observe(
        &mut self,
        requested_model: &str,
        batch: &hkask_types::EmbeddingBatch,
    ) -> Result<(), McpToolError> {
        if batch.requested_model != requested_model {
            return Err(McpToolError::internal(format!(
                "embedding transport changed requested model identity: expected '{requested_model}', got '{}'",
                batch.requested_model
            )));
        }
        match batch.actual_model.as_deref() {
            Some(actual_model) => {
                if let Some(previous) = self.actual_model.as_deref()
                    && previous != actual_model
                {
                    return Err(McpToolError::failed_precondition(format!(
                        "embedding provider returned inconsistent model identities: '{previous}' and '{actual_model}'"
                    )));
                }
                self.actual_model
                    .get_or_insert_with(|| actual_model.to_string());
                self.confirmed_batches += 1;
            }
            None => self.missing_batches += 1,
        }
        Ok(())
    }

    fn status(&self) -> &'static str {
        match (self.confirmed_batches, self.missing_batches) {
            (0, _) => "unavailable",
            (_, 0) => "confirmed",
            _ => "partial",
        }
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    fn batch(actual_model: Option<&str>) -> hkask_types::EmbeddingBatch {
        hkask_types::EmbeddingBatch {
            vectors: vec![vec![1.0]],
            requested_model: "requested/model".to_string(),
            actual_model: actual_model.map(str::to_string),
            usage: hkask_types::InferenceUsage::default(),
        }
    }

    #[test]
    fn identity_status_distinguishes_confirmed_partial_and_unavailable() {
        let mut confirmed = EmbeddingIdentityState::default();
        confirmed
            .observe("requested/model", &batch(Some("actual/model")))
            .expect("identity matches");
        assert_eq!(confirmed.status(), "confirmed");

        let mut partial = EmbeddingIdentityState::default();
        partial
            .observe("requested/model", &batch(Some("actual/model")))
            .expect("identity matches");
        partial
            .observe("requested/model", &batch(None))
            .expect("missing identity is observable");
        assert_eq!(partial.status(), "partial");

        let mut unavailable = EmbeddingIdentityState::default();
        unavailable
            .observe("requested/model", &batch(None))
            .expect("missing identity is observable");
        assert_eq!(unavailable.status(), "unavailable");
    }

    #[test]
    fn identity_reconciliation_rejects_requested_or_actual_model_drift() {
        let mut state = EmbeddingIdentityState::default();
        state
            .observe("requested/model", &batch(Some("actual/one")))
            .expect("first identity establishes the batch model");
        assert!(
            state
                .observe("requested/model", &batch(Some("actual/two")))
                .is_err()
        );

        let mut wrong_request = batch(Some("actual/one"));
        wrong_request.requested_model = "other/request".to_string();
        assert!(state.observe("requested/model", &wrong_request).is_err());
    }

    #[test]
    fn qa_request_schema_documents_complete_v2_adjudication() {
        let schema = schemars::schema_for!(GenerateQaBatchRequest);
        let rendered = serde_json::to_string(&schema).expect("serialize request schema");
        assert!(rendered.contains("prepared-qa-adjudication-v2"));
        assert!(rendered.contains("exact qa_types order"));
        assert!(rendered.contains("V1 and partial manifests fail"));
        assert!(
            !rendered.contains("verification_model"),
            "the rejected verifier surface must not reappear in the request schema"
        );
        let schema: serde_json::Value =
            serde_json::from_str(&rendered).expect("parse request schema");
        let required = schema["required"]
            .as_array()
            .expect("required array")
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();
        assert!(
            required.contains(&"quality_adjudications_jsonl"),
            "adjudication must be required, not defaulted"
        );
    }
}

#[tool_router(router = semantic_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Generate unverified QA candidates from compact prepared requests: prompt_id, protocol, local-to-canonical passages, candidate_terms, and ordered qa_types. Requires a complete prepared-qa-adjudication-v2 JSONL binding passage and per-level mandates to every prompt; reviewed skips are terminal and zero-inference, and reviewed admits execute the ordered level mandates through planning, one bounded generator-owned correction each for mandate mismatch and writer-schema mismatch, then writing with deterministic validation. Generation never authorizes ingestion: candidates are unverified, and corpus_ingest_qa requires a separate complete grounding manifest over canonical source chunks."
    )]
    pub async fn corpus_generate_qa_batch(
        &self,
        Parameters(GenerateQaBatchRequest {
            prompts_jsonl,
            quality_adjudications_jsonl,
            output,
            concurrency,
            model,
        }): Parameters<GenerateQaBatchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_generate_qa_batch", async {
            crate::services::qa_batch::QaBatchService::new(Arc::clone(&self.inference_router))
                .generate_qa_batch(crate::services::qa_batch::QaBatchRequest {
                    prompts_jsonl,
                    quality_adjudications_jsonl,
                    output,
                    concurrency,
                    model,
                })
                .await
        })
        .await
    }

    #[tool(
        description = "Extract assertions (subject, predicate, object) from corpus chunks using the inference engine. Uses the canonical classifier model (HKASK_CLASSIFIER_MODEL; unset is a typed permission_denied naming the setting — kask never falls back to a hidden code constant) with 3-attempt retry. Reads chunks from chunks_jsonl, processes them concurrently, and stores each assertion as a chunk-anchored h_mem (entity=entity_ref, attribute=predicate, value={subject, object}) in the memory DB. When tagged_jsonl is provided, ontology tags from the tagging step are injected to guide predicate selection (GOLEM for narrative, schema.org for expository). Returns a summary (total_chunks, succeeded, failed, h_mems_stored); rows with an empty entity_ref or text are skipped and counted in skipped_invalid_rows."
    )]
    pub async fn corpus_extract_assertions(
        &self,
        Parameters(ExtractAssertionsRequest {
            chunks_jsonl,
            tagged_jsonl,
            db_path,
            max_assertions,
            owner,
            concurrency,
        }): Parameters<ExtractAssertionsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_extract_assertions", async {
            let db_path = crate::helpers::contained_db(&db_path)?;
            let passphrase = resolve_corpus_passphrase()?;
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
        description = "Generate ontology-anchored embedding vectors for corpus chunks. Uses the configured embedding model via the inference router. Reads chunks from chunks_jsonl (entity_ref, source, text, word_count per line). When tagged_jsonl is provided, ontology tags are prepended to chunk text before embedding (per INSTRUCTOR, Su et al. 2023), producing vectors that encode both content and ontological classification. Batch-embeds in groups of batch_size and replaces each DB/ref vector plus original passage_text, publishing to the warm cache. Rows with an empty entity_ref or text are skipped and counted in skipped_invalid_rows. Annotations affect embedding input only. A run that embeds nothing fails with the provider's classified error; a partial run adds first_error. Returns total, embedded, skipped_existing, failed, retries (transient retries consumed across batches — a saturation signal, not an error), cancelled, requested_model, provider-returned actual_model, and actual_model_status (confirmed, partial, or unavailable); clear/purge cancellation includes a note — no inline vectors."
    )]
    pub async fn corpus_embed(
        &self,
        Parameters(EmbedRequest {
            chunks_jsonl,
            tagged_jsonl,
            db_path,
            model,
            batch_size,
            skip_existing,
            write_hmems,
        }): Parameters<EmbedRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_embed", async {
            let db_path = crate::helpers::contained_db(&db_path)?;
            let passphrase = resolve_corpus_passphrase()?;
            self.embed_batch_from_jsonl(
                &chunks_jsonl,
                tagged_jsonl.as_deref(),
                model,
                &db_path,
                &passphrase,
                batch_size,
                skip_existing,
                write_hmems,
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
    #[allow(clippy::too_many_arguments)]
    async fn embed_batch_from_jsonl(
        &self,
        chunks_path: &str,
        tagged_jsonl: Option<&str>,
        model: Option<String>,
        db_path: &str,
        passphrase: &str,
        batch_size: usize,
        skip_existing: bool,
        write_hmems: bool,
    ) -> Result<serde_json::Value, McpToolError> {
        // Parse chunks: each line has entity_ref, source, text, word_count
        let chunks_values = read_jsonl::<serde_json::Value>(chunks_path, "chunks_jsonl")?;
        let mut chunks: Vec<(String, String, String, Option<String>)> = Vec::new(); // (entity_ref, original text, embedding input, origin_provider)
        let mut chunk_sources: Vec<Option<String>> = Vec::new();
        let mut skipped_invalid_rows = 0usize;
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
                skipped_invalid_rows += 1;
                continue;
            }
            let origin_provider = v
                .get(hkask_types::corpus::ORIGIN_PROVIDER_FIELD)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let source_label = v
                .get("source")
                .and_then(|v| v.as_str())
                .and_then(format_source_context_prefix);
            let text = hkask_memory::text_chunking::sanitize_text(&text);
            chunks.push((entity_ref, text.clone(), text, origin_provider));
            chunk_sources.push(source_label);
        }

        let total = chunks.len();
        if total == 0 {
            return Err(McpToolError::invalid_argument(format!(
                "chunks_jsonl contains no valid chunks ({skipped_invalid_rows} rows skipped: empty entity_ref or text)"
            )));
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
            for (entity_ref, _, text, _) in chunks.iter_mut() {
                let annotation = tag_map
                    .get(entity_ref)
                    .map(|s| s.as_str())
                    .unwrap_or("[unclassified] ");
                text.insert_str(0, annotation);
            }
        } else {
            for ((_, _, embed_input, _), source_label) in chunks.iter_mut().zip(chunk_sources) {
                if let Some(prefix) = source_label {
                    embed_input.insert_str(0, &prefix);
                }
            }
        }

        // Fail-visible: no configured embedding model is a typed error
        // naming the setting — never a hidden constant.
        let model_name = model
            .or_else(|| default_embedding_model())
            .ok_or_else(crate::helpers::no_embedding_model)?;

        let write = self.index.begin_durable(
            db_path,
            passphrase,
            crate::index::PublicationScope::Entities(
                chunks.iter().map(|chunk| chunk.0.clone()).collect(),
            ),
            self.webid,
        )?;

        let mut skipped_existing = 0usize;
        if skip_existing {
            let requested_refs: Vec<String> = chunks.iter().map(|c| c.0.clone()).collect();
            let stored = write.embedding_models_for_refs(&requested_refs)?;
            let before = chunks.len();
            chunks.retain(|chunk| {
                stored
                    .get(&chunk.0)
                    .is_none_or(|stored_model| !embedding_model_matches(&model_name, stored_model))
            });
            skipped_existing = before.saturating_sub(chunks.len());
            if chunks.is_empty() {
                tracing::info!(
                    target: "hkask.mcp.docproc.embed",
                    total,
                    skipped_existing,
                    model = %model_name,
                    "All requested chunks already embedded; skipping inference"
                );
                return Ok(json!({
                    "total": total,
                    "skipped_invalid_rows": skipped_invalid_rows,
                    "skipped_existing": skipped_existing,
                    "embedded": 0,
                    "failed": 0,
                    "retries": 0,
                    "failed_entity_refs": [],
                    "failed_entity_refs_complete": true,
                    "cancelled": 0,
                    "requested_model": model_name,
                    "actual_model": null,
                    "actual_model_status": "skipped_existing",
                    "identity_batches": {
                        "confirmed": 0,
                        "missing": 0,
                    },
                }));
            }
        }

        // Dual-bound batching: cap each batch by BOTH `batch_size` (max rows)
        // AND `MAX_EMBED_BATCH_CHARS` (~32K chars ≈ 8–10K tokens) so dense
        // ontology/code chunks (3K+ chars each) automatically form smaller
        // batches instead of exceeding provider per-request token burst quotas.
        const MAX_EMBED_BATCH_CHARS: usize = 32_000;
        let max_rows = batch_size.max(1);
        let mut batches: Vec<Vec<(String, String, String, Option<String>)>> = Vec::new();
        let mut current_batch: Vec<(String, String, String, Option<String>)> = Vec::new();
        let mut current_chars = 0usize;
        for chunk in chunks {
            let chunk_chars = chunk.2.len();
            if !current_batch.is_empty()
                && (current_batch.len() >= max_rows
                    || current_chars.saturating_add(chunk_chars) > MAX_EMBED_BATCH_CHARS)
            {
                batches.push(std::mem::take(&mut current_batch));
                current_chars = 0;
            }
            current_chars = current_chars.saturating_add(chunk_chars);
            current_batch.push(chunk);
        }
        if !current_batch.is_empty() {
            batches.push(current_batch);
        }
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
        let write = Arc::new(write);
        let model_name = Arc::new(model_name.clone());

        let embedded = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let failed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let retries = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let failed_entity_refs = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        // The first provider failure, classified: a run that embeds nothing
        // returns it as the tool error, a partial run reports its message.
        let first_provider_error = Arc::new(std::sync::Mutex::new(None::<McpToolError>));
        let identity = Arc::new(std::sync::Mutex::new(EmbeddingIdentityState::default()));

        let cancelled = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let mut join_set = tokio::task::JoinSet::new();

        for (batch_idx, chunk_batch) in batches.into_iter().enumerate() {
            let limiter = limiter.clone();
            let router = Arc::clone(&router);
            let write = Arc::clone(&write);
            let index = Arc::clone(&self.index);
            let cancelled = Arc::clone(&cancelled);
            let model_name = Arc::clone(&model_name);
            let embedded = Arc::clone(&embedded);
            let failed = Arc::clone(&failed);
            let retries = Arc::clone(&retries);
            let failed_entity_refs = Arc::clone(&failed_entity_refs);
            let first_provider_error = Arc::clone(&first_provider_error);
            let identity = Arc::clone(&identity);

            let batch_len = chunk_batch.len();

            join_set.spawn(async move {
                let slot = limiter.acquire().await;

                let batch_texts: Vec<String> = chunk_batch.iter().map(|c| c.2.clone()).collect();

                // Record a whole-batch failure — the shared tail of the
                // transport-failure and invalid-response arms: count the
                // batch's rows as failed, extend the failed entity refs,
                // and end the task Ok (the batch is spent; the summary
                // reports it). Per-row publish failures keep their own
                // tail (single-row count, push, continue).
                let record_batch_failure = || -> Result<(), McpToolError> {
                    failed.fetch_add(batch_len, std::sync::atomic::Ordering::Relaxed);
                    failed_entity_refs
                        .lock()
                        .map_err(|_| McpToolError::internal("failed entity-ref lock poisoned"))?
                        .extend(chunk_batch.iter().map(|chunk| chunk.0.clone()));
                    Ok(())
                };

                let outcome = retry_with_backoff(
                    MAX_RETRIES,
                    "hkask.mcp.docproc.embed",
                    &format!("batch {batch_idx} of {batch_len}"),
                    || {
                        router.embed_with_dimensions(
                            &model_name,
                            &batch_texts,
                            Some(crate::embedding_dim() as u32),
                        )
                    },
                )
                .await;
                // The ONE outcome classifier: a breaker-open verdict pauses
                // the limiter (the embedding port carries none today — its
                // impl is honestly None), any failure halves.
                slot.report_inference_outcome(&outcome);
                let batch = match outcome {
                    Ok(outcome) => {
                        retries.fetch_add(outcome.retries, std::sync::atomic::Ordering::Relaxed);
                        outcome.value
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "hkask.mcp.docproc.embed",
                            batch = batch_idx,
                            error = %e,
                            "Batch {batch_idx} failed after retries"
                        );
                        first_provider_error
                            .lock()
                            .map_err(|_| McpToolError::internal("provider error lock poisoned"))?
                            .get_or_insert(crate::helpers::map_embedding_error(e));
                        return record_batch_failure();
                    }
                };

                identity
                    .lock()
                    .map_err(|_| McpToolError::internal("embedding identity lock poisoned"))?
                    .observe(&model_name, &batch)?;
                let durable_model = batch
                    .actual_model
                    .clone()
                    .unwrap_or_else(|| model_name.to_string());
                let vectors = batch.vectors;
                if let Err(error) = crate::index::validate_vectors(&vectors, batch_len) {
                    tracing::warn!(%error, "Invalid embedding batch response");
                    first_provider_error
                        .lock()
                        .map_err(|_| McpToolError::internal("provider error lock poisoned"))?
                        .get_or_insert(error);
                    return record_batch_failure();
                }

                let batch_items: Vec<crate::index::DurableBatchItem<'_>> = chunk_batch
                    .iter()
                    .zip(vectors.iter())
                    .map(|(c, vector)| crate::index::DurableBatchItem {
                        entity_ref: &c.0,
                        text: &c.1,
                        embedding: vector,
                        model: &durable_model,
                        origin_provider: c.3.as_deref(),
                    })
                    .collect();
                if let Err(e) = index.publish_durable_batch(&write, &batch_items, write_hmems) {
                    if write.is_cancelled() {
                        cancelled.fetch_add(batch_len, std::sync::atomic::Ordering::Relaxed);
                        return record_batch_failure();
                    }
                    return Err(e);
                }
                embedded.fetch_add(batch_len, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            });
        }

        // Wait for all batch tasks to complete.
        let mut publication_error = None;
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    publication_error.get_or_insert(error);
                }
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.mcp.docproc.embed",
                        %error,
                        "Embedding batch task join failed"
                    );
                }
            }
        }
        if let Some(error) = publication_error {
            return Err(error);
        }

        let embedded = embedded.load(std::sync::atomic::Ordering::Relaxed);
        // Includes rows lost to a task panic, not just reported provider/store errors.
        let attempted = total.saturating_sub(skipped_existing);
        let failed = attempted.saturating_sub(embedded);
        let cancelled = cancelled.load(std::sync::atomic::Ordering::Relaxed);
        let retries = retries.load(std::sync::atomic::Ordering::Relaxed);
        let identity = identity
            .lock()
            .map_err(|_| McpToolError::internal("embedding identity lock poisoned"))?;
        let actual_model_status = identity.status();
        let actual_model = identity.actual_model.clone();
        let mut failed_entity_refs = failed_entity_refs
            .lock()
            .map_err(|_| McpToolError::internal("failed entity-ref lock poisoned"))?
            .clone();
        failed_entity_refs.sort();
        failed_entity_refs.dedup();
        let failed_entity_refs_complete = failed_entity_refs.len() == failed;
        let first_provider_error = first_provider_error
            .lock()
            .map_err(|_| McpToolError::internal("provider error lock poisoned"))?
            .take();
        // A run that stored nothing because the provider refused or answered
        // wrongly is that failure, classified — not a success with a zero count.
        if embedded == 0
            && let Some(error) = first_provider_error
        {
            return Err(McpToolError::new(
                error.kind,
                format!(
                    "no chunk was embedded ({total} requested, model {model_name}): {}",
                    error.message
                ),
            ));
        }

        tracing::info!(
            target: "hkask.mcp.docproc.embed",
            total, embedded, failed, retries, num_batches, ceiling = crate::max_concurrency(),
            "Embedding complete"
        );

        let mut result = json!({
            "total": total,
            "skipped_invalid_rows": skipped_invalid_rows,
            "skipped_existing": skipped_existing,
            "embedded": embedded,
            "failed": failed,
            "retries": retries,
            "failed_entity_refs": failed_entity_refs,
            "failed_entity_refs_complete": failed_entity_refs_complete,
            "cancelled": cancelled,
            "requested_model": model_name,
            "actual_model": actual_model,
            "actual_model_status": actual_model_status,
            "identity_batches": {
                "confirmed": identity.confirmed_batches,
                "missing": identity.missing_batches,
            },
        });
        if cancelled > 0 {
            result["note"] =
                json!("Publication cancelled by clear/purge; rerun to publish new data");
        }
        if let Some(error) = first_provider_error {
            result["first_error"] = json!(error.message);
        }
        let outcome = BatchOutcome::from_counts(failed, total);
        outcome.log_if_degraded("hkask.mcp.docproc.embed", "Embedding");
        Ok(result)
    }
}

// ── Request structs ────────────────────────────────────────────────────────

/// Request for generating QA from a canonical prepared-prompt JSONL file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GenerateQaBatchRequest {
    /// Canonical prepared JSONL: prompt_id, protocol, passages, candidate_terms,
    /// and ordered qa_types. IDs must be unique, 1–64 provider-safe ASCII characters.
    pub prompts_jsonl: String,
    /// Required complete `prepared-qa-adjudication-v2` JSONL. Every prepared prompt
    /// appears exactly once with matching prompt_id, chunk_ref, source, one passage
    /// decision, and level decisions in exact qa_types order. V1 and partial manifests fail;
    /// there is no unadjudicated generation path.
    pub quality_adjudications_jsonl: String,
    /// Output path for generated QAs JSONL.
    pub output: String,
    /// Max concurrent LLM calls.
    #[serde(default = "default_batch_concurrency")]
    pub concurrency: usize,
    /// Optional provider-prefixed generation model for every prompt in this batch.
    /// Overrides `kask.models.qa_generation_model` (`HKASK_QA_GENERATION_MODEL`);
    /// no active-chat or training-base fallback. Must accept non-thinking requests.
    #[serde(default)]
    pub model: Option<String>,
}

fn default_batch_concurrency() -> usize {
    crate::max_concurrency()
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
    /// Maximum h_mems to extract per chunk (default 15).
    #[serde(default = "default_max_assertions")]
    pub max_assertions: usize,
    /// Owner persona for stored h_mems (e.g. "capabilities-reasoner").
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
#[serde(deny_unknown_fields)]
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
    /// Embedding model to use. If not set, uses the configured default.
    #[serde(default)]
    pub model: Option<String>,
    /// Batch size for embedding API calls (default 50).
    #[serde(default = "default_embed_batch_size")]
    pub batch_size: usize,
    /// When true, skip chunks whose `entity_ref` is already stored in `db_path`
    /// under a matching embedding model (natively resumable bulk embedding).
    #[serde(default)]
    pub skip_existing: bool,
    /// When true (default), also write companion `hmems` rows (`text`,
    /// `method_signals`, optional `origin_provider`) alongside `embeddings` +
    /// `vec_embeddings`. Set false for read-only retrieval DBs (e.g.
    /// `zk-ref-open` `fine.db`) whose consumers read only `embeddings` +
    /// `vec_embeddings`.
    #[serde(default = "default_write_hmems")]
    pub write_hmems: bool,
}

fn default_embed_batch_size() -> usize {
    50
}

fn default_write_hmems() -> bool {
    true
}

/// Derive a compact contextual document-title prefix from a source filename
/// (zero-LLM-cost floor of Anthropic Contextual Retrieval: preserves document
/// identity in short chunk embeddings while keeping stored `passage_text` clean).
fn format_source_context_prefix(source: &str) -> Option<String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return None;
    }
    let filename = std::path::Path::new(trimmed)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(trimmed);
    let stem = filename
        .rsplit_once('.')
        .map(|(base, _)| base)
        .filter(|base| !base.is_empty())
        .unwrap_or(filename);
    let clean: String = stem
        .chars()
        .map(|c| if c == '-' || c == '_' { ' ' } else { c })
        .collect();
    let normalized = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(format!("[Source: {normalized}]\n"))
    }
}

/// Match a requested provider-prefixed embedding model (e.g.
/// `"openrouter/qwen/qwen3-embedding-8b"`) against a stored provider-confirmed
/// model string (e.g. `"Qwen/Qwen3-Embedding-8B"`).
fn embedding_model_matches(requested: &str, stored: &str) -> bool {
    if requested.eq_ignore_ascii_case(stored) {
        return true;
    }
    if let Some((_, suffix)) = requested.split_once('/')
        && suffix.eq_ignore_ascii_case(stored)
    {
        return true;
    }
    false
}
