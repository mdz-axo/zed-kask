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

use crate::helpers::read_text_capped;
use crate::services::consolidation::{ChunkConsolidationRequest, ConsolidationService};
use crate::services::prompt_builder::{
    BuildPromptsRequest as ServiceBuildPromptsRequest, PromptBuilderService,
};
use crate::services::qa_grounding::{GroundQaRequest, QA_GROUNDING_PROTOCOL};
use crate::{
    Arc, CorpusServer, McpToolError, Parameters, default_owner, execute_tool, json, owner_webid,
    tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

pub(crate) mod clustering;
#[cfg(test)]
mod ingest_tests;
pub(crate) mod qa_parsing;
mod qa_types;

pub(crate) use clustering::read_tagged_chunks;
use qa_parsing::{ParsedQa, QaRecordError, parse_qa_record};
pub(crate) use qa_types::{QaType, parse_type_distribution, qa_type_instruction};

/// Derive one gated candidate's grounding metadata: the prompt_id (required
/// — a gated candidate cannot have lost it), the bundle row key, and the
/// re-executed answer provenance the gate recorded for that row. The single
/// derivation for both `corpus_ingest_qa` loops (QA export rows and h_mem
/// values) — previously duplicated verbatim, risking the two views drifting
/// apart on a shape change.
fn grounding_fields(
    qa: &ParsedQa,
    line: usize,
    gate: &crate::services::qa_grounding::GroundingGateReport,
) -> Result<(String, String), McpToolError> {
    let prompt_id = qa
        .prompt_id
        .as_deref()
        .ok_or_else(|| McpToolError::internal("Gated candidate lost its prompt_id"))?;
    let row_key = crate::services::qa_grounding::candidate_row_key(prompt_id, &qa.qa_type, line);
    let answer_provenance = gate
        .answer_provenance
        .get(&row_key)
        .cloned()
        .ok_or_else(|| {
            McpToolError::internal(format!(
                "Gated candidate '{row_key}' has no re-executed answer provenance"
            ))
        })?;
    Ok((row_key, answer_provenance))
}

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
            let passphrase = crate::helpers::resolve_corpus_passphrase()?;
            crate::helpers::ensure_db_exists(&req.db_path)?;
            let input = crate::services::cluster::load_clusters(
                &req.tagged_jsonl,
                &req.db_path,
                &passphrase,
                &req.prefix,
            )?;
            if input.dropped_malformed > 0 {
                tracing::warn!(
                    dropped = input.dropped_malformed,
                    "tagged_jsonl contained malformed lines; deduplicating the parseable subset"
                );
            }
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
                "dropped_malformed_lines": input.dropped_malformed,
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
            crate::helpers::write_contained(&req.output, &out)?;

            Ok(result)
        })
        .await
    }

    #[tool(
        description = "Consolidate semantically related chunks via LLM synthesis. Clusters chunks within each source file by cosine similarity > threshold (default 0.75), then uses the inference router to synthesize each multi-chunk cluster into a single comprehensive passage. Re-embeds annotated synthesis while storing original synthesized passage_text and publishing it to warm retrieval; durable storage retains that text across restarts. Repeated DB/ref writes replace prior embeddings; original source entities remain. Clear/purge cancels overlapping in-flight publication with an error. Writes consolidated tagged chunks JSONL with provenance."
    )]
    pub async fn corpus_consolidate_chunks(
        &self,
        Parameters(req): Parameters<ConsolidateChunksRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_consolidate_chunks", async {
            let passphrase = crate::helpers::resolve_corpus_passphrase()?;
            crate::helpers::ensure_db_exists(&req.db_path)?;
            ConsolidationService::new(
                Arc::clone(&self.inference_router),
                Arc::clone(&self.index),
                self.webid,
            )
            .consolidate(ChunkConsolidationRequest {
                tagged_jsonl: req.tagged_jsonl,
                output: req.output,
                db_path: req.db_path,
                passphrase,
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
        description = "Build compact prepared QA requests from classified chunks. One request can produce several Bloom-level pairs; canonical passage identities remain server-side and the model receives only local IDs. context_k defaults to 0 (primary only); db_path is required only when context_k > 0 (the DB passphrase resolves server-side and is never model-supplied). qa_pairs_per_chunk defaults to 2, max_pairs caps requested pairs, and old rendered-message prompt files are rejected."
    )]
    pub async fn corpus_build_prompts(
        &self,
        Parameters(req): Parameters<BuildPromptsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_build_prompts", async {
            // The credential is only needed when context_k > 0 resolves
            // neighbors from the DB — primary-only builds stay usable
            // without a resolvable passphrase (same laziness as before
            // the field removal).
            let passphrase = if req.context_k > 0 {
                Some(crate::helpers::resolve_corpus_passphrase()?)
            } else {
                None
            };
            // The DB is read only when context_k > 0 resolves neighbors —
            // check existence exactly there (primary-only builds never
            // touch it).
            if req.context_k > 0
                && let Some(path) = req.db_path.as_deref()
            {
                crate::helpers::ensure_db_exists(path)?;
            }
            PromptBuilderService::new()
                .build_prompts(ServiceBuildPromptsRequest {
                    tagged_jsonl: req.tagged_jsonl,
                    output: req.output,
                    db_path: req.db_path,
                    passphrase,
                    prefix: req.prefix,
                    context_k: req.context_k,
                    qa_pairs_per_chunk: req.qa_pairs_per_chunk,
                    type_distribution: req.type_distribution,
                    max_pairs: req.max_pairs,
                })
                .await
        })
        .await
    }

    // ── Ingest QA ─────────────────────────────────────────────────────────

    #[tool(
        description = "Ingest generated QA candidates behind the corpus-qa-grounding-v1 gate. Fails closed on partial input (malformed, generator-error, or structurally incomplete rows). Before dedup, output, or DB access, the gate re-hashes the bundle, checks row bijection, requires sources classified under the current published-ontology protocol, recomputes ontology resolutions, re-executes every mechanical check — evidence quotes byte-exact in uniquely identified canonical chunks — requires each artifact row to equal its re-execution, requires every citation claim to be tool-verified or platform-derived and at least one to exist, and admits a synthesized answer that is not byte-exact as a model-mediated observation persisted with answer_provenance=model_inference; a model_inference answer is never relabelled verified. Case-insensitive exact-instruction dedup keeps the first valid row. Preserves evidence, source metadata, grounding manifest identity, and canonical ontology in QA export JSONL and QA h_mems. Reports reconciled row counts and explicit partial storage failures. Dry-run validates without storing. Dataset and owner must be nonblank. Existing records retain the legacy training:qa:{dataset}: entity prefix and training_qa_pair attribute for storage compatibility only; no training executor consumes them."
    )]
    pub async fn corpus_ingest_qa(
        &self,
        Parameters(req): Parameters<IngestQaRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_ingest_qa", async {
            for (name, value) in [("dataset", &req.dataset), ("owner", &req.owner)] {
                if value.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(format!(
                        "{name} must be nonblank"
                    )));
                }
            }
            let content = read_text_capped(&req.generated_jsonl, "generated_jsonl")?;

            let mut total_nonblank_rows = 0usize;
            let mut generator_errors = 0usize;
            let mut malformed = 0usize;
            let mut qas: Vec<(usize, ParsedQa)> = Vec::new();
            let mut malformed_lines: Vec<usize> = Vec::new();
            let mut first_malformed_detail: Option<String> = None;
            for (index, line) in content.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                total_nonblank_rows += 1;
                match parse_qa_record(line) {
                    Ok(qa) => qas.push((index + 1, qa)),
                    Err(QaRecordError::GeneratorError) => generator_errors += 1,
                    Err(QaRecordError::Malformed(detail)) => {
                        malformed += 1;
                        if malformed_lines.len() < 10 {
                            malformed_lines.push(index + 1);
                        }
                        if first_malformed_detail.is_none() {
                            first_malformed_detail = Some(detail);
                        }
                    }
                }
            }
            tracing::info!(
                parsed = qas.len(),
                malformed,
                generator_errors,
                "Parsed QA input"
            );
            if malformed > 0 || generator_errors > 0 {
                let mut message = format!(
                    "Grounding-gated ingestion rejects partial input: {malformed} malformed and {generator_errors} non-QA rows"
                );
                if malformed > 0 {
                    message.push_str(&format!(
                        "; malformed lines (first 10): {}",
                        malformed_lines
                            .iter()
                            .map(usize::to_string)
                            .collect::<Vec<String>>()
                            .join(", ")
                    ));
                    if let Some(detail) = first_malformed_detail {
                        message.push_str(&format!("; first malformed detail: {detail}"));
                    }
                }
                return Err(McpToolError::invalid_argument(message));
            }

            // Structural admission only: concise answers are not low-quality answers.
            let filtered: Vec<(usize, &ParsedQa)> = qas
                .iter()
                .filter(|(_, q)| {
                    !q.instruction.trim().is_empty()
                        && !q.output.trim().is_empty()
                        && !q.qa_type.trim().is_empty()
                        && !q.source.trim().is_empty()
                        && q.chunk_ref
                            .as_ref()
                            .is_some_and(|value| !value.trim().is_empty())
                })
                .map(|(line, qa)| (*line, qa))
                .collect();
            tracing::info!(
                "  Structural filter: {} (removed {})",
                filtered.len(),
                qas.len() - filtered.len()
            );
            if filtered.len() != qas.len() {
                return Err(McpToolError::invalid_argument(format!(
                    "Grounding-gated ingestion rejects {} structurally incomplete QA rows",
                    qas.len() - filtered.len()
                )));
            }
            // Grounding gate: authority is re-executed from the candidates and
            // canonical chunks before dedup, output, or DB access. The bundle
            // is a record, never a source of authority.
            let gate = crate::services::qa_grounding::verify_grounding_gate(
                &req.grounding_manifest,
                &filtered,
                &req.source_chunks_jsonl,
            )?;

            // Brave clause-xiii exclusion (task c1d65229): QA candidates whose
            // canonical source chunk is Brave-derived never reach QA-bound
            // storage — Brave's API ToS bans using Search Results to train AI
            // models. The provenance map also stamps every retained row and
            // stored h_mem with its originating provider, so an external
            // downstream reader cannot mistake a Brave-derived pair for
            // trainable material.
            let (source_chunks, _dropped_source_rows) =
                crate::helpers::read_jsonl_lenient::<hkask_types::corpus::TaggedChunk>(
                    &req.source_chunks_jsonl,
                    "source_chunks_jsonl",
                )?;
            let mut origin_by_ref: std::collections::HashMap<String, Option<String>> =
                std::collections::HashMap::new();
            let mut brave_refs: std::collections::HashSet<String> = std::collections::HashSet::new();
            for chunk in &source_chunks {
                if chunk
                    .origin_provider
                    .as_deref()
                    .is_some_and(hkask_types::corpus::is_brave_origin)
                {
                    brave_refs.insert(chunk.entity_ref.clone());
                }
                origin_by_ref.insert(chunk.entity_ref.clone(), chunk.origin_provider.clone());
            }
            let brave_excluded = filtered
                .iter()
                .filter(|(_, qa)| {
                    qa.chunk_ref
                        .as_ref()
                        .is_some_and(|chunk_ref| brave_refs.contains(chunk_ref))
                })
                .count();
            if brave_excluded > 0 {
                tracing::warn!(
                    brave_excluded,
                    "excluding Brave-derived QA candidates from ingestion (Brave API ToS clause xiii)"
                );
            }
            let filtered: Vec<(usize, &ParsedQa)> = filtered
                .into_iter()
                .filter(|(_, qa)| {
                    !qa.chunk_ref
                        .as_ref()
                        .is_some_and(|chunk_ref| brave_refs.contains(chunk_ref))
                })
                .collect();

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
            let mut deduped: Vec<(usize, &ParsedQa)> = Vec::new();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for pair in &filtered {
                if seen.insert(pair.1.instruction.to_lowercase()) {
                    deduped.push(*pair);
                }
            }

            let deduped_count = deduped.len();
            tracing::info!(
                "  Deduped: {} (removed {})",
                deduped_count,
                filtered.len() - deduped_count
            );

            let mut result = json!({
                "total_nonblank_rows": total_nonblank_rows,
                "generator_errors": generator_errors,
                "malformed": malformed,
                "parsed": qas.len(),
                "filtered": filtered.len(),
                "filter_drops": qas.len() - filtered.len(),
                "duplicates": filtered.len() - deduped_count,
                "deduped": deduped_count,
                "brave_excluded": brave_excluded,
                "retained": deduped_count,
                "qa_export_rows": 0,
                "record_role": "qa_export_metadata",
                "embeddings_generated": 0,
                "retrieval_note": "QA h_mems retain the research export and evidence; ingestion does not index generated questions or answers as source passages.",
                "stored": 0,
                "stored_h_mems": 0,
                "failed": 0,
                "storage_errors": [],
                "grounding_protocol": QA_GROUNDING_PROTOCOL,
                "grounding_manifest_sha256": gate.manifest_sha256,
                "grounding_rows": gate.rows,
                "dry_run": req.dry_run,
                "status": "dry_run",
            });
            if req.dry_run {
                return Ok(result);
            }

            // Keep evidence available for downstream audits in the QA export.
            let mut qa_export = String::new();
            for (line, qa) in &deduped {
                let (row_key, answer_provenance) = grounding_fields(qa, *line, &gate)?;
                let mut row = json!({
                    "instruction": qa.instruction, "input": "", "output": qa.output,
                    "qa_type": qa.qa_type, "type": qa.response_type.as_deref().unwrap_or(&qa.qa_type),
                    "source": qa.source, "chunk_ref": qa.chunk_ref,
                    "evidence_quotes": qa.evidence_quotes,
                    "prompt_id": qa.prompt_id, "provenance": qa.provenance,
                    "difficulty": qa.difficulty, "concepts": qa.concepts,
                    "grounding": {
                        "protocol": QA_GROUNDING_PROTOCOL,
                        "manifest_sha256": gate.manifest_sha256,
                        "row_key": row_key,
                        "answer_provenance": answer_provenance,
                    },
                });
                if let Some(origin) = qa
                    .chunk_ref
                    .as_ref()
                    .and_then(|chunk_ref| origin_by_ref.get(chunk_ref))
                    .cloned()
                    .flatten()
                {
                    row[hkask_types::corpus::ORIGIN_PROVIDER_FIELD] = json!(origin);
                }
                qa_export.push_str(
                    &serde_json::to_string(&row)
                        .map_err(|e| McpToolError::internal(format!("Serialize QA: {e}")))?,
                );
                qa_export.push('\n');
            }
            crate::helpers::write_contained(&req.output, &qa_export)?;
            tracing::info!("  Wrote: {} QAs to {}", deduped_count, req.output);
            result["qa_export_rows"] = json!(deduped_count);

            // Store h_mems as research-corpus records; this path does not
            // generate embeddings or feed a training executor. The output file
            // and individual DB inserts are not one atomic transaction.
            let store = crate::helpers::open_memory_store(
                &req.db_path,
                &crate::helpers::resolve_corpus_passphrase()?,
            )?;
            let webid = owner_webid(&req.owner);
            let mut stored = 0usize;
            let mut storage_errors = Vec::new();

            for (i, (line, qa)) in deduped.iter().enumerate() {
                // Compatibility-only storage identity: existing corpus QA rows
                // use this prefix and attribute. Keeping them readable does not
                // retain a model-training executor or handoff.
                let entity = format!("{LEGACY_QA_ENTITY_PREFIX}:{}:{}:{i}", req.dataset, qa.source);
                let (row_key, answer_provenance) = grounding_fields(qa, *line, &gate)?;
                let v = serde_json::json!({
                    "question": qa.instruction,
                    "answer": qa.output,
                    "bloom_level": qa.qa_type,
                    "qa_type": qa.qa_type,
                    "type": qa.response_type.as_deref().unwrap_or(&qa.qa_type),
                    "source": qa.source,
                    "dataset": req.dataset,
                    "difficulty": qa.difficulty,
                    "concepts": qa.concepts,
                    "chunk_ref": qa.chunk_ref,
                    "evidence_quotes": qa.evidence_quotes,
                    "prompt_id": qa.prompt_id,
                    "provenance": qa.provenance,
                    "grounding": {
                        "protocol": QA_GROUNDING_PROTOCOL,
                        "manifest_sha256": gate.manifest_sha256,
                        "row_key": row_key,
                        "answer_provenance": answer_provenance,
                    },
                });
                let mut v = v;
                if let Some(origin) = qa
                    .chunk_ref
                    .as_ref()
                    .and_then(|chunk_ref| origin_by_ref.get(chunk_ref))
                    .cloned()
                    .flatten()
                {
                    v[hkask_types::corpus::ORIGIN_PROVIDER_FIELD] = json!(origin);
                }
                let mut ontology = hkask_types::HMemOntology::state(
                    hkask_bridge_ontology::dc_bibo::DOCUMENT,
                    qa.concepts.clone(),
                    qa.source.clone(),
                );
                for term in &qa.concepts {
                    ontology = ontology.with_candidate_term(term.clone());
                }
                ontology.pko_procedure = Some("corpus_generate_qa_batch".to_string());
                ontology.pko_step = qa.chunk_ref.clone();
                let h_mem = hkask_storage::HMem::new(&entity, LEGACY_QA_ATTRIBUTE, v, webid)
                    .with_visibility(hkask_types::Visibility::Public)
                    .with_confidence(0.8)
                    .with_ontology(ontology);
                match store.store(h_mem) {
                    Ok(()) => stored += 1,
                    Err(error) => {
                        tracing::warn!(%entity, %error, "QA h_mem storage failed");
                        storage_errors.push(json!({"entity": entity, "error": error.to_string()}));
                    }
                }
            }
            let failed = storage_errors.len();
            tracing::info!(stored, failed, "QA h_mem storage finished");
            result["stored"] = json!(stored);
            result["stored_h_mems"] = json!(stored);
            result["failed"] = json!(failed);
            result["storage_errors"] = json!(storage_errors);
            result["output"] = json!(req.output);
            result["status"] = json!(if failed == 0 {
                "complete"
            } else {
                "partial_failure"
            });
            Ok(result)
        })
        .await
    }

    // ── Ground generated QA ──────────────────────────────────────────────

    #[tool(
        description = "Produce a hash-bound corpus-qa-grounding-v1 bundle (manifest.json plus grounding-rows.jsonl) for generated QA candidates. Deterministic and zero-inference: verifies every evidence quote byte-exactly against uniquely identified canonical tagged chunks classified under the current published-ontology protocol, records byte spans, and recomputes ontology resolutions for the preserved candidate terms through the published ladder. The bundle records mechanical facts only — no verified, authorized, or confidence fields exist in it, and rows with paraphrase answers are recorded honestly as model_inference. corpus_ingest_qa re-executes every check before admission; this tool authorizes nothing."
    )]
    pub async fn corpus_ground_generated_qa(
        &self,
        Parameters(req): Parameters<GroundQaRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_ground_generated_qa", async {
            crate::services::qa_grounding::build_grounding_bundle(&req)
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
    /// Classified tagged chunks JSONL. Failed, unverified or duplicate refs are rejected.
    pub tagged_jsonl: String,
    /// Output path for prompts JSONL (one JSON per line, consumed by generate_qa_batch).
    pub output: String,
    /// SQLCipher memory DB used only when context_k > 0.
    #[serde(default)]
    pub db_path: Option<String>,
    /// Entity-ref prefix for the KNN embedding lookup (default
    /// "corpus:researcher:"). All input references must be under this prefix.
    /// KNN reads complete-source stored passages, not just the input partition.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Source-scoped KNN neighbors. Default 0 keeps factual/conceptual pairs
    /// primary-only; positive values require db_path and passphrase.
    #[serde(default)]
    pub context_k: usize,
    /// Positive number of QA pairs requested in one model call per chunk.
    #[serde(default = "default_qa_pairs_per_chunk")]
    pub qa_pairs_per_chunk: usize,
    /// Bloom's taxonomy weight distribution (e.g. "1,1,1,1,1" = equal).
    #[serde(default = "default_type_distribution")]
    pub type_distribution: String,
    /// Maximum requested QA pairs (0 = all chunks × qa_pairs_per_chunk).
    #[serde(default)]
    pub max_pairs: usize,
}

