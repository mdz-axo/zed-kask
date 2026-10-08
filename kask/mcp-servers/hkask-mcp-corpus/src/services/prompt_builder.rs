//! Compact prepared QA requests from current-protocol classified passages.

use std::collections::{HashMap, HashSet};

use hkask_bridge_ontology::term_resolution::TERM_RESOLUTION_PROTOCOL;
use hkask_mcp_server::server::McpToolError;
use hkask_storage::{DatabaseDriver, SqliteDriver};
use hkask_types::corpus::qa_prompt_id;

use crate::normalize_in_place;
use crate::services::qa_pipeline::{PREPARED_QA_PROTOCOL, PreparedQaPassage, PreparedQaPrompt};
use crate::tools::corpus::read_tagged_chunks;
use crate::tools::corpus::{QaType, parse_type_distribution};

pub(crate) struct BuildPromptsRequest {
    pub tagged_jsonl: String,
    pub output: String,
    pub db_path: Option<String>,
    pub passphrase: Option<String>,
    pub prefix: Option<String>,
    pub context_k: usize,
    pub qa_pairs_per_chunk: usize,
    pub type_distribution: String,
    pub max_pairs: usize,
}

struct StoredPassage {
    source: String,
    text: String,
    vector: Vec<f32>,
}

/// One compact prepared-request builder. Provider messages are rendered later
/// from the stored protocol and variables by the generation path.
pub(crate) struct PromptBuilderService;

impl PromptBuilderService {
    pub fn new() -> Self {
        Self
    }

