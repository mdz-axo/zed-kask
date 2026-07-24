//! Corpus pipeline tools — semantic chunk dedup and consolidation.
//!
//! These tools operate on tagged chunks JSONL (from the salience phase) and
//! the SQLCipher memory DB (containing chunk embeddings). They are the
//! "Phase 2c" and "Phase 2d" quality gates in the corpus pipeline.
//!
//! - `docproc_dedup_chunks`: Removes near-identical chunks (cosine > 0.85)
//!   using stored embeddings. Keeps highest-salience survivor per cluster.
//! - `docproc_consolidate_chunks`: Clusters semantically related chunks
//!   (cosine > 0.75), uses the inference router to LLM-synthesize each
//!   multi-chunk cluster into a single comprehensive passage, re-embeds
//!   the consolidated text, and stores the new embedding in the DB.

use crate::tools::semantic::{GUARD, INPUT_GUARD_ENABLED, configured_qa_model};
use crate::*;
use schemars::JsonSchema;
use serde::Deserialize;

use hkask_types::corpus::{ChunkOntology, TaggedChunk};

mod clustering;
mod qa_parsing;
mod qa_types;

use clustering::{cluster_within_source, read_tagged_chunks};
use qa_parsing::{ParsedQa, parse_qa_record};
use qa_types::{QaType, parse_type_distribution, qa_type_instruction, qa_type_str};