fn default_qa_pairs_per_chunk() -> usize {
    2
}

fn default_type_distribution() -> String {
    "1,1,1,1,1".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IngestQaRequest {
    /// Path to generated QA candidates JSONL (from corpus_generate_qa_batch).
    pub generated_jsonl: String,
    /// Path to the manifest.json of a `corpus-qa-grounding-v1` bundle produced by
    /// `corpus_ground_generated_qa`. The gate re-executes every mechanical check
    /// from the candidates and canonical chunks; the artifact is never trusted
    /// as authority, and self-reported verification cannot open the gate.
    pub grounding_manifest: String,
    /// Canonical tagged-chunk JSONL used to re-execute every grounding check.
    /// Every chunk must be classified under `published-term-resolution-v2` with
    /// reconciling candidate terms.
    pub source_chunks_jsonl: String,
    /// Output path for QA export JSONL (instruction/input/output plus evidence metadata).
    pub output: String,
    /// Path to the SQLCipher memory DB for research QA h_mem storage.
    pub db_path: String,
    /// If true, validate and dedup without writing output or storage.
    #[serde(default)]
    pub dry_run: bool,
    /// Dataset name for QA h_mems. Stored rows retain the legacy
    /// `training_qa_pair` attribute for compatibility with existing corpora.
    #[serde(default = "default_dataset")]
    pub dataset: String,
    /// Owner persona for stored research QA h_mems.
    #[serde(default = "default_owner")]
    pub owner: String,
}

fn default_dataset() -> String {
    "capabilities-reasoner".to_string()
}

// Durable identifiers from the former training handoff. Renaming them would
// strand existing corpus QA records, so they remain compatibility-only storage
// schema; no model-training executor consumes them.
const LEGACY_QA_ENTITY_PREFIX: &str = "training:qa";
const LEGACY_QA_ATTRIBUTE: &str = "training_qa_pair";