    pub async fn build_prompts(
        &self,
        request: BuildPromptsRequest,
    ) -> Result<serde_json::Value, McpToolError> {
        let (chunks, dropped_malformed) = read_tagged_chunks(&request.tagged_jsonl)?;
        if dropped_malformed > 0 {
            tracing::warn!(
                dropped = dropped_malformed,
                "tagged_jsonl contained malformed lines; building prompts from the parseable subset"
            );
        }
        if chunks.is_empty() {
            return Err(McpToolError::invalid_argument("tagged_jsonl is empty"));
        }
        let prefix = request.prefix.as_deref().unwrap_or("corpus:researcher:");
        let mut references = HashSet::new();
        for chunk in &chunks {
            if !chunk.has_current_canonical_terms() {
                return Err(McpToolError::invalid_argument(format!(
                    "Chunk '{}' is not reconciled under ontology protocol '{}': {:?}",
                    chunk.entity_ref, TERM_RESOLUTION_PROTOCOL, chunk.classification
                )));
            }
            if !references.insert(&chunk.entity_ref) {
                return Err(McpToolError::invalid_argument(format!(
                    "Duplicate chunk_ref '{}'",
                    chunk.entity_ref
                )));
            }
            if chunk.entity_ref.trim().is_empty()
                || chunk.source.trim().is_empty()
                || chunk.text.trim().is_empty()
                || !chunk.entity_ref.starts_with(prefix)
            {
                return Err(McpToolError::invalid_argument(format!(
                    "Chunk '{}' needs nonblank source/text and a reference under prefix '{prefix}'",
                    chunk.entity_ref
                )));
            }
        }
        if request.qa_pairs_per_chunk == 0 {
            return Err(McpToolError::invalid_argument(
                "qa_pairs_per_chunk must be positive",
            ));
        }
        let requested_pairs = chunks
            .len()
            .checked_mul(request.qa_pairs_per_chunk)
            .ok_or_else(|| {
                McpToolError::invalid_argument("Requested QA pair count overflows usize")
            })?;
        let pair_limit = if request.max_pairs == 0 {
            requested_pairs
        } else {
            request.max_pairs.min(requested_pairs)
        };
        let rotation = parse_type_distribution(&request.type_distribution)
            .map_err(|error| McpToolError::invalid_argument(error.to_string()))?;

        // Source-scoped KNN context, computed source-by-source so only one
        // source's passage pool is resident at a time. The former preload
        // held every stored passage's vector and text — ~1.3 GB at
        // zk-ref-open scale (279,730 passages) — to score each chunk against
        // its same-source pool (measured class, 2026-10-06). Selection
        // semantics are unchanged: per chunk, the top context_k same-source
        // passages by cosine over the complete same-source pool, with the
        // same validation failures on the passages a pool actually loads.
        // Pools load in a pre-pass so prompt order and the max_pairs
        // truncation follow the input chunk order.
        let mut context_selections: HashMap<&str, Vec<PreparedQaPassage>> = HashMap::new();
        let mut stored_passages_loaded = 0usize;
        if request.context_k > 0 {
            let db_path = request.db_path.as_deref().ok_or_else(|| {
                McpToolError::invalid_argument("db_path is required when context_k > 0")
            })?;
            let passphrase = request.passphrase.as_deref().ok_or_else(|| {
                McpToolError::invalid_argument("passphrase is required when context_k > 0")
            })?;
            let database =
                hkask_storage::Database::open_read_only(db_path, passphrase).map_err(|error| {
                    McpToolError::failed_precondition(format!(
                        "Cannot open the corpus database '{db_path}' read-only: {error}"
                    ))
                })?;
            let pool = database.sqlite_pool().map_err(|error| {
                McpToolError::internal(format!("Corpus database pool failed: {error}"))
            })?;
            let driver: std::sync::Arc<dyn DatabaseDriver> =
                std::sync::Arc::new(SqliteDriver::new_labeled(pool, db_path));

            // ref → unique source over the prefix's corpus-passage text
            // h_mems — the same provenance the preload read per ref. A ref
            // with conflicting stored sources fails exactly as before.
            let rows = driver
                .query(
                    "SELECT entity, json_extract(ontology, '$.dc_source') \
                     FROM hmems WHERE attribute = 'text' AND ontology IS NOT NULL",
                    &[],
                )
                .map_err(|error| {
                    McpToolError::internal(format!("QA context provenance scan failed: {error}"))
                })?;
            let mut ref_source: HashMap<String, String> = HashMap::new();
            for row in &rows {
                let (Ok(entity), Ok(source)) = (row.get_str(0), row.get_str(1)) else {
                    continue;
                };
                if !entity.starts_with(prefix)
                    || !hkask_types::corpus::is_corpus_passage_ref(entity)
                    || source.trim().is_empty()
                    || source == entity
                {
                    continue;
                }
                match ref_source.entry(entity.to_string()) {
                    std::collections::hash_map::Entry::Occupied(existing)
                        if existing.get() != source =>
                    {
                        return Err(McpToolError::failed_precondition(format!(
                            "QA context '{entity}' needs one original source in text h_mem ontology.dc_source; found conflicting stored sources"
                        )));
                    }
                    std::collections::hash_map::Entry::Occupied(_) => {}
                    std::collections::hash_map::Entry::Vacant(vacant) => {
                        vacant.insert(source.to_string());
                    }
                }
            }

            let mut chunks_by_source: HashMap<&str, Vec<usize>> = HashMap::new();
            for (index, chunk) in chunks.iter().enumerate() {
                chunks_by_source
                    .entry(chunk.source.as_str())
                    .or_default()
                    .push(index);
            }
            for (source, indices) in chunks_by_source {
                let refs: Vec<&str> = ref_source
                    .iter()
                    .filter(|(_, stored)| stored.as_str() == source)
                    .map(|(reference, _)| reference.as_str())
                    .collect();
                let pool = load_source_pool(driver.as_ref(), &refs, source)?;
                stored_passages_loaded += pool.len();
                for &index in &indices {
                    let chunk = &chunks[index];
                    let primary = pool.get(chunk.entity_ref.as_str()).ok_or_else(|| {
                        McpToolError::failed_precondition(format!(
                            "No stored passage for primary '{}'",
                            chunk.entity_ref
                        ))
                    })?;
                    if primary.text != chunk.text {
                        return Err(McpToolError::failed_precondition(format!(
                            "Primary '{}' disagrees with stored passage_text",
                            chunk.entity_ref
                        )));
                    }
                    let mut scored = Vec::new();
                    for (reference, candidate) in &pool {
                        if reference.as_str() == chunk.entity_ref.as_str() {
                            continue;
                        }
                        if candidate.vector.len() != primary.vector.len() {
                            return Err(McpToolError::failed_precondition(format!(
                                "Embedding dimensions differ for '{reference}'"
                            )));
                        }
                        let similarity = primary
                            .vector
                            .iter()
                            .zip(&candidate.vector)
                            .map(|(left, right)| left * right)
                            .sum::<f32>();
                        scored.push((reference, candidate, similarity));
                    }
                    scored.sort_by(|left, right| {
                        right.2.total_cmp(&left.2).then_with(|| left.0.cmp(right.0))
                    });
                    scored.truncate(request.context_k);
                    let selected = scored
                        .into_iter()
                        .enumerate()
                        .map(|(ordinal, (reference, passage, _))| PreparedQaPassage {
                            local_id: format!("p{}", ordinal + 1),
                            chunk_ref: reference.to_string(),
                            source: passage.source.clone(),
                            text: passage.text.clone(),
                        })
                        .collect::<Vec<_>>();
                    context_selections.insert(chunk.entity_ref.as_str(), selected);
                }
            }
        }

        let mut output = String::new();
        let mut prompts_written = 0usize;
        let mut pairs_requested = 0usize;
        let mut context_links = 0usize;
        for chunk in &chunks {
            if pairs_requested == pair_limit {
                break;
            }
            let pair_count = request.qa_pairs_per_chunk.min(pair_limit - pairs_requested);
            let qa_types = (0..pair_count)
                .map(|ordinal| rotation[ordinal % rotation.len()])
                .collect::<Vec<QaType>>();
            let type_key = qa_types
                .iter()
                .map(QaType::as_str)
                .collect::<Vec<_>>()
                .join("+");
            let prompt_id = qa_prompt_id(&chunk.source, &chunk.entity_ref, &type_key, 0);

            let mut prepared_passages = vec![PreparedQaPassage {
                local_id: "p0".to_string(),
                chunk_ref: chunk.entity_ref.clone(),
                source: chunk.source.clone(),
                text: chunk.text.clone(),
            }];
            if let Some(selected) = context_selections.get(chunk.entity_ref.as_str()) {
                prepared_passages.extend(selected.iter().map(|passage| PreparedQaPassage {
                    local_id: passage.local_id.clone(),
                    chunk_ref: passage.chunk_ref.clone(),
                    source: passage.source.clone(),
                    text: passage.text.clone(),
                }));
            }
            context_links += prepared_passages.len() - 1;

            let prompt = PreparedQaPrompt {
                prompt_id,
                protocol: PREPARED_QA_PROTOCOL.to_string(),
                passages: prepared_passages,
                candidate_terms: chunk.candidate_terms.clone(),
                qa_types,
            };
            prompt.validate()?;
            output.push_str(&serde_json::to_string(&prompt).map_err(|error| {
                McpToolError::internal(format!("Cannot serialize prepared QA: {error}"))
            })?);
            output.push('\n');
            prompts_written += 1;
            pairs_requested += pair_count;
        }
        crate::helpers::write_contained(&request.output, &output)?;
        Ok(serde_json::json!({
            "total_chunks": chunks.len(),
            "prompts_written": prompts_written,
            "pairs_requested": pairs_requested,
            "output": request.output,
            "context_enabled": request.context_k > 0,
            "context_links": context_links,
            "context_scope": if request.context_k > 0 { "complete_source" } else { "primary_only" },
            "stored_passages": stored_passages_loaded,
        }))
    }
}

