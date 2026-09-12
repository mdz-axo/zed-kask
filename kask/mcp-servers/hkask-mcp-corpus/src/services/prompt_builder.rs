//! Compact prepared QA requests from current-protocol classified passages.

use std::collections::{HashMap, HashSet};

use hkask_bridge_ontology::term_resolution::TERM_RESOLUTION_PROTOCOL;
use hkask_mcp_server::server::McpToolError;
use hkask_types::corpus::qa_prompt_id;

use crate::helpers::map_memory_store_error;
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
/// from the stored protocol and variables by both transports identically.
pub(crate) struct PromptBuilderService;

impl PromptBuilderService {
    pub fn new() -> Self {
        Self
    }

    pub async fn build_prompts(
        &self,
        request: BuildPromptsRequest,
    ) -> Result<serde_json::Value, McpToolError> {
        let chunks = read_tagged_chunks(&request.tagged_jsonl)?;
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
        let rotation = parse_type_distribution(&request.type_distribution);

        let mut passages = HashMap::new();
        if request.context_k > 0 {
            let db_path = request.db_path.as_deref().ok_or_else(|| {
                McpToolError::invalid_argument("db_path is required when context_k > 0")
            })?;
            let passphrase = request.passphrase.as_deref().ok_or_else(|| {
                McpToolError::invalid_argument("passphrase is required when context_k > 0")
            })?;
            let store = crate::helpers::open_memory_store(db_path, passphrase)?;
            let rows = store.all_embeddings_with_text().map_err(|error| {
                map_memory_store_error(error, "Cannot load complete-source QA context")
            })?;
            for (reference, mut vector, text) in rows.into_iter().filter(|(reference, _, _)| {
                reference.starts_with(prefix)
                    && hkask_types::corpus::is_corpus_passage_ref(reference)
            }) {
                let text = text.filter(|text| !text.trim().is_empty()).ok_or_else(|| {
                    McpToolError::failed_precondition(format!(
                        "QA context '{reference}' has no stored passage_text"
                    ))
                })?;
                if vector.is_empty()
                    || vector.iter().any(|value| !value.is_finite())
                    || !vector.iter().any(|value| *value != 0.0)
                {
                    return Err(McpToolError::failed_precondition(format!(
                        "QA context '{reference}' has an invalid embedding"
                    )));
                }
                normalize_in_place(&mut vector);
                let memories = store.query_deduped_untouched(&reference).map_err(|error| {
                    map_memory_store_error(error, "Cannot read QA source provenance")
                })?;
                let sources: HashSet<_> = memories
                    .iter()
                    .filter(|memory| memory.attribute == "text")
                    .filter_map(|memory| memory.ontology.as_ref())
                    .map(|ontology| ontology.dc_source.as_str())
                    .filter(|source| !source.trim().is_empty() && *source != reference)
                    .collect();
                if sources.len() != 1 {
                    return Err(McpToolError::failed_precondition(format!(
                        "QA context '{reference}' needs one original source in text h_mem ontology.dc_source; found {}",
                        sources.len()
                    )));
                }
                let source = sources
                    .into_iter()
                    .next()
                    .ok_or_else(|| McpToolError::internal("Source count invariant failed"))?
                    .to_string();
                if passages
                    .insert(
                        reference.clone(),
                        StoredPassage {
                            source,
                            text,
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

        let mut by_source: HashMap<&str, Vec<(&str, &StoredPassage)>> = HashMap::new();
        for (reference, passage) in &passages {
            by_source
                .entry(&passage.source)
                .or_default()
                .push((reference, passage));
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
            if request.context_k > 0 {
                let primary = passages.get(&chunk.entity_ref).ok_or_else(|| {
                    McpToolError::failed_precondition(format!(
                        "No stored passage for primary '{}'",
                        chunk.entity_ref
                    ))
                })?;
                if primary.source != chunk.source || primary.text != chunk.text {
                    return Err(McpToolError::failed_precondition(format!(
                        "Primary '{}' disagrees with stored source or passage_text",
                        chunk.entity_ref
                    )));
                }
                let mut scored = Vec::new();
                for (reference, candidate) in
                    by_source.get(chunk.source.as_str()).into_iter().flatten()
                {
                    if *reference == chunk.entity_ref {
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
                    scored.push((*reference, *candidate, similarity));
                }
                scored.sort_by(|left, right| {
                    right.2.total_cmp(&left.2).then_with(|| left.0.cmp(right.0))
                });
                scored.truncate(request.context_k);
                for (index, (reference, passage, _)) in scored.into_iter().enumerate() {
                    prepared_passages.push(PreparedQaPassage {
                        local_id: format!("p{}", index + 1),
                        chunk_ref: reference.to_string(),
                        source: passage.source.clone(),
                        text: passage.text.clone(),
                    });
                }
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
            "stored_passages": passages.len(),
        }))
    }
}

#[cfg(test)]
#[path = "prompt_builder_tests.rs"]
mod tests;

impl Default for PromptBuilderService {
    fn default() -> Self {
        Self::new()
    }
}
