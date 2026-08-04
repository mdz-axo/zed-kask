//! Prompt builder service — KNN + concept graph + knowledge graph + QA prompts.
//!
//! Extracted from `CorpusServer::corpus_build_prompts` in `tools/corpus/mod.rs`.
//! Builds QA generation prompts with source-scoped KNN context, ontology context,
//! and h_mem knowledge graph sections.

use hkask_mcp_server::server::McpToolError;
use hkask_memory::SemanticMemory;
use hkask_types::corpus::TaggedChunk;
use serde_json::json;

use crate::helpers::map_corpus_io_error;
use crate::tools::corpus::{
    QaType, parse_type_distribution, qa_type_instruction, qa_type_str, read_tagged_chunks,
};
use crate::{embedding_dim, normalize_in_place, render_docproc_template};

/// Input for [`PromptBuilderService::build_prompts`].
pub struct BuildPromptsRequest {
    pub tagged_jsonl: String,
    pub output: String,
    pub db_path: String,
    pub passphrase: String,
    pub context_k: usize,
    pub prompts_per_chunk: usize,
    pub type_distribution: String,
    pub cross_reference: bool,
    pub max_prompts: usize,
    pub owner: String,
    pub ontology_bloom_overrides: Option<String>,
}

/// KNN context + concept graph + knowledge graph + QA prompt builder.
///
/// Each call to [`build_prompts`] reads tagged chunks, loads embeddings from
/// the memory DB, and writes QA prompts JSONL. No inference router is needed —
/// the method queries the DB for pre-computed embeddings, not the inference API.
pub struct PromptBuilderService;

impl PromptBuilderService {
    pub fn new() -> Self {
        Self
    }

