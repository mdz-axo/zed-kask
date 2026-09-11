//! Prepared QA prompts from complete-source stored passages, never from the
//! current input partition's accidental subset of neighboring chunks.

use std::collections::{HashMap, HashSet};

use hkask_mcp_server::server::McpToolError;
use hkask_types::corpus::{ClassificationOutcome, TaggedChunk, qa_prompt_id};
use serde_json::json;

use crate::helpers::map_memory_store_error;
use crate::services::qa_pipeline::{PreparedQaPrompt, QA_RESPONSE_CONTRACT};
use crate::tools::corpus::{
    QaType, parse_type_distribution, qa_type_instruction, qa_type_str, read_tagged_chunks,
};
use crate::{normalize_in_place, render_docproc_template};

pub(crate) struct BuildPromptsRequest {
    pub tagged_jsonl: String,
    pub output: String,
    pub db_path: String,
    pub passphrase: String,
    pub prefix: Option<String>,
    pub context_k: usize,
    pub prompts_per_chunk: usize,
    pub type_distribution: String,
    pub max_prompts: usize,
    pub ontology_bloom_overrides: Option<String>,
}

struct StoredPassage {
    source: String,
    text: String,
    vector: Vec<f32>,
    memories: Vec<hkask_storage::HMem>,
}