/// Load one source's passage pool from the corpus DB: the named refs'
/// vectors (decoded from the stored little-endian f32 blobs) and passage
/// texts, validated and normalized exactly as the former whole-DB preload
/// validated them — empty text or an invalid vector fails the call.
/// Keyed by reference; a duplicate stored row for one ref fails too.
fn load_source_pool(
    driver: &dyn DatabaseDriver,
    refs: &[&str],
    source: &str,
) -> Result<HashMap<String, StoredPassage>, McpToolError> {
    let mut pool: HashMap<String, StoredPassage> = HashMap::new();
    for reference_chunk in refs.chunks(500) {
        let placeholders = vec!["?"; reference_chunk.len()].join(",");
        let params: Vec<hkask_storage::database::value::DbValue> = reference_chunk
            .iter()
            .map(|reference| {
                hkask_storage::database::value::DbValue::Text((*reference).to_string())
            })
            .collect();
        let rows = driver
            .query(
                &format!(
                    "SELECT entity_ref, vector, passage_text FROM embeddings \
                     WHERE entity_ref IN ({placeholders})"
                ),
                &params,
            )
            .map_err(|error| {
                McpToolError::internal(format!("QA context pool query failed: {error}"))
            })?;
        for row in &rows {
            let reference = row.get_str(0).map_err(|error| {
                McpToolError::internal(format!("QA context pool reference read failed: {error}"))
            })?;
            let blob = row
                .get(1)
                .map_err(|error| {
                    McpToolError::internal(format!("QA context pool vector read failed: {error}"))
                })?
                .as_blob()
                .map_err(|error| {
                    McpToolError::internal(format!(
                        "QA context '{reference}' has a non-blob stored vector: {error}"
                    ))
                })?;
            let text = row.get_str(2).map_err(|_| {
                McpToolError::failed_precondition(format!(
                    "QA context '{reference}' has no stored passage_text"
                ))
            })?;
            if text.trim().is_empty() {
                return Err(McpToolError::failed_precondition(format!(
                    "QA context '{reference}' has no stored passage_text"
                )));
            }
            if blob.len() % 4 != 0 {
                return Err(McpToolError::failed_precondition(format!(
                    "QA context '{reference}' has an invalid embedding"
                )));
            }
            let mut vector: Vec<f32> = blob
                .chunks_exact(4)
                .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                .collect();
            if vector.is_empty()
                || vector.iter().any(|value| !value.is_finite())
                || !vector.iter().any(|value| *value != 0.0)
            {
                return Err(McpToolError::failed_precondition(format!(
                    "QA context '{reference}' has an invalid embedding"
                )));
            }
            normalize_in_place(&mut vector);
            if pool
                .insert(
                    reference.to_string(),
                    StoredPassage {
                        source: source.to_string(),
                        text: text.to_string(),
                        vector,
                    },
                )
                .is_some()
            {
                return Err(McpToolError::failed_precondition(format!(
                    "Duplicate stored passage reference '{reference}'"
                )));
            }
        }
    }
    Ok(pool)
}

#[cfg(test)]
#[path = "prompt_builder_tests.rs"]
mod tests;

impl Default for PromptBuilderService {
    fn default() -> Self {
        Self::new()
    }
}
