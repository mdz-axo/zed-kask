//! Corpus pipeline tools — semantic chunk dedup and consolidation.
//!
//! These tools operate on tagged chunks JSONL (from the salience phase) and
//! the SQLCipher memory DB (containing chunk embeddings). They are the
//! "Phase 2c" and "Phase 2d" quality gates in the corpus pipeline.
//!
//! - `corpus_dedup_chunks`: Removes near-identical chunks (cosine > 0.85)
//!   using stored embeddings. Keeps highest-salience survivor per cluster.
//! - `corpus_consolidate_chunks`: Clusters semantically related chunks
//!   (cosine > 0.75), uses the inference router to LLM-synthesize each
//!   multi-chunk cluster into a single comprehensive passage, re-embeds
//!   the consolidated text, and stores the new embedding in the DB.

use crate::helpers::{default_corpus_passphrase, read_text_capped};
use crate::services::consolidation::{ChunkConsolidationRequest, ConsolidationService};
use crate::services::prompt_builder::{
    BuildPromptsRequest as ServiceBuildPromptsRequest, PromptBuilderService,
};
use crate::{
    Arc, CorpusServer, McpToolError, Parameters, default_owner, execute_tool, json, owner_webid,
    tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

pub(crate) mod clustering;
mod lora_config;
mod qa_parsing;
mod qa_types;

pub(crate) use clustering::read_tagged_chunks;
use lora_config::build_lora_config;
use qa_parsing::{ParsedQa, parse_qa_record};
pub(crate) use qa_types::{QaType, parse_type_distribution, qa_type_instruction, qa_type_str};

// Re-export helpers used by the service layer (services/consolidation.rs,
// services/prompt_builder.rs) so the services don't depend on the private
// submodule paths.

#[tool_router(router = corpus_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Deduplicate chunks by semantic embedding similarity. Queries chunk embeddings from the memory DB, clusters within each source file by cosine similarity > threshold (default 0.85), and keeps the highest-salience chunk per cluster. Writes deduplicated tagged chunks JSONL."
    )]
    pub async fn corpus_dedup_chunks(
        &self,
        Parameters(req): Parameters<DedupChunksRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_dedup_chunks", async {
            let input = crate::services::cluster::load_clusters(
                &req.tagged_jsonl,
                &req.db_path,
                &req.passphrase,
                &req.prefix,
            )?;
            let threshold = req.threshold as f32;
            let all_clusters = input.cluster_by_source(threshold, usize::MAX);
            let chunks = input.chunks;

            let mut keep_indices: Vec<usize> = all_clusters
                .iter()
                .map(|cluster| cluster[0]) // highest-salience survivor
                .collect();
            keep_indices.sort_unstable();
            keep_indices.dedup();

            let sources = chunks
                .iter()
                .map(|c| c.source.as_str())
                .collect::<std::collections::HashSet<&str>>()
                .len();

            let result = json!({
                "original": chunks.len(),
                "deduped": keep_indices.len(),
                "removed": chunks.len() - keep_indices.len(),
                "clusters": all_clusters.len(),
                "sources": sources,
                "reduction_pct": (1.0 - keep_indices.len() as f64 / chunks.len().max(1) as f64) * 100.0,
            });

            if req.dry_run {
                return Ok(result);
            }

            let mut out = String::new();
            for &idx in &keep_indices {
                out.push_str(&serde_json::to_string(&chunks[idx])
                    .map_err(|e| McpToolError::internal(format!("Serialize: {e}")))?); // rr0044-ok: serde serialization of own struct
                out.push('\n');
            }
            crate::helpers::write_contained(&req.output, &out)?;

            Ok(result)
        })
        .await
    }

    #[tool(
        description = "Consolidate semantically related chunks via LLM synthesis. Clusters chunks within each source file by cosine similarity > threshold (default 0.75), then uses the inference router to synthesize each multi-chunk cluster into a single comprehensive passage. Re-embeds consolidated text and stores the new embedding. Writes consolidated tagged chunks JSONL with provenance."
    )]
    pub async fn corpus_consolidate_chunks(
        &self,
        Parameters(req): Parameters<ConsolidateChunksRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_consolidate_chunks", async {
            ConsolidationService::new(Arc::clone(&self.inference_router))
                .consolidate(ChunkConsolidationRequest {
                    tagged_jsonl: req.tagged_jsonl,
                    output: req.output,
                    db_path: req.db_path,
                    passphrase: req.passphrase,
                    prefix: req.prefix,
                    threshold: req.threshold,
                    concurrency: req.concurrency,
                    max_chunks_per_cluster: req.max_chunks_per_cluster,
                    dry_run: req.dry_run,
                })
                .await
        })
        .await
    }

    // ── Build Prompts ──────────────────────────────────────────────────────

    #[tool(
        description = "Build QA generation prompts from tagged chunks with KNN context scaffold, ontology context, and h_mem knowledge graph. For each chunk, retrieves embedding-similar passages (KNN) under the entity-ref prefix (default corpus:researcher: — pass the prefix you chunked under), formats ontology tags (5W1H + Dublin Core + PKO), and queries h_mems from the memory DB to build a knowledge graph section. Outputs prompts JSONL consumed by corpus_generate_qa_batch."
    )]
    pub async fn corpus_build_prompts(
        &self,
        Parameters(req): Parameters<BuildPromptsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_build_prompts", async {
            PromptBuilderService::new()
                .build_prompts(ServiceBuildPromptsRequest {
                    tagged_jsonl: req.tagged_jsonl,
                    output: req.output,
                    db_path: req.db_path,
                    passphrase: req.passphrase,
                    prefix: req.prefix,
                    context_k: req.context_k,
                    prompts_per_chunk: req.prompts_per_chunk,
                    type_distribution: req.type_distribution,
                    max_prompts: req.max_prompts,
                    ontology_bloom_overrides: req.ontology_bloom_overrides,
                })
                .await
        })
        .await
    }

    // ── Ingest QA ─────────────────────────────────────────────────────────

    #[tool(
        description = "Ingest generated QA pairs: parse, quality-filter, exact-match dedup (case-insensitive on instruction), write training JSONL, store QA h_mems with 5W1H dimension + Dublin Core / PKO metadata. Semantic dedup (SemDeDup K-means) was removed — see the inline rationale."
    )]
    pub async fn corpus_ingest_qa(
        &self,
        Parameters(req): Parameters<IngestQaRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_ingest_qa", async {
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
            crate::helpers::write_contained(&req.output, &(train + "\n"))?;
            tracing::info!("  Wrote: {} QAs to {}", deduped_count, req.output);

            // Store h_mems + embeddings
            let store = crate::helpers::open_memory_store(&req.db_path, &req.passphrase)?;
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
                });
                // Dual-axis anchoring (P5.4) in the first-class `ontology`
                // column rather than the value blob: a generated QA pair is
                // both a document (state axis: BIBO type + concepts as
                // subject) and the product of a procedure (process axis:
                // `corpus_generate_qa`, with the source chunk as the step).
                // The value blob's former nested `"ontology"` object was not
                // queryable — this is.
                let mut ontology = hkask_types::HMemOntology::state(
                    hkask_bridge_ontology::dc_bibo::DOCUMENT,
                    qa.concepts.clone(),
                    qa.source.clone(),
                );
                ontology.pko_procedure = Some("corpus_generate_qa".to_string());
                ontology.pko_step = qa.chunk_ref.clone();
                let h_mem = hkask_storage::HMem::new(&entity, "training_qa_pair", v, webid)
                    .with_visibility(hkask_types::Visibility::Public)
                    .with_confidence(0.8)
                    .with_ontology(ontology);
                if store.store(h_mem).is_ok() {
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
            Ok(result)
        })
        .await
    }
    /// Prepare a training dataset from corpus QA pairs for LoRA fine-tuning.
    ///
    /// This tool bridges the docproc corpus pipeline and the training server:
    /// 1. Reads Alpaca-format JSONL from `corpus_ingest_qa`
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
    pub async fn corpus_prepare_training_dataset(
        &self,
        Parameters(req): Parameters<PrepareTrainingDatasetRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_prepare_training_dataset", async {
            // Note: this site intentionally does NOT use `read_jsonl`/`read_jsonl_lenient`.
            // It collects per-line parse errors (with line numbers) into the tool
            // response (`parse_errors`), which is part of the external API. The
            // shared helpers either propagate the first error (strict) or drop
            // silently (lenient) — neither preserves the multi-error report.
            // Containment + size cap are still enforced via `read_text_capped`.
            let content = read_text_capped(&req.input_jsonl, "input_jsonl")?;

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

            // Generate a preview PEFT config recommendation based on the 5-gate
            // heuristic (G1-G5). This is a PREVIEW — the authoritative
            // recommendation comes from the `lora-training` skill's full 8-gate
            // refinement (G0, G-D0, G1-G6) with operator accept/override. The
            // operator should invoke `lora-training/select-method` before
            // training. See `lora_config.rs` doc for the drift hazard.
            tracing::info!(
                target: "hkask.corpus.training_dataset",
                base_model = %req.base_model,
                n_samples,
                "PEFT config preview generated — invoke the lora-training skill for the authoritative 8-gate recommendation before training",
            );
            let config_recommendation = build_lora_config(&req.base_model, n_samples);
            let use_qlora = config_recommendation
                .get("use_qlora")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let recommended_r = config_recommendation
                .get("lora")
                .and_then(|l| l.get("r"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;

            // Write output if not dry run
            if !req.dry_run && !chatml_lines.is_empty() {
                crate::helpers::write_contained(
                    &req.output_jsonl,
                    &(chatml_lines.join("\n") + "\n"),
                )?;
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

// ── Build Prompts helpers ─────────────────────────────────────────────────

// ── Corpus pipeline request structs ───────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DedupChunksRequest {
    /// Path to tagged chunks JSONL (from salience phase).
    pub tagged_jsonl: String,
    /// Output path for deduplicated tagged chunks JSONL.
    pub output: String,
    /// Path to the SQLCipher memory DB containing chunk embeddings.
    pub db_path: String,
    /// Passphrase for the memory DB.
    #[serde(default = "default_corpus_passphrase")]
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
pub(crate) struct ConsolidateChunksRequest {
    /// Path to tagged chunks JSONL (from dedup or salience phase).
    pub tagged_jsonl: String,
    /// Output path for consolidated tagged chunks JSONL.
    pub output: String,
    /// Path to the SQLCipher memory DB.
    pub db_path: String,
    /// Passphrase for the memory DB.
    #[serde(default = "default_corpus_passphrase")]
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
    crate::max_concurrency()
}

fn default_max_chunks_per_cluster() -> usize {
    5
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct BuildPromptsRequest {
    /// Path to tagged chunks JSONL (from consolidate phase).
    pub tagged_jsonl: String,
    /// Output path for prompts JSONL (one JSON per line, consumed by generate_qa_batch).
    pub output: String,
    /// Path to the SQLCipher memory DB for embedding retrieval + h_mem knowledge graph.
    pub db_path: String,
    /// Passphrase for the memory DB.
    #[serde(default = "default_corpus_passphrase")]
    pub passphrase: String,
    /// Entity-ref prefix for the KNN embedding lookup (default
    /// "corpus:researcher:"). Pass the prefix you chunked under — any other
    /// prefix silently yields no embedding context.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Number of KNN context passages to retrieve per chunk (default 3).
    #[serde(default = "default_context_k")]
    pub context_k: usize,
    /// Number of Bloom-level QA prompts per chunk (default 5 — one per level).
    #[serde(default = "default_prompts_per_chunk")]
    pub prompts_per_chunk: usize,
    /// Bloom's taxonomy weight distribution (e.g. "1,1,1,1,1" = equal).
    #[serde(default = "default_type_distribution")]
    pub type_distribution: String,
    /// Max prompts to output (0 = all qualifying chunks).
    #[serde(default)]
    pub max_prompts: usize,
    /// Per-ontology Bloom distribution overrides. Format:
    /// "golem:0,1,2,1,1|fibo:2,2,1,0,0|pko:1,1,1,2,0|sepio:1,1,2,1,0"
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
    /// Path to generated QAs JSONL (from corpus_generate_qa_batch).
    pub generated_jsonl: String,
    /// Output path for training-ready JSONL (instruction/input/output per line).
    pub output: String,
    /// Path to the SQLCipher memory DB for h_mem + embedding storage.
    pub db_path: String,
    /// Passphrase for the memory DB.
    #[serde(default = "default_corpus_passphrase")]
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

/// Request for `corpus_prepare_training_dataset`.
///
/// Converts Alpaca-format JSONL (from `corpus_ingest_qa`) to ChatML format
/// (what `training_submit` expects), applies the lora-training skill's G-D1
/// gate (dataset size check), and returns lora-training config recommendations.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PrepareTrainingDatasetRequest {
    /// Path to Alpaca-format JSONL (from corpus_ingest_qa).
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