#[tool_router(router = corpus_router, vis = "pub")]
impl DocProcServer {
    #[tool(
        description = "Deduplicate chunks by semantic embedding similarity. Queries chunk embeddings from the memory DB, clusters within each source file by cosine similarity > threshold (default 0.85), and keeps the highest-salience chunk per cluster. Writes deduplicated tagged chunks JSONL."
    )]
    pub async fn docproc_dedup_chunks(
        &self,
        Parameters(req): Parameters<DedupChunksRequest>,
    ) -> String {
        execute_tool(self, "docproc_dedup_chunks", async {
            let chunks = read_tagged_chunks(&req.tagged_jsonl)?;
            if chunks.is_empty() {
                return Err(McpToolError::invalid_argument("tagged_jsonl is empty"));
            }

            let dim = embedding_dim();
            let semantic = SemanticMemory::open(&req.db_path, &req.passphrase, dim).map_err(|e| {
                McpToolError::failed_precondition(format!("Cannot open memory DB: {e}"))
            })?;
            let embeddings = semantic
                .embeddings_by_prefix(&req.prefix)
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

            let threshold = req.threshold as f32;
            let mut keep_indices: Vec<usize> = Vec::new();
            let mut clusters_total = 0usize;

            for indices in by_source.values() {
                let clusters = cluster_within_source(
                    indices,
                    &chunks,
                    &norm_map,
                    threshold,
                    usize::MAX, // no cap for dedup
                );
                clusters_total += clusters.len();
                for cluster in &clusters {
                    // Keep the first (highest-salience) member
                    keep_indices.push(cluster[0]);
                }
            }

            keep_indices.sort_unstable();
            keep_indices.dedup();

            let result = json!({
                "original": chunks.len(),
                "deduped": keep_indices.len(),
                "removed": chunks.len() - keep_indices.len(),
                "clusters": clusters_total,
                "sources": by_source.len(),
                "reduction_pct": (1.0 - keep_indices.len() as f64 / chunks.len().max(1) as f64) * 100.0,
            });

            if req.dry_run {
                return Ok(result);
            }

            let mut out = String::new();
            for &idx in &keep_indices {
                out.push_str(&serde_json::to_string(&chunks[idx])
                    .map_err(|e| McpToolError::internal(format!("Serialize: {e}")))?);
                out.push('\n');
            }
            std::fs::write(&req.output, &out).map_err(|e| {
                McpToolError::internal(format!("Cannot write output '{}': {e}", req.output))
            })?;

            self.record_experience(
                "docproc_dedup_chunks",
                &format!("{} → {}", chunks.len(), keep_indices.len()),
                "success",
                result.clone(),
            );
            Ok(result)
        })
        .await
    }

    #[tool(
        description = "Consolidate semantically related chunks via LLM synthesis. Clusters chunks within each source file by cosine similarity > threshold (default 0.75), then uses the inference router to synthesize each multi-chunk cluster into a single comprehensive passage. Re-embeds consolidated text and stores the new embedding. Writes consolidated tagged chunks JSONL with provenance."
    )]
    pub async fn docproc_consolidate_chunks(
        &self,
        Parameters(req): Parameters<ConsolidateChunksRequest>,
    ) -> String {
        execute_tool(self, "docproc_consolidate_chunks", async {
            let chunks = read_tagged_chunks(&req.tagged_jsonl)?;
            if chunks.is_empty() {
                return Err(McpToolError::invalid_argument("tagged_jsonl is empty"));
            }

            let dim = embedding_dim();
            let semantic = SemanticMemory::open(&req.db_path, &req.passphrase, dim).map_err(|e| {
                McpToolError::failed_precondition(format!("Cannot open memory DB: {e}"))
            })?;
            let embeddings = semantic
                .embeddings_by_prefix(&req.prefix)
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

            let threshold = req.threshold as f32;

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
                    req.max_chunks_per_cluster,
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

            if req.dry_run {
                return Ok(stats);
            }

            // Phase 2: LLM consolidation of multi-chunk clusters
            let multi_indices: Vec<usize> = all_clusters
                .iter()
                .enumerate()
                .filter(|(_, c)| c.len() > 1)
                .map(|(i, _)| i)
                .collect();

            let results: Arc<std::sync::Mutex<Vec<Option<String>>>> =
                Arc::new(std::sync::Mutex::new(
                    (0..all_clusters.len()).map(|_| None).collect(),
                ));

            let sem = Arc::new(tokio::sync::Semaphore::new(req.concurrency));
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
                            n = texts.len(), source = source, concepts = concepts.join(", "), passages = passages
                        )
                    } else {
                        combined
                    };

                    // ContentGuard input scan — operator may disable via HKASK_ENABLE_CONTENT_GUARD
                    if *INPUT_GUARD_ENABLED {
                        let input_scan = GUARD.scan_input(&combined);
                        if !input_scan.passed {
                            let mut results = results.lock().unwrap();
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
                            let mut results = results.lock().unwrap();
                            results[ci] = Some(text);
                        }
                        Err(_) => {
                            let mut results = results.lock().unwrap();
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
            let consolidated_texts: Vec<Option<String>> = results.lock().unwrap().clone();
            let mut consolidated: Vec<TaggedChunk> = Vec::with_capacity(all_clusters.len());
            let mut reembed_texts: Vec<(String, String)> = Vec::new();

            for (ci, cluster) in all_clusters.iter().enumerate() {
                if cluster.len() == 1 {
                    consolidated.push(chunks[cluster[0]].clone());
                } else {
                    let llm_text = consolidated_texts[ci].as_ref().unwrap();
                    let source = &chunks[cluster[0]].source;
                    let entity_ref = format!("corpus:researcher:consolidated:{source}:{ci}");

                    let text = if llm_text == "__FALLBACK__" {
                        chunks[cluster[0]].text.clone()
                    } else {
                        llm_text.clone()
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
                    let mut merged_tags: std::collections::HashMap<String, std::collections::HashSet<String>> =
                        std::collections::HashMap::new();
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
                        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
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
                        .map(hkask_types::corpus::ExpertiseLevel::from_rank)
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
            if !reembed_texts.is_empty() && self.embedding_router.is_some() {
                let emb_router = self.embedding_router.as_ref().unwrap();
                let emb_model = std::env::var("HKASK_EMBEDDING_MODEL")
                    .unwrap_or_else(|_| "DI/Qwen/Qwen3-Embedding-0.6B".to_string());

                for batch in reembed_texts.chunks(50) {
                    let texts: Vec<&str> = batch.iter().map(|(_, t)| t.as_str()).collect();
                    if let Ok(vectors) = emb_router.embed_sentences(&emb_model, &texts).await {
                        for ((entity_ref, _), vector) in batch.iter().zip(vectors.iter()) {
                            if semantic
                                .store_embedding(entity_ref, vector, &emb_model)
                                .is_ok()
                            {
                                embedded_count += 1;
                            }
                        }
                    }
                }
            }

            // Phase 5: Write output
            let mut out = String::new();
            for chunk in &consolidated {
                out.push_str(&serde_json::to_string(chunk)
                    .map_err(|e| McpToolError::internal(format!("Serialize: {e}")))?);
                out.push('\n');
            }
            std::fs::write(&req.output, &out).map_err(|e| {
                McpToolError::internal(format!("Cannot write output '{}': {e}", req.output))
            })?;

            let result = json!({
                "original": chunks.len(),
                "consolidated": consolidated.len(),
                "multi_chunk_clusters": multi,
                "absorbed": absorbed,
                "reembedded": embedded_count,
                "reduction_pct": (1.0 - consolidated.len() as f64 / chunks.len().max(1) as f64) * 100.0,
            });

            self.record_experience(
                "docproc_consolidate_chunks",
                &format!("{} → {}", chunks.len(), consolidated.len()),
                "success",
                result.clone(),
            );
            Ok(result)
        })
        .await
    }

    // ── Build Prompts ──────────────────────────────────────────────────────

    #[tool(
        description = "Build QA generation prompts from tagged chunks with KNN context scaffold, ontology context, and h_mem knowledge graph. For each chunk, retrieves embedding-similar passages (KNN), formats ontology tags (5W1H + Dublin Core + PKO), and queries h_mems from the memory DB to build a knowledge graph section. Outputs prompts JSONL consumed by docproc_generate_qa_batch."
    )]
    pub async fn docproc_build_prompts(
        &self,
        Parameters(req): Parameters<BuildPromptsRequest>,
    ) -> String {
        execute_tool(self, "docproc_build_prompts", async {
            let chunks = read_tagged_chunks(&req.tagged_jsonl)?;
            if chunks.is_empty() {
                return Err(McpToolError::invalid_argument("tagged_jsonl is empty"));
            }
            let total = chunks.len();
            tracing::info!("  Build prompts: {} chunks", total);

            // QA type rotation — default distribution
            let default_rotation = parse_type_distribution(&req.type_distribution);

            // Parse per-ontology Bloom overrides (if provided)
            // Format: "golem:0,1,2,1,1|fibo:2,2,1,0,0|pko:1,1,1,2,0|eso:1,1,2,1,0"
            let bloom_overrides: std::collections::HashMap<String, Vec<QaType>> =
                req.ontology_bloom_overrides
                    .as_deref()
                    .map(|s| {
                        s.split('|')
                            .filter_map(|entry| {
                                let (ns, dist) = entry.split_once(':')?;
                                let vals = parse_type_distribution(dist);
                                if vals.is_empty() { None } else { Some((ns.to_string(), vals)) }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            let limit = if req.max_prompts > 0 {
                req.max_prompts.min(total)
            } else {
                total
            };

            // Sort by salience descending
            let mut sorted: Vec<&TaggedChunk> = chunks.iter().collect();
            sorted.sort_by(|a, b| {
                b.salience
                    .partial_cmp(&a.salience)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            // Bulk-load embeddings for in-memory KNN
            let dim = embedding_dim();
            let semantic = SemanticMemory::open(&req.db_path, &req.passphrase, dim)
                .map_err(|e| McpToolError::failed_precondition(format!("Cannot open memory DB: {e}")))?;

            let text_map: std::collections::HashMap<&str, &str> = chunks
                .iter()
                .map(|c| (c.entity_ref.as_str(), c.text.as_str()))
                .collect();
            let source_map: std::collections::HashMap<&str, &str> = chunks
                .iter()
                .map(|c| (c.entity_ref.as_str(), c.source.as_str()))
                .collect();

            let emb_map: std::collections::HashMap<String, Vec<f32>> =
                match semantic.embeddings_by_prefix("corpus:researcher:") {
                    Ok(embs) => {
                        let map: std::collections::HashMap<String, Vec<f32>> = embs
                            .into_iter()
                            .map(|(er, mut v)| {
                                normalize_in_place(&mut v);
                                (er, v)
                            })
                            .collect();
                        tracing::info!("  Bulk-loaded {} normalized embeddings", map.len());
                        map
                    }
                    Err(e) => {
                        tracing::info!("  Warning: embedding query failed — scaffold disabled: {e}");
                        std::collections::HashMap::new()
                    }
                };

            // Group embeddings by source for scoped KNN
            let mut emb_by_source: std::collections::HashMap<&str, Vec<(&String, &Vec<f32>)>> =
                std::collections::HashMap::new();
            for chunk in &chunks {
                if let Some(v) = emb_map.get(&chunk.entity_ref) {
                    emb_by_source
                        .entry(chunk.source.as_str())
                        .or_default()
                        .push((&chunk.entity_ref, v));
                }
            }

            // Build concept graph (concept → chunk_count)
            let mut concept_connections: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            for chunk in &chunks {
                for concept in &chunk.concepts {
                    *concept_connections.entry(concept.as_str()).or_default() += 1;
                }
            }

            // Owner is passed to the template via vars; WebID not needed here since
            // build_prompts only queries h_mems (read-only), doesn't store new ones.

            let mut out = String::new();
            let mut ti = 0usize;

            for tc in sorted.iter().take(limit) {
                // KNN scaffold: source-scoped search
                let context_passages: Vec<serde_json::Value> = {
                    let query_vec = match emb_map.get(&tc.entity_ref) {
                        Some(v) => v.as_slice(),
                        None => &[],
                    };
                    if query_vec.is_empty() {
                        Vec::new()
                    } else {
                        let k = req.context_k;
                        let candidates = emb_by_source
                            .get(tc.source.as_str())
                            .map(|v| v.as_slice())
                            .unwrap_or(&[]);
                        let mut scored: Vec<(&String, f32)> = candidates
                            .iter()
                            .filter(|(er, _)| er.as_str() != tc.entity_ref)
                            .map(|(er, v)| {
                                // Vectors are pre-normalized, so dot product = cosine similarity.
                                let cosine_sim: f32 = query_vec.iter().zip(v.iter()).map(|(a, b)| a * b).sum();
                                (*er, cosine_sim)
                            })
                            .collect();
                        let top_k: Vec<(&String, f32)> = if scored.len() > k {
                            // Partition around index k-1 so that elements 0..k are
                            // the top-k by score (descending). The return value
                            // (pivot, left, right) is discarded — only the
                            // partitioning side effect matters. After partitioning,
                            // scored[..k] contains the top-k but unsorted, so we
                            // sort that slice in place. This avoids sorting the
                            // entire scored vec (O(n log n)) in favor of
                            // partition + partial sort (O(n + k log k)).
                            scored.select_nth_unstable_by(k.saturating_sub(1), |a, b| {
                                b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                            });
                            scored[..k]
                                .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                            scored.into_iter().take(k).collect()
                        } else {
                            scored
                                .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                            scored.into_iter().collect()
                        };
                        top_k
                            .into_iter()
                            .map(|(er, sim)| {
                                let text = text_map.get(er.as_str()).copied().unwrap_or("");
                                let source = source_map.get(er.as_str()).copied().unwrap_or(er);
                                serde_json::json!({
                                    "source": source,
                                    "similarity": sim,
                                    "text": text,
                                })
                            })
                            .collect()
                    }
                };

                // Format context text
                let context_text = if context_passages.is_empty() {
                    "(none — no embedding context available)".to_string()
                } else {
                    context_passages
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let source = p["source"].as_str().unwrap_or("?");
                            let sim = p["similarity"].as_f64().unwrap_or(0.0);
                            let text = p["text"].as_str().unwrap_or("");
                            let truncated = if text.len() > 2000 {
                                let mut end = 2000;
                                while end > 0 && !text.is_char_boundary(end) {
                                    end -= 1;
                                }
                                &text[..end]
                            } else {
                                text
                            };
                            format!("[{}] Source: {}, Similarity: {:.2}\n    {}", i + 1, source, sim, truncated)
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n")
                };

                // Issue 7: Diagnostic — log KNN neighbor sources to verify
                // ontology-anchored embeddings produce same-domain retrieval.
                if !context_passages.is_empty() {
                    let neighbor_sources: Vec<&str> = context_passages
                        .iter()
                        .filter_map(|p| p["source"].as_str())
                        .collect();
                    tracing::info!(
                        target: "hkask.mcp.docproc.build_prompts",
                        chunk_ref = %tc.entity_ref,
                        chunk_source = %tc.source,
                        neighbor_sources = ?neighbor_sources,
                        "KNN context retrieved — verify neighbors share ontology with chunk"
                    );
                }

                // Concept graph
                let concept_graph_text = tc.concepts.iter().map(|concept| {
                    let connected = concept_connections.get(concept.as_str()).copied().unwrap_or(1);
                    format!("- {} (connected to {} chunks)", concept, connected)
                }).collect::<Vec<_>>().join("\n");
                let concept_graph_text = if concept_graph_text.is_empty() {
                    "(none)".to_string()
                } else {
                    concept_graph_text
                };

                // h_mem knowledge graph — query all h_mems for this chunk
                let kg_text = match semantic.query_deduped(&tc.entity_ref) {
                    Ok(h_mems) if !h_mems.is_empty() => {
                        let mut lines: Vec<String> = Vec::new();
                        for h_mem in &h_mems {
                            if h_mem.attribute == "text" || h_mem.attribute == "corpus_provenance" || h_mem.attribute == "ontology_tags" {
                                continue; // skip non-triple h_mems
                            }
                            let dim = h_mem.dimension.as_ref().map(|d| d.as_str()).unwrap_or("what");
                            let conf = format!("{:.2}", h_mem.confidence.value());
                            // v2: value is {"subject": "...", "object": "..."}
                            let (subj, obj) = match &h_mem.value {
                                serde_json::Value::Object(map) => {
                                    let s = map.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                                    let o = map.get("object").map(|v| match v {
                                        serde_json::Value::String(s) => s.clone(),
                                        v => v.to_string(),
                                    }).unwrap_or_default();
                                    (s.to_string(), o)
                                }
                                // Legacy: value is the object directly
                                serde_json::Value::String(s) => (String::new(), s.clone()),
                                v => (String::new(), v.to_string()),
                            };
                            let entity_label = if subj.is_empty() { tc.entity_ref.as_str() } else { &subj };
                            lines.push(format!("  - [{}] (conf={}) {} --{}--> {}", dim, conf, entity_label, h_mem.attribute, obj));
                        }
                        if lines.is_empty() {
                            "(none)".to_string()
                        } else {
                            lines.join("\n")
                        }
                    }
                    _ => "(none)".to_string(),
                };

                // Generate prompts_per_chunk QAs per chunk at consecutive Bloom levels
                // Select Bloom distribution: check ontologies in priority order
                // (narrative > financial > epistemic > process > default).
                // This ensures narrative chunks always get golem distribution
                // even if they also have epistemic or pko tags.
                let type_rotation: &[QaType] = {
                    const PRIORITY: &[&str] = &["pko", "golem", "fibo", "eso", "epistemic"];
                    let mut selected: Option<&[QaType]> = None;
                    for ns in PRIORITY {
                        if tc.ontology_tags.contains_key(*ns)
                            && bloom_overrides.contains_key(*ns)
                        {
                            selected = Some(&bloom_overrides[*ns]);
                            break;
                        }
                    }
                    selected.unwrap_or(&default_rotation)
                };

                for offset in 0..req.prompts_per_chunk {
                    let qt = type_rotation[(ti + offset) % type_rotation.len()];
                    let qt_str = qa_type_str(qt);

                    let dimensions_str = if tc.dimensions.is_empty() {
                        "what".to_string()
                    } else {
                        tc.dimensions.join(", ")
                    };
                    // ExpertiseLevel is always valid (deserializer maps unknown
                    // strings to Analyst), so no empty-check needed.
                    let expertise = tc.expertise_level.as_str();
                    let dc_type = if tc.dc_type.is_empty() {
                        "bibo:Document"
                    } else {
                        tc.dc_type.as_str()
                    };
                    let dc_subject = if tc.dc_subject.is_empty() {
                        tc.concepts.join(", ")
                    } else {
                        tc.dc_subject.join(", ")
                    };
                    let tags_str: String = tc
                        .ontology_tags
                        .iter()
                        .map(|(ns, concepts)| format!("{}: {}", ns, concepts.join(", ")))
                        .collect::<Vec<_>>()
                        .join(" | ");
                    let consolidated_from = if tc.consolidated_from.is_empty() {
                        String::new()
                    } else {
                        tc.consolidated_from.len().to_string()
                    };

                    // Render system prompt from Jinja2 template
                    let mut vars: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
                    vars.insert("qa_instruction", qa_type_instruction(qt).to_string());
                    vars.insert("dimensions", dimensions_str.clone());
                    vars.insert("qa_type", qt_str.to_string());
                    vars.insert("expertise", expertise.to_string());
                    vars.insert("source", tc.source.clone());
                    vars.insert("dc_type", dc_type.to_string());
                    vars.insert("dc_subject", dc_subject.clone());
                    vars.insert("consolidated_from", consolidated_from);
                    vars.insert("ontology_tags", if tags_str.is_empty() { "(none)".to_string() } else { tags_str.clone() });
                    vars.insert("context_passages", context_text.clone());
                    vars.insert("concept_graph", concept_graph_text.clone());
                    vars.insert("knowledge_graph", kg_text.clone());
                    let system = render_docproc_template("build-prompts", &vars);
                    let system = if system.is_empty() {
                        // Fallback if template not found
                        format!(
                            "You are a Capabilities Researcher training data generator. Given a primary passage from capabilities and research literature, generate ONE question-answer pair. Calibrate question depth to the expertise level indicated below.\n\n{}\n\n## Ontological Context\n5W1H: [{}]. QA at {} for {} expertise.\nSource: {}. Tags: {}\n\n## Context Passages\n{}\n\n## Knowledge Graph\n{}",
                            qa_type_instruction(qt), dimensions_str, qt_str, expertise, tc.source,
                            if tags_str.is_empty() { "(none)" } else { &tags_str },
                            context_text, kg_text
                        )
                    } else {
                        system
                    };

                    let prompt = serde_json::json!({
                        "chunk_ref": tc.entity_ref,
                        "source": tc.source,
                        "concepts": tc.concepts,
                        "salience": tc.salience,
                        "qa_type": qt_str,
                        "system": system,
                        "user": format!("Generate a {} QA pair from this passage:\n\n---\n{}\n---\n\nConcepts: {}\n\nInclude this chunk_ref in your output: {}", qt_str, tc.text, tc.concepts.join(", "), tc.entity_ref),
                    });
                    out.push_str(&serde_json::to_string(&prompt).unwrap_or_default());
                    out.push('\n');
                }
                ti += req.prompts_per_chunk;
            }

            std::fs::write(&req.output, &out).map_err(|e| {
                McpToolError::internal(format!("Cannot write output '{}': {e}", req.output))
            })?;

            let result = json!({
                "total_chunks": total,
                "prompts_written": ti,
                "output": req.output,
            });
            self.record_experience(
                "docproc_build_prompts",
                &format!("{} prompts from {} chunks", ti, total),
                "success",
                result.clone(),
            );
            Ok(result)
        })
        .await
    }

    // ── Ingest QA ─────────────────────────────────────────────────────────

    #[tool(
        description = "Ingest generated QA pairs: parse, quality-filter, exact-match dedup (case-insensitive on instruction), write training JSONL, store QA h_mems with 5W1H dimension + Dublin Core / PKO metadata. Semantic dedup (SemDeDup K-means) was removed — see the inline rationale."
    )]
    pub async fn docproc_ingest_qa(&self, Parameters(req): Parameters<IngestQaRequest>) -> String {
        execute_tool(self, "docproc_ingest_qa", async {
            let content = std::fs::read_to_string(&req.generated_jsonl).map_err(|e| {
                McpToolError::invalid_argument(format!("Cannot read generated_jsonl '{}': {e}", req.generated_jsonl))
            })?;

            // Parse QA records — handle both flat and envelope formats
            let mut malformed = 0usize;
            let qas: Vec<ParsedQa> = content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|line| parse_qa_record(line).or_else(|| {
                    malformed += 1;
                    None
                }))
                .collect();
            tracing::info!("  Parsed: {} ({} malformed rejected)", qas.len(), malformed);

            // Quality filter
            let filtered: Vec<&ParsedQa> = qas
                .iter()
                .filter(|q| {
                    q.instruction.len() >= 30
                        && q.output.len() >= 50
                        && !q.qa_type.is_empty()
                        && q.chunk_ref.is_some()
                })
                .collect();
            tracing::info!(
                "  Quality filter: {} (removed {})",
                filtered.len(),
                qas.len() - filtered.len()
            );

            // Exact-match dedup (case-insensitive on instruction).
            //
            // Semantic dedup (SemDeDup: embed → k-means → within-cluster cosine
            // dedup) was removed from this path. At corpus scale the naive
            // single-threaded O(N·K) K-means with K=2.5%·N was pathologically
            // slow (~hours for 230K QAs) and defeated SemDeDup's own
            // cheaper-than-O(N²) premise; the survivor heuristic (keep shortest
            // instruction) also degraded quality. Exact-dedup measured
            // <0.01% duplicates on this corpus. If semantic near-dup removal is
            // later shown to matter, use MinHash/LSH on instructions or an ANN
            // index on stored QA embeddings — not the O(N·K) K-means.
            let mut deduped: Vec<&ParsedQa> = Vec::new();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for qa in &filtered {
                if seen.insert(qa.instruction.to_lowercase()) {
                    deduped.push(qa);
                }
            }

            let deduped_count = deduped.len();
            tracing::info!("  Deduped: {} (removed {})", deduped_count, filtered.len() - deduped_count);

            if req.dry_run {
                return Ok(json!({
                    "parsed": qas.len(),
                    "filtered": filtered.len(),
                    "deduped": deduped_count,
                    "dry_run": true,
                }));
            }

            // Write training JSONL
            let train: String = deduped.iter().map(|q| {
                serde_json::to_string(&serde_json::json!({"instruction": q.instruction, "input": "", "output": q.output}))
                    .unwrap_or_default()
            }).collect::<Vec<_>>().join("\n");
            std::fs::write(&req.output, train + "\n").map_err(|e| {
                McpToolError::internal(format!("Cannot write output '{}': {e}", req.output))
            })?;
            tracing::info!("  Wrote: {} QAs to {}", deduped_count, req.output);

            // Store h_mems + embeddings
            let dim = embedding_dim();
            let semantic = SemanticMemory::open(&req.db_path, &req.passphrase, dim)
                .map_err(|e| McpToolError::failed_precondition(format!("Cannot open memory DB: {e}")))?;
            let webid = owner_webid(&req.owner);
            let mut stored = 0usize;

            for (i, qa) in deduped.iter().enumerate() {
                let entity = format!("training:qa:{}:{}:{}", req.dataset, qa.source, i);
                let v = serde_json::json!({
                    "question": qa.instruction,
                    "answer": qa.output,
                    "bloom_level": qa.qa_type,
                    "source": qa.source,
                    "dataset": req.dataset,
                    "difficulty": qa.difficulty,
                    "concepts": qa.concepts,
                    "chunk_ref": qa.chunk_ref,
                    "evidence_quotes": qa.evidence_quotes,
                    "ontology": {
                        "dimension": "what",
                        "anchor": "dual_axis",
                        "dc_type": "bibo:Document",
                        "dc_source": qa.source,
                        "dc_subject": qa.concepts,
                        "pko_produced_by": "docproc_generate_qa",
                        "pko_extracted_from": qa.chunk_ref,
                    },
                });
                let h_mem = hkask_storage::HMem::new(&entity, "training_qa_pair", v, webid)
                    .with_visibility(hkask_types::Visibility::Public)
                    .with_confidence(0.8)
                    .with_dimension(hkask_types::Dimension::What);
                if semantic.store(h_mem).is_ok() {
                    stored += 1;
                }
            }
            tracing::info!("  Stored: {} QA h_mems", stored);

            let result = json!({
                "parsed": qas.len(),
                "filtered": filtered.len(),
                "deduped": deduped_count,
                "stored_h_mems": stored,
                "output": req.output,
            });
            self.record_experience(
                "docproc_ingest_qa",
                &format!("{} deduped from {}", deduped_count, qas.len()),
                "success",
                result.clone(),
            );
            Ok(result)
        })
        .await
    }
}

// ── Build Prompts helpers ─────────────────────────────────────────────────

// ── Corpus pipeline request structs ───────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DedupChunksRequest {
    /// Path to tagged chunks JSONL (from salience phase).
    pub tagged_jsonl: String,
    /// Output path for deduplicated tagged chunks JSONL.
    pub output: String,
    /// Path to the SQLCipher memory DB containing chunk embeddings.
    pub db_path: String,
    /// Passphrase for the memory DB.
    pub passphrase: String,
    /// Entity-ref prefix for chunk embeddings in the DB (e.g. "corpus:researcher:").
    #[serde(default = "default_corpus_prefix")]
    pub prefix: String,
    /// Cosine similarity threshold — chunks above this are near-duplicates.
    #[serde(default = "default_dedup_threshold")]
    pub threshold: f64,
    /// If true, only report clustering stats without writing output.
    #[serde(default)]
    pub dry_run: bool,
}

fn default_corpus_prefix() -> String {
    "corpus:researcher:".to_string()
}

fn default_dedup_threshold() -> f64 {
    0.85
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConsolidateChunksRequest {
    /// Path to tagged chunks JSONL (from dedup or salience phase).
    pub tagged_jsonl: String,
    /// Output path for consolidated tagged chunks JSONL.
    pub output: String,
    /// Path to the SQLCipher memory DB.
    pub db_path: String,
    /// Passphrase for the memory DB.
    pub passphrase: String,
    /// Entity-ref prefix for chunk embeddings.
    #[serde(default = "default_corpus_prefix")]
    pub prefix: String,
    /// Cosine similarity threshold for clustering (0.75 = semantic overlap).
    #[serde(default = "default_consolidate_threshold")]
    pub threshold: f64,
    /// Max concurrent LLM consolidation calls.
    #[serde(default = "default_consolidate_concurrency")]
    pub concurrency: usize,
    /// Max chunks per consolidation cluster (limits LLM context).
    #[serde(default = "default_max_chunks_per_cluster")]
    pub max_chunks_per_cluster: usize,
    /// If true, only report clustering stats without LLM calls.
    #[serde(default)]
    pub dry_run: bool,
}

fn default_consolidate_threshold() -> f64 {
    0.75
}

fn default_consolidate_concurrency() -> usize {
    12
}

fn default_max_chunks_per_cluster() -> usize {
    5
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BuildPromptsRequest {
    /// Path to tagged chunks JSONL (from consolidate phase).
    pub tagged_jsonl: String,
    /// Output path for prompts JSONL (one JSON per line, consumed by generate_qa_batch).
    pub output: String,
    /// Path to the SQLCipher memory DB for embedding retrieval + h_mem knowledge graph.
    pub db_path: String,
    /// Passphrase for the memory DB.
    pub passphrase: String,
    /// Number of KNN context passages to retrieve per chunk (default 3).
    #[serde(default = "default_context_k")]
    pub context_k: usize,
    /// Number of Bloom-level QA prompts per chunk (default 5 — one per level).
    #[serde(default = "default_prompts_per_chunk")]
    pub prompts_per_chunk: usize,
    /// Bloom's taxonomy weight distribution (e.g. "1,1,1,1,1" = equal).
    #[serde(default = "default_type_distribution")]
    pub type_distribution: String,
    /// Generate cross-reference synthesis prompts.
    #[serde(default)]
    pub cross_reference: bool,
    /// Max prompts to output (0 = all qualifying chunks).
    #[serde(default)]
    pub max_prompts: usize,
    /// Owner persona for h_mem queries (e.g. "john-brooks").
    #[serde(default = "default_owner")]
    pub owner: String,
    /// Per-ontology Bloom distribution overrides. Format:
    /// "golem:0,1,2,1,1|fibo:2,2,1,0,0|pko:1,1,1,2,0|eso:1,1,2,1,0"
    /// When a chunk's ontology_tags contain the key, use the override
    /// instead of the default type_distribution. Chunks without matching
    /// ontology tags use type_distribution.
    #[serde(default)]
    pub ontology_bloom_overrides: Option<String>,
}

fn default_context_k() -> usize {
    3
}

fn default_prompts_per_chunk() -> usize {
    5
}

fn default_type_distribution() -> String {
    "1,1,1,1,1".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IngestQaRequest {
    /// Path to generated QAs JSONL (from docproc_generate_qa_batch).
    pub generated_jsonl: String,
    /// Output path for training-ready JSONL (instruction/input/output per line).
    pub output: String,
    /// Path to the SQLCipher memory DB for h_mem + embedding storage.
    pub db_path: String,
    /// Passphrase for the memory DB.
    pub passphrase: String,
    /// If true, validate and dedup without storing.
    #[serde(default)]
    pub dry_run: bool,
    /// Dataset name for training_qa_pair h_mems.
    #[serde(default = "default_dataset")]
    pub dataset: String,
    /// Owner persona for stored h_mems (e.g. "john-brooks").
    #[serde(default = "default_owner")]
    pub owner: String,
}

fn default_dataset() -> String {
    "capabilities-researcher".to_string()
}

// ── Training dataset preparation ───────────────────────────────────────────

/// Request for `docproc_prepare_training_dataset`.
///
/// Converts Alpaca-format JSONL (from `docproc_ingest_qa`) to ChatML format
/// (what `training_submit` expects), applies the lora-training skill's G-D1
/// gate (dataset size check), and returns lora-training config recommendations.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrepareTrainingDatasetRequest {
    /// Path to Alpaca-format JSONL (from docproc_ingest_qa).
    /// Each line: {"instruction": "...", "input": "", "output": "..."}
    pub input_jsonl: String,
    /// Output path for ChatML-format JSONL (for training_submit).
    /// Each line: {"messages": [{"role": "user", ...}, {"role": "assistant", ...}]}
    pub output_jsonl: String,
    /// Optional system prompt to prepend to each conversation.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Base model the dataset will be used to fine-tune (e.g., "Qwen/Qwen2.5-7B").
    /// Used to generate lora-training config recommendations.
    pub base_model: String,
    /// If true, convert and validate without writing the output file.
    #[serde(default)]
    pub dry_run: bool,
}

impl DocProcServer {
    /// Prepare a training dataset from corpus QA pairs for LoRA fine-tuning.
    ///
    /// This tool bridges the docproc corpus pipeline and the training server:
    /// 1. Reads Alpaca-format JSONL from `docproc_ingest_qa`
    /// 2. Converts to ChatML format (what `training_submit` expects)
    /// 3. Applies the lora-training skill's G-D1 gate (dataset size check)
    /// 4. Returns lora-training config recommendations (rank, alpha, QLoRA)
    ///
    /// The config recommendations are derived from the lora-training skill's
    /// 5-gate decision (G1 inference, G2 memory, G3 task distance, G4 quality,
    /// G5 knowledge preservation) using the base model size and dataset stats.
    #[tool(
        description = "Convert Alpaca-format QA JSONL to ChatML training format, apply lora-training G-D1 dataset size gate, and return PEFT config recommendations (rank, alpha, QLoRA, init strategy) based on the base model and dataset characteristics. Bridges the docproc corpus pipeline to the training server."
    )]
    pub async fn docproc_prepare_training_dataset(
        &self,
        Parameters(req): Parameters<PrepareTrainingDatasetRequest>,
    ) -> String {
        execute_tool(self, "docproc_prepare_training_dataset", async {
            let content = std::fs::read_to_string(&req.input_jsonl).map_err(|e| {
                McpToolError::invalid_argument(format!(
                    "Cannot read input JSONL '{}': {e}",
                    req.input_jsonl
                ))
            })?;

            // Parse Alpaca-format lines and convert to ChatML
            let mut chatml_lines: Vec<String> = Vec::new();
            let mut parse_errors: Vec<serde_json::Value> = Vec::new();
            let mut total_tokens_approx: usize = 0;

            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<serde_json::Value>(trimmed) {
                    Ok(v) => {
                        let instruction = v.get("instruction")
                            .and_then(|i| i.as_str())
                            .unwrap_or("");
                        let input = v.get("input")
                            .and_then(|i| i.as_str())
                            .unwrap_or("");
                        let output = v.get("output")
                            .and_then(|o| o.as_str())
                            .unwrap_or("");

                        if instruction.is_empty() || output.is_empty() {
                            parse_errors.push(json!({
                                "line": i + 1,
                                "error": "missing instruction or output"
                            }));
                            continue;
                        }

                        // Build the user message (combine instruction + input if present)
                        let user_content = if input.is_empty() {
                            instruction.to_string()
                        } else {
                            format!("{instruction}\n\n{input}")
                        };

                        // Build the ChatML conversation
                        let mut messages: Vec<serde_json::Value> = Vec::new();
                        if let Some(ref sp) = req.system_prompt {
                            messages.push(json!({"role": "system", "content": sp}));
                        }
                        messages.push(json!({"role": "user", "content": user_content}));
                        messages.push(json!({"role": "assistant", "content": output}));

                        let chatml = json!({"messages": messages});
                        chatml_lines.push(serde_json::to_string(&chatml).unwrap_or_default());

                        // Approximate token count (1 token ≈ 4 chars)
                        total_tokens_approx += (user_content.len() + output.len()) / 4;
                    }
                    Err(e) => {
                        parse_errors.push(json!({
                            "line": i + 1,
                            "error": format!("JSON parse error: {e}")
                        }));
                    }
                }
            }

            let n_samples = chatml_lines.len();

            // G-D1: Dataset size gate (from lora-training skill)
            let mut gd1_warnings: Vec<String> = Vec::new();
            if n_samples < 1000 {
                gd1_warnings.push(format!(
                    "G-D1 WARN: dataset has only {} examples — QLoRA paper §5 recommends small high-quality, but <1000 may be insufficient",
                    n_samples
                ));
            }
            if n_samples > 100_000 {
                gd1_warnings.push(format!(
                    "G-D1 WARN: dataset has {} examples — large datasets require quality audit (dedup, contamination)",
                    n_samples
                ));
            }

            // Generate lora-training config recommendations
            // based on the 5-gate decision (G1-G5)
            let lower = req.base_model.to_lowercase();
            let model_size_b: u32 = if ["1b", "3b"].iter().any(|p| lower.contains(p)) {
                1
            } else if ["7b", "8b", "9b"].iter().any(|p| lower.contains(p)) {
                8
            } else if ["13b", "14b"].iter().any(|p| lower.contains(p)) {
                14
            } else if ["30b", "34b", "35b"].iter().any(|p| lower.contains(p)) {
                35
            } else if ["70b", "72b"].iter().any(|p| lower.contains(p)) {
                70
            } else {
                8 // default
            };

            // G2: Memory budget — model_size × 2 (bf16) > 24GB → QLoRA
            let use_qlora = (model_size_b * 2) > 24;

            // G3: Task distance — QA pairs from corpus are "moderate" (new domain knowledge)
            let recommended_r = if n_samples < 1000 { 16 } else { 32 };
            let recommended_alpha = recommended_r * 2;

            // G4: Quality/cost — default LoRA (not DoRA/PiSSA)
            let recommended_init = "true"; // PEFT default

            // G5: Knowledge preservation — not required for new domain adaptation
            let recommended_use_rslora = recommended_r > 64;

            let config_recommendation = json!({
                "base_model": req.base_model,
                "model_size_b": model_size_b,
                "use_qlora": use_qlora,
                "lora": {
                    "r": recommended_r,
                    "alpha": recommended_alpha,
                    "dropout": 0.0,
                    "target_modules": ["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"],
                    "use_rslora": recommended_use_rslora,
                    "use_dora": false,
                    "init_lora_weights": recommended_init,
                    "bias": "none"
                },
                "quantization": if use_qlora {
                    json!({
                        "load_in_4bit": true,
                        "bnb_4bit_quant_type": "nf4",
                        "bnb_4bit_compute_dtype": "bf16",
                        "bnb_4bit_use_double_quant": true
                    })
                } else {
                    json!({"load_in_4bit": false})
                },
                "optimization": {
                    "optimizer": if use_qlora { "paged_adamw_8bit" } else { "adamw_torch" },
                    "lr_scheduler": "cosine",
                    "gradient_accumulation_steps": 1
                },
                "advanced": {
                    "bf16": true,
                    "gradient_checkpointing": "true"
                },
                "gate_decisions": {
                    "G1_inference": "must-merge (LoRA-family)",
                    "G2_memory": if use_qlora { "QLoRA (NF4)" } else { "LoRA (bf16)" },
                    "G3_task_distance": "moderate (new domain knowledge)",
                    "G4_quality_cost": "default (LoRA with PEFT default init)",
                    "G5_knowledge_preservation": "not required"
                }
            });

            // Write output if not dry run
            if !req.dry_run && !chatml_lines.is_empty() {
                std::fs::write(&req.output_jsonl, chatml_lines.join("\n") + "\n")
                    .map_err(|e| {
                        McpToolError::internal(format!(
                            "Cannot write output '{}': {e}",
                            req.output_jsonl
                        ))
                    })?;
            }

            tracing::info!(
                target: "hkask.docproc.training_dataset_prepared",
                input_path = %req.input_jsonl,
                output_path = %req.output_jsonl,
                n_samples = n_samples,
                approx_tokens = total_tokens_approx,
                use_qlora = use_qlora,
                recommended_r = recommended_r,
                "Training dataset prepared from corpus QA pairs"
            );

            Ok(json!({
                "input_jsonl": req.input_jsonl,
                "output_jsonl": req.output_jsonl,
                "n_samples": n_samples,
                "approx_tokens": total_tokens_approx,
                "parse_errors": parse_errors,
                "parse_error_count": parse_errors.len(),
                "gd1_warnings": gd1_warnings,
                "config_recommendation": config_recommendation,
                "dry_run": req.dry_run,
                "next_step": "Pass output_jsonl to training_submit with the config_recommendation params"
            }))
        })
        .await
    }
}
