//! Consolidation service — cluster + LLM-synthesize + re-embed chunks.
//!
//! Extracted from `CorpusServer::corpus_consolidate_chunks` in `tools/corpus/mod.rs`.
//! 5-phase pipeline: cluster → concurrent LLM consolidation → build TaggedChunks
//! with ontology merging → re-embed → write.

use std::sync::Arc;

use hkask_mcp_server::server::McpToolError;
use hkask_memory::SemanticMemory;
use hkask_types::InferencePort;
use hkask_types::corpus::{ChunkOntology, ExpertiseLevel, TaggedChunk};
use hkask_types::template::LLMParameters;
use serde_json::json;

use crate::guard::{GUARD, INPUT_GUARD_ENABLED};
use crate::helpers::map_corpus_io_error;
use crate::tools::corpus::{cluster_within_source, read_tagged_chunks};
use crate::tools::semantic::configured_qa_model;
use crate::{embedding_dim, normalize_concept, normalize_in_place, render_docproc_template};

/// Input for [`ConsolidationService::consolidate`].
pub struct ConsolidationRequest {
    pub tagged_jsonl: String,
    pub output: String,
    pub db_path: String,
    pub passphrase: String,
    pub prefix: String,
    pub threshold: f64,
    pub concurrency: usize,
    pub max_chunks_per_cluster: usize,
    pub dry_run: bool,
}

/// Cluster + LLM-synthesize + re-embed chunks.
///
/// Holds the shared inference router. Each call to [`consolidate`] runs the
/// full 5-phase consolidation pipeline.
pub struct ConsolidationService {
    inference_router: Arc<dyn InferencePort>,
}

impl ConsolidationService {
    pub fn new(inference_router: Arc<dyn InferencePort>) -> Self {
        Self { inference_router }
    }

    /// Consolidate semantically related chunks via LLM synthesis.
    ///
    /// Clusters chunks within each source file by cosine similarity > threshold,
    /// then uses the inference router to synthesize each multi-chunk cluster into
    /// a single comprehensive passage. Re-embeds consolidated text and stores the
    /// new embedding. Writes consolidated tagged chunks JSONL with provenance.
    #[must_use = "result must be used"]
    pub async fn consolidate(
        &self,
        request: ConsolidationRequest,
    ) -> Result<serde_json::Value, McpToolError> {
        let ConsolidationRequest {
            tagged_jsonl,
            output,
            db_path,
            passphrase,
            prefix,
            threshold,
            concurrency,
            max_chunks_per_cluster,
            dry_run,
        } = request;

        let chunks = read_tagged_chunks(&tagged_jsonl)?;
        if chunks.is_empty() {
            return Err(McpToolError::invalid_argument("tagged_jsonl is empty"));
        }

        let dim = embedding_dim();
        let semantic = SemanticMemory::open(&db_path, &passphrase, dim).map_err(|e| {
            McpToolError::failed_precondition(format!("Cannot open memory DB: {e}"))
        })?;
        let embeddings = semantic
            .embeddings_by_prefix(&prefix)
            .map_err(|e| McpToolError::internal(format!("Embedding query failed: {e}")))?;

        // Pre-normalize all vectors
        let normalized: Vec<(String, Vec<f32>)> = embeddings
            .into_iter()
            .map(|(er, mut v)| {
                normalize_in_place(&mut v);
                (er, v)
            })
            .collect();
        let norm_map: std::collections::HashMap<&str, &[f32]> = normalized
            .iter()
            .map(|(e, v)| (e.as_str(), v.as_slice()))
            .collect();

        // Group by source
        let mut by_source: std::collections::HashMap<&str, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, c) in chunks.iter().enumerate() {
            by_source.entry(c.source.as_str()).or_default().push(i);
        }

        let threshold = threshold as f32;

        // Phase 1: Cluster
        let mut all_clusters: Vec<Vec<usize>> = Vec::new();
        let mut singletons = 0usize;
        let mut multi = 0usize;

        for indices in by_source.values() {
            let clusters = cluster_within_source(
                indices,
                &chunks,
                &norm_map,
                threshold,
                max_chunks_per_cluster,
            );
            for c in clusters {
                if c.len() > 1 {
                    multi += 1;
                } else {
                    singletons += 1;
                }
                all_clusters.push(c);
            }
        }

        let total_members: usize = all_clusters.iter().map(|c| c.len()).sum();
        let absorbed = total_members - all_clusters.len();

        let stats = json!({
            "original": chunks.len(),
            "clusters": all_clusters.len(),
            "singletons": singletons,
            "multi_chunk": multi,
            "absorbed": absorbed,
            "reduction_pct": (absorbed as f64 / chunks.len().max(1) as f64) * 100.0,
        });

        if dry_run {
            return Ok(stats);
        }

        // Phase 2: LLM consolidation of multi-chunk clusters
        let multi_indices: Vec<usize> = all_clusters
            .iter()
            .enumerate()
            .filter(|(_, c)| c.len() > 1)
            .map(|(i, _)| i)
            .collect();