    /// Build QA generation prompts from tagged chunks.
    ///
    /// For each chunk, retrieves embedding-similar passages (KNN), formats
    /// ontology tags (5W1H + Dublin Core + PKO), and queries h_mems from the
    /// memory DB to build a knowledge graph section. Outputs prompts JSONL
    /// consumed by `corpus_generate_qa_batch`.
    #[must_use = "result must be used"]
    pub async fn build_prompts(
        &self,
        request: BuildPromptsRequest,
    ) -> Result<serde_json::Value, McpToolError> {
        let BuildPromptsRequest {
            tagged_jsonl,
            output,
            db_path,
            passphrase,
            context_k,
            prompts_per_chunk,
            type_distribution,
            cross_reference: _, // accepted but not yet wired — see note below
            max_prompts,
            owner: _, // accepted but not yet wired — see note below
            ontology_bloom_overrides,
        } = request;

        let chunks = read_tagged_chunks(&tagged_jsonl)?;
        if chunks.is_empty() {
            return Err(McpToolError::invalid_argument("tagged_jsonl is empty"));
        }
        let total = chunks.len();
        tracing::info!("  Build prompts: {} chunks", total);

        // QA type rotation — default distribution
        let default_rotation = parse_type_distribution(&type_distribution);

        // Parse per-ontology Bloom overrides (if provided)
        // Format: "golem:0,1,2,1,1|fibo:2,2,1,0,0|pko:1,1,1,2,0|eso:1,1,2,1,0"
        let bloom_overrides: std::collections::HashMap<String, Vec<QaType>> =
            ontology_bloom_overrides
                .as_deref()
                .map(|s| {
                    s.split('|')
                        .filter_map(|entry| {
                            let (ns, dist) = entry.split_once(':')?;
                            let vals = parse_type_distribution(dist);
                            if vals.is_empty() {
                                None
                            } else {
                                Some((ns.to_string(), vals))
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
        let limit = if max_prompts > 0 {
            max_prompts.min(total)
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
        let semantic = SemanticMemory::open(&db_path, &passphrase, dim).map_err(|e| {
            McpToolError::failed_precondition(format!("Cannot open memory DB: {e}"))
        })?;

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

        // Build concept graph (concept -> chunk_count)
        let mut concept_connections: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for chunk in &chunks {
            for concept in &chunk.concepts {
                *concept_connections.entry(concept.as_str()).or_default() += 1;
            }
        }

        // `cross_reference` and `owner` are accepted by the request struct but
        // not yet wired into prompt generation. `cross_reference` is intended to
        // enable cross-chunk synthesis prompts; `owner` would tag generated
        // prompts with a WebID. Both are kept in the schema for forward-compat and
        // ignored here — not yet enforced.

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
                    let k = context_k;
                    let candidates = emb_by_source
                        .get(tc.source.as_str())
                        .map(|v| v.as_slice())
                        .unwrap_or(&[]);
                    let mut scored: Vec<(&String, f32)> = candidates
                        .iter()
                        .filter(|(er, _)| er.as_str() != tc.entity_ref)
                        .map(|(er, v)| {
                            // Vectors are pre-normalized, so dot product = cosine similarity.
                            let cosine_sim: f32 =
                                query_vec.iter().zip(v.iter()).map(|(a, b)| a * b).sum();
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
                        scored[..k].sort_by(|a, b| {
                            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        scored.into_iter().take(k).collect()
                    } else {
                        scored.sort_by(|a, b| {
                            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                        });
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
                        format!(
                            "[{}] Source: {}, Similarity: {:.2}\n    {}",
                            i + 1,
                            source,
                            sim,
                            truncated
                        )
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
            let concept_graph_text = tc
                .concepts
                .iter()
                .map(|concept| {
                    let connected = concept_connections
                        .get(concept.as_str())
                        .copied()
                        .unwrap_or(1);
                    format!("- {} (connected to {} chunks)", concept, connected)
                })
                .collect::<Vec<_>>()
                .join("\n");
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
                        if h_mem.attribute == "text"
                            || h_mem.attribute == "corpus_provenance"
                            || h_mem.attribute == "ontology_tags"
                        {
                            continue; // skip non-triple h_mems
                        }
                        let dim = h_mem
                            .dimension
                            .as_ref()
                            .map(|d| d.as_str())
                            .unwrap_or("what");
                        let conf = format!("{:.2}", h_mem.confidence.value());
                        // v2: value is {"subject": "...", "object": "..."}
                        let (subj, obj) = match &h_mem.value {
                            serde_json::Value::Object(map) => {
                                let s = map.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                                let o = map
                                    .get("object")
                                    .map(|v| match v {
                                        serde_json::Value::String(s) => s.clone(),
                                        v => v.to_string(),
                                    })
                                    .unwrap_or_default();
                                (s.to_string(), o)
                            }
                            // Legacy: value is the object directly
                            serde_json::Value::String(s) => (String::new(), s.clone()),
                            v => (String::new(), v.to_string()),
                        };
                        let entity_label = if subj.is_empty() {
                            tc.entity_ref.as_str()
                        } else {
                            &subj
                        };
                        lines.push(format!(
                            "  - [{}] (conf={}) {} --{}--> {}",
                            dim, conf, entity_label, h_mem.attribute, obj
                        ));
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
                    if tc.ontology_tags.contains_key(*ns) && bloom_overrides.contains_key(*ns) {
                        selected = Some(&bloom_overrides[*ns]);
                        break;
                    }
                }
                selected.unwrap_or(&default_rotation)
            };

            for offset in 0..prompts_per_chunk {
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
                let mut vars: std::collections::HashMap<&str, String> =
                    std::collections::HashMap::new();
                vars.insert("qa_instruction", qa_type_instruction(qt).to_string());
                vars.insert("dimensions", dimensions_str.clone());
                vars.insert("qa_type", qt_str.to_string());
                vars.insert("expertise", expertise.to_string());
                vars.insert("source", tc.source.clone());
                vars.insert("dc_type", dc_type.to_string());
                vars.insert("dc_subject", dc_subject.clone());
                vars.insert("consolidated_from", consolidated_from);
                vars.insert(
                    "ontology_tags",
                    if tags_str.is_empty() {
                        "(none)".to_string()
                    } else {
                        tags_str.clone()
                    },
                );
                vars.insert("context_passages", context_text.clone());
                vars.insert("concept_graph", concept_graph_text.clone());
                vars.insert("knowledge_graph", kg_text.clone());
                let system = render_docproc_template("build-prompts", &vars);
                let system = if system.is_empty() {
                    // Fallback if template not found
                    format!(
                        "You are a Capabilities Researcher training data generator. Given a primary passage from capabilities and research literature, generate ONE question-answer pair. Calibrate question depth to the expertise level indicated below.\n\n{}\n\n## Ontological Context\n5W1H: [{}]. QA at {} for {} expertise.\nSource: {}. Tags: {}\n\n## Context Passages\n{}\n\n## Knowledge Graph\n{}",
                        qa_type_instruction(qt),
                        dimensions_str,
                        qt_str,
                        expertise,
                        tc.source,
                        if tags_str.is_empty() {
                            "(none)"
                        } else {
                            &tags_str
                        },
                        context_text,
                        kg_text
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
            ti += prompts_per_chunk;
        }

        let output_path = crate::path_safety::contain_for_write(&output)?;
        std::fs::write(&output_path, &out)
            .map_err(|e| map_corpus_io_error(e, &format!("Cannot write output '{}'", output)))?;

        let result = json!({
            "total_chunks": total,
            "prompts_written": ti,
            "output": output,
        });
        Ok(result)
    }
}

impl Default for PromptBuilderService {
    fn default() -> Self {
        Self::new()
    }
}