/// Canonical MemoryStore reads own provenance and passage text. No source DB,
/// copied embedding store, partition fallback, or inference is introduced.
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
            if chunk.classification != ClassificationOutcome::Classified {
                return Err(McpToolError::invalid_argument(format!(
                    "Chunk '{}' is not classified: {:?}",
                    chunk.entity_ref, chunk.classification
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
                || !chunk.salience.is_finite()
                || !chunk.entity_ref.starts_with(prefix)
            {
                return Err(McpToolError::invalid_argument(format!(
                    "Chunk '{}' needs nonblank source/text, finite salience and a reference under prefix '{prefix}'",
                    chunk.entity_ref
                )));
            }
        }
        if request.prompts_per_chunk == 0 {
            return Err(McpToolError::invalid_argument(
                "prompts_per_chunk must be positive",
            ));
        }
        let requested = chunks
            .len()
            .checked_mul(request.prompts_per_chunk)
            .ok_or_else(|| {
                McpToolError::invalid_argument("Requested prompt count overflows usize")
            })?;
        let limit = if request.max_prompts == 0 {
            requested
        } else {
            request.max_prompts.min(requested)
        };
        let default_rotation = parse_type_distribution(&request.type_distribution);
        let mut bloom_overrides: HashMap<&str, Vec<QaType>> = HashMap::new();
        if let Some(overrides) = request.ontology_bloom_overrides.as_deref() {
            for entry in overrides.split('|') {
                let (namespace, distribution) = entry.split_once(':').ok_or_else(|| {
                    McpToolError::invalid_argument(
                        "ontology_bloom_overrides must use namespace:distribution entries",
                    )
                })?;
                if namespace.is_empty()
                    || bloom_overrides
                        .insert(namespace, parse_type_distribution(distribution))
                        .is_some()
                {
                    return Err(McpToolError::invalid_argument(
                        "Empty or duplicate Bloom override namespace",
                    ));
                }
            }
        }
        let store = crate::helpers::open_memory_store(&request.db_path, &request.passphrase)?;
        let mut passages = HashMap::new();
        // context_k=0 is an explicit opt-out, not a fallback after failed reads.
        if request.context_k > 0 {
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
                            memories,
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
        let mut sorted: Vec<&TaggedChunk> = chunks.iter().collect();
        sorted.sort_by(|a, b| {
            b.salience
                .total_cmp(&a.salience)
                .then_with(|| a.entity_ref.cmp(&b.entity_ref))
        });
        let mut output = String::new();
        let mut written = 0;
        let mut context_links = 0;
        'chunks: for chunk in sorted {
            let (context, memories) = if request.context_k == 0 {
                (
                    Vec::new(),
                    store
                        .query_deduped_untouched(&chunk.entity_ref)
                        .map_err(|error| {
                            map_memory_store_error(error, "Cannot read QA knowledge graph")
                        })?,
                )
            } else {
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
                    let similarity: f32 = primary
                        .vector
                        .iter()
                        .zip(&candidate.vector)
                        .map(|(a, b)| a * b)
                        .sum();
                    scored.push((*reference, *candidate, similarity));
                }
                scored.sort_by(|a, b| b.2.total_cmp(&a.2).then_with(|| a.0.cmp(b.0)));
                scored.truncate(request.context_k);
                let context = scored.into_iter().map(|(reference, passage, similarity)| json!({
                    "chunk_ref":reference, "source":passage.source, "text":passage.text, "similarity":similarity
                })).collect::<Vec<_>>();
                (context, primary.memories.clone())
            };
            context_links += context.len();
            let context_text = serde_json::to_string(&context).map_err(|error| {
                McpToolError::internal(format!("Cannot serialize QA context: {error}"))
            })?;
            // Assertion values are context, never independently verified facts.
            let mut assertions: Vec<_> = memories
                .iter()
                .filter(|memory| {
                    !matches!(
                        memory.attribute.as_str(),
                        "text" | "method_signals" | "ontology_tags"
                    )
                })
                .map(|memory| format!("{}: {}", memory.attribute, memory.value))
                .collect();
            assertions.sort();
            let rotation = ["pko", "golem", "fibo", "sepio", "epistemic"]
                .into_iter()
                .find_map(|namespace| {
                    chunk
                        .ontology_tags
                        .contains_key(namespace)
                        .then(|| bloom_overrides.get(namespace))
                        .flatten()
                })
                .unwrap_or(&default_rotation);
            let mut type_ordinals: HashMap<&str, usize> = HashMap::new();
            for ordinal in 0..request.prompts_per_chunk {
                if written == limit {
                    break 'chunks;
                }
                let qa_type = *rotation
                    .get(ordinal % rotation.len())
                    .ok_or_else(|| McpToolError::invalid_argument("Empty QA type rotation"))?;
                let kind = qa_type_str(qa_type);
                let type_ordinal = type_ordinals.entry(kind).or_default();
                let prompt_id = qa_prompt_id(&chunk.source, &chunk.entity_ref, kind, *type_ordinal);
                *type_ordinal += 1;
                let mut tags: Vec<_> = chunk.ontology_tags.iter().collect();
                tags.sort_by(|a, b| a.0.cmp(b.0));
                let mut vars = HashMap::new();
                vars.insert("qa_instruction", qa_type_instruction(qa_type).to_string());
                vars.insert("dimensions", chunk.dimensions.join(", "));
                vars.insert("qa_type", kind.to_string());
                vars.insert("expertise", chunk.expertise_level.as_str().to_string());
                vars.insert("source", chunk.source.clone());
                vars.insert("dc_type", chunk.dc_type.clone());
                vars.insert("dc_subject", chunk.dc_subject.join(", "));
                vars.insert("consolidated_from", chunk.consolidated_from.join(", "));
                vars.insert(
                    "ontology_tags",
                    tags.into_iter()
                        .map(|(namespace, concepts)| {
                            format!("{namespace}: {}", concepts.join(", "))
                        })
                        .collect::<Vec<_>>()
                        .join(" | "),
                );
                vars.insert("context_passages", context_text.clone());
                vars.insert("concept_graph", chunk.concepts.join(", "));
                vars.insert("knowledge_graph", assertions.join("\n"));
                let system = render_docproc_template("build-prompts", &vars);
                if system.is_empty() {
                    return Err(McpToolError::failed_precondition(
                        "Required docproc/build-prompts template is unavailable",
                    ));
                }
                let primary =
                    json!({"chunk_ref":chunk.entity_ref,"source":chunk.source,"text":chunk.text});
                let prompt = PreparedQaPrompt {
                    prompt_id,
                    chunk_ref: chunk.entity_ref.clone(),
                    source: chunk.source.clone(),
                    concepts: chunk.concepts.clone(),
                    salience: f64::from(chunk.salience),
                    qa_type: kind.to_string(),
                    system: format!(
                        "{system}\n\n{QA_RESPONSE_CONTRACT}\nRequested bloom_level: {kind}."
                    ),
                    user: format!(
                        "Generate a {kind} QA pair from this primary passage (explicit source and chunk identities):\n{primary}"
                    ),
                };
                prompt.validate()?;
                output.push_str(&serde_json::to_string(&prompt).map_err(|error| {
                    McpToolError::internal(format!("Cannot serialize prepared QA: {error}"))
                })?);
                output.push('\n');
                written += 1;
            }
        }
        crate::helpers::write_contained(&request.output, &output)?;
        Ok(
            json!({"total_chunks":chunks.len(), "prompts_written":written, "output":request.output,
            "context_enabled":request.context_k > 0, "context_links":context_links,
            "context_scope":"complete_source", "stored_passages":passages.len()}),
        )
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