        let results: Arc<std::sync::Mutex<Vec<Option<String>>>> = Arc::new(std::sync::Mutex::new(
            (0..all_clusters.len()).map(|_| None).collect(),
        ));

        let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
        let router = Arc::clone(&self.inference_router);
        let model_override = configured_qa_model(None);

        let mut handles = Vec::with_capacity(multi_indices.len());
        for &ci in &multi_indices {
            let router = Arc::clone(&router);
            let sem = Arc::clone(&sem);
            let results = Arc::clone(&results);
            let model_override = model_override.clone();
            let cluster = &all_clusters[ci];

            let texts: Vec<String> = cluster
                .iter()
                .map(|&idx| chunks[idx].text.clone())
                .collect();
            let source = chunks[cluster[0]].source.clone();
            let concepts: Vec<String> = cluster
                .iter()
                .flat_map(|&idx| chunks[idx].concepts.iter().cloned())
                .collect::<std::collections::HashSet<String>>()
                .into_iter()
                .collect();

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await;

                let mut passages = String::new();
                for (i, text) in texts.iter().enumerate() {
                    passages.push_str(&format!("[Passage {}]\n{}\n\n", i + 1, text));
                }

                // Render consolidation prompt from Jinja2 template
                // (registry/templates/docproc/consolidate-chunks.j2)
                let mut vars = std::collections::HashMap::new();
                vars.insert("passage_count", texts.len().to_string());
                vars.insert("source", source.clone());
                vars.insert("concepts", concepts.join(", "));
                vars.insert("passages", passages.clone());
                let combined = render_docproc_template("consolidate-chunks", &vars);
                let combined = if combined.is_empty() {
                    format!(
                        "You are a corpus consolidator. Synthesize {n} overlapping passages from \"{source}\" (concepts: {concepts}) into a single comprehensive passage. Preserve ALL unique information, remove redundancy. Output only the consolidated passage text.\n\n{passages}",
                        n = texts.len(),
                        source = source,
                        concepts = concepts.join(", "),
                        passages = passages
                    )
                } else {
                    combined
                };

                // ContentGuard input scan — operator may disable via HKASK_ENABLE_CONTENT_GUARD
                if *INPUT_GUARD_ENABLED {
                    let input_scan = GUARD.scan_input(&combined);
                    if !input_scan.passed {
                        let mut results = results.lock().unwrap_or_else(|e| e.into_inner());
                        results[ci] = Some("__FALLBACK__".to_string());
                        return;
                    }
                }

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

                match router
                    .generate_with_model(&combined, &params, model_override.as_deref(), None)
                    .await
                {
                    Ok(response) => {
                        let output_scan = GUARD.scan_output(&response.text);
                        let content = output_scan.output.content(&response.text);
                        let text = content.trim().to_string();
                        let mut results = results.lock().unwrap_or_else(|e| e.into_inner());
                        results[ci] = Some(text);
                    }
                    Err(_) => {
                        let mut results = results.lock().unwrap_or_else(|e| e.into_inner());
                        results[ci] = Some("__FALLBACK__".to_string());
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            let _ = handle.await;
        }

        // Phase 3: Build consolidated TaggedChunks (collect data, then drop guard)
        let consolidated_texts: Vec<Option<String>> =
            results.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut consolidated: Vec<TaggedChunk> = Vec::with_capacity(all_clusters.len());
        let mut reembed_texts: Vec<(String, String)> = Vec::new();

        for (ci, cluster) in all_clusters.iter().enumerate() {
            if cluster.len() == 1 {
                consolidated.push(chunks[cluster[0]].clone());
            } else {
                let llm_text = consolidated_texts[ci].as_deref().unwrap_or("__FALLBACK__");
                let source = &chunks[cluster[0]].source;
                let entity_ref = format!("corpus:researcher:consolidated:{source}:{ci}");

                let text = if llm_text == "__FALLBACK__" {
                    chunks[cluster[0]].text.clone()
                } else {
                    llm_text.to_string()
                };

                let concepts: Vec<String> = cluster
                    .iter()
                    .flat_map(|&idx| chunks[idx].concepts.iter().cloned())
                    .collect::<std::collections::HashSet<String>>()
                    .into_iter()
                    .collect();

                let salience = cluster
                    .iter()
                    .map(|&idx| chunks[idx].salience)
                    .fold(0.0f32, f32::max);
                let consolidated_from: Vec<String> = cluster
                    .iter()
                    .map(|&idx| chunks[idx].entity_ref.clone())
                    .collect();

                // Dublin Core + PKO metadata for the consolidated chunk
                let ontology = ChunkOntology {
                    dc_type: hkask_bridge_dublincore::DOCUMENT.to_string(),
                    dc_subject: concepts.clone(),
                    dc_source: source.clone(),
                    pko_extracted_from: consolidated_from.clone(),
                };

                // Merge ontology tags from all cluster members
                let dimensions: Vec<String> = cluster
                    .iter()
                    .flat_map(|&idx| chunks[idx].dimensions.iter().cloned())
                    .collect::<std::collections::HashSet<String>>()
                    .into_iter()
                    .collect();
                let dc_type = chunks[cluster[0]].dc_type.clone();
                let dc_subject: Vec<String> = cluster
                    .iter()
                    .flat_map(|&idx| chunks[idx].dc_subject.iter().cloned())
                    .collect::<std::collections::HashSet<String>>()
                    .into_iter()
                    .collect();
                // Merge ontology_tags: union all concept lists per namespace.
                // C2 fix: normalize namespace keys and concept strings so the
                // consolidated chunk's tags are graph-key-consistent with the
                // tagging-phase output. Without this, a cluster containing
                // chunks with "ROIC" and "roic" would produce a merged
                // ontology_tags entry with both variants, fragmenting the
                // salience graph and polluting the embedding annotation prefix.
                let mut merged_tags: std::collections::HashMap<
                    String,
                    std::collections::HashSet<String>,
                > = std::collections::HashMap::new();
                for &idx in cluster {
                    for (ns, concepts) in &chunks[idx].ontology_tags {
                        let norm_ns = normalize_concept(ns);
                        if norm_ns.is_empty() {
                            continue;
                        }
                        let entry = merged_tags.entry(norm_ns).or_default();
                        for c in concepts {
                            let norm = normalize_concept(c);
                            if !norm.is_empty() {
                                entry.insert(norm);
                            }
                        }
                    }
                }
                let ontology_tags: std::collections::HashMap<String, Vec<String>> = merged_tags
                    .into_iter()
                    .map(|(ns, set)| {
                        let mut v: Vec<String> = set.into_iter().collect();
                        v.sort();
                        (ns, v)
                    })
                    .collect();
                // Rebuild concepts cache from merged ontology_tags (already normalized).
                let concepts: Vec<String> = {
                    let mut seen: std::collections::HashSet<String> =
                        std::collections::HashSet::new();
                    let mut v = Vec::new();
                    for concepts_list in ontology_tags.values() {
                        for c in concepts_list {
                            if seen.insert(c.clone()) {
                                v.push(c.clone());
                            }
                        }
                    }
                    v
                };
                // Take highest expertise level (researcher > analyst > practitioner).
                // Uses ExpertiseLevel::rank() and from_rank() so the enum
                // invariant is preserved — no string matching needed.
                let expertise_level = cluster
                    .iter()
                    .map(|&idx| chunks[idx].expertise_level.rank())
                    .max()
                    .map(ExpertiseLevel::from_rank)
                    .unwrap_or_default();

                // Build ontology annotation prefix for consistent re-embedding.
                // Consolidated chunks must use the same [ns: concepts] prefix as
                // original chunks to maintain a consistent embedding space.
                let annotation: String = if ontology_tags.is_empty() {
                    "[unclassified] ".to_string()
                } else {
                    let parts: Vec<String> = ontology_tags
                        .iter()
                        .map(|(ns, concepts)| format!("{ns}: {}", concepts.join(", ")))
                        .collect();
                    format!("[{}] ", parts.join(" | "))
                };
                reembed_texts.push((entity_ref.clone(), format!("{}{}", annotation, text)));

                let word_count = text.split_whitespace().count();
                consolidated.push(TaggedChunk {
                    entity_ref,
                    source: source.clone(),
                    text,
                    word_count,
                    dimensions,
                    dc_type,
                    dc_subject,
                    ontology_tags,
                    concepts,
                    expertise_level,
                    salience,
                    consolidated_from,
                    ontology: Some(ontology),
                });
            }
        }

        // Phase 4: Re-embed consolidated chunks
        let mut embedded_count = 0usize;
        if !reembed_texts.is_empty() {
            let emb_model = hkask_inference::model_constants::embedding_model();

            for batch in reembed_texts.chunks(50) {
                let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
                match self.inference_router.embed(&emb_model, &texts).await {
                    Ok(vectors) => {
                        for ((entity_ref, _), vector) in batch.iter().zip(vectors.iter()) {
                            if let Err(e) = semantic.store_embedding(entity_ref, vector, &emb_model)
                            {
                                tracing::warn!(
                                    entity_ref = %entity_ref,
                                    error = %e,
                                    "Failed to store consolidated embedding"
                                );
                            } else {
                                embedded_count += 1;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            model = %emb_model,
                            batch_len = batch.len(),
                            error = %e,
                            "Embedding call failed for consolidated chunk batch"
                        );
                    }
                }
            }
        }

        // Phase 5: Write output
        let mut out = String::new();
        for chunk in &consolidated {
            out.push_str(
                &serde_json::to_string(chunk)
                    .map_err(|e| McpToolError::internal(format!("Serialize: {e}")))?,
            );
            out.push('\n');
        }
        let output_path = crate::path_safety::contain_for_write(&output)?;
        std::fs::write(&output_path, &out)
            .map_err(|e| map_corpus_io_error(e, &format!("Cannot write output '{}'", output)))?;

        let result = json!({
            "original": chunks.len(),
            "consolidated": consolidated.len(),
            "multi_chunk_clusters": multi,
            "absorbed": absorbed,
            "reembedded": embedded_count,
            "reduction_pct": (1.0 - consolidated.len() as f64 / chunks.len().max(1) as f64) * 100.0,
        });

        Ok(result)
    }
}
