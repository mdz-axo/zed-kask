//! Ontology tagging tools — multi-dimensional chunk annotation.
//!
//! `corpus_tag_chunks`: Tags each chunk with 5W1H interrogatory dimensions,
//! Dublin Core metadata, PKO process concepts, FIBO/GOLEM domain concepts,
//! and expertise level. Uses LLM-based extraction via a Jinja2 template.
//! Every chunk gets at least one 5W1H dimension — no zero-tag chunks.

use crate::batch::{
    ADAPTIVE_CONCURRENCY_FLOOR, AdaptiveLimiter, BatchOutcome, MAX_RETRIES, retry_with_backoff,
};
use crate::{
    Arc, CorpusServer, LLMParameters, McpToolError, Parameters, execute_tool,
    extract_json_from_response, json, normalize_concept, read_jsonl_stream,
    render_docproc_template, tool, tool_router,
};
use hkask_inference::model_constants::classifier_model;
use hkask_types::corpus::{ClassificationOutcome, TaggedChunk};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Maximum length of a single concept string after normalization.
/// Guards against LLM-produced or injected oversized concept strings that
/// would bloat embedding annotation prefixes and QA system prompts.
const MAX_CONCEPT_LEN: usize = 80;

/// Maximum number of concepts per ontology namespace. Guards against
/// LLM-produced concept spam that would dominate the salience graph.
const MAX_CONCEPTS_PER_NS: usize = 30;

/// Minimal chunk for tagging (from chunks.jsonl).
#[derive(Debug, Clone, Deserialize)]
struct InputChunk {
    entity_ref: String,
    source: String,
    text: String,
    #[serde(default)]
    word_count: usize,
}

/// Ontology tags extracted by the LLM from a passage.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct OntologyTags {
    /// 5W1H interrogatory dimensions (at least one required).
    dimensions: Vec<String>,
    /// Dublin Core BIBO type (e.g., "bibo:Book").
    dc_type: String,
    /// Dublin Core subject keywords.
    dc_subject: Vec<String>,
    /// Flexible ontology tags keyed by namespace (e.g., "fibo", "golem", "pko", "other").
    ontology_tags: std::collections::HashMap<String, Vec<String>>,
    /// Expertise level — deserialized via `ExpertiseLevel`'s custom serde,
    /// which maps invalid strings to `Analyst`.
    expertise_level: hkask_types::corpus::ExpertiseLevel,
}

/// Both array entries and singleton responses use this same required identity.
#[derive(Deserialize)]
struct TagResponse {
    chunk_ref: String,
    #[serde(flatten)]
    tags: OntologyTags,
}

/// Accept a response only when it covers exactly this batch's identity set.
/// Validate before publishing anything: a malformed batch cannot spill into its neighbor.
fn correlate_tags(
    text: &str,
    chunks: &[InputChunk],
) -> Result<HashMap<String, OntologyTags>, String> {
    let cleaned = extract_json_from_response(text);
    let value: serde_json::Value =
        serde_json::from_str(&cleaned).map_err(|error| format!("invalid tagging JSON: {error}"))?;
    let entries = match value {
        serde_json::Value::Array(entries) => entries,
        serde_json::Value::Object(_) if chunks.len() == 1 => vec![value],
        _ => {
            return Err("expected tagging JSON array (singleton object only for one input)".into());
        }
    };
    let expected: HashSet<&str> = chunks
        .iter()
        .map(|chunk| chunk.entity_ref.as_str())
        .collect();
    let mut correlated = HashMap::new();
    for entry in entries {
        let response: TagResponse = serde_json::from_value(entry)
            .map_err(|error| format!("invalid tagging JSON entry: {error}"))?;
        if !expected.contains(response.chunk_ref.as_str()) {
            return Err(format!("unknown chunk_ref: {}", response.chunk_ref));
        }
        if correlated.contains_key(&response.chunk_ref) {
            return Err(format!("duplicate chunk_ref: {}", response.chunk_ref));
        }
        correlated.insert(response.chunk_ref, validate_ontology_tags(response.tags));
    }
    let omitted: Vec<&str> = chunks
        .iter()
        .map(|chunk| chunk.entity_ref.as_str())
        .filter(|id| !correlated.contains_key(*id))
        .collect();
    if !omitted.is_empty() {
        return Err(format!("omitted chunk_ref(s): {}", omitted.join(", ")));
    }
    Ok(correlated)
}

fn fallback_tags() -> OntologyTags {
    OntologyTags {
        dimensions: vec!["what".into()],
        dc_type: hkask_bridge_ontology::dc_bibo::DOCUMENT.into(),
        ..Default::default()
    }
}

/// A byte budget, rounded down to a UTF-8 character boundary.
fn utf8_prefix(text: &str, max_bytes: usize) -> &str {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn read_input_chunks(path: &str) -> Result<Vec<InputChunk>, McpToolError> {
    // Use the streaming reader to avoid the 32 MiB read cap on large
    // chunks.jsonl files (the pipeline manifest notes ~71.8 MB).
    let mut chunks = read_jsonl_stream::<InputChunk>(path, "chunks_jsonl")?;
    if chunks.is_empty() {
        return Err(McpToolError::invalid_argument("chunks_jsonl is empty"));
    }
    let mut refs = HashSet::new();
    for chunk in &chunks {
        if chunk.entity_ref.trim().is_empty() {
            return Err(McpToolError::invalid_argument(
                "entity_ref must be nonblank",
            ));
        }
        if !refs.insert(&chunk.entity_ref) {
            return Err(McpToolError::invalid_argument(format!(
                "duplicate entity_ref: {}",
                chunk.entity_ref
            )));
        }
    }
    // Sanitize control characters from PDF extraction. pdftotext maps
    // mathematical symbols to raw C0 control bytes when PDFs use custom
    // font encodings. These bytes cause the tagger LLM to see garbage and
    // return "empty passage" fallback tags. This handles pre-existing chunk
    // files that were created before sanitize_text was added to chunk_text.
    for chunk in &mut chunks {
        chunk.text = hkask_memory::text_chunking::sanitize_text(&chunk.text);
    }
    Ok(chunks)
}

// Compute graph-centrality salience via the memory service.
//
// Delegates to hkask_memory::salience::compute_salience_batch — the two-hop
// connectedness × (1 − redundancy) graph-centrality core. Only ontology
// concepts feed the graph; 5W1H dimensions are excluded (only six values with
// "what" in most chunks → a near-complete clique whose redundancy the (1−r)
// penalty suppresses, drowning the real shared-concept signal). Dimensions stay
// on the TaggedChunk as metadata for downstream use.
//
// Method signals are stored separately as numeric ontology metadata, not
// mixed into this concept graph as if measurements were shared concepts.
fn compute_salience(tagged: &[TaggedChunk]) -> Vec<f32> {
    let all_tags: Vec<hkask_memory::salience::EntityTags> = tagged
        .iter()
        .map(|c| hkask_memory::salience::EntityTags {
            concepts: c.concepts.clone(),
            ..Default::default()
        })
        .collect();
    hkask_memory::salience::compute_salience_batch(&all_tags)
}

/// Validate and normalize LLM-extracted ontology tags before they enter the
/// corpus. This is the security-critical boundary between untrusted LLM output
/// and the trusted `TaggedChunk` record.
///
/// Applies the following invariants:
/// - `dimensions`: filtered to the 5W1H allowlist; defaults to `["what"]` if empty.
/// - `expertise_level`: must be one of `practitioner` | `analyst` | `researcher`;
///   defaults to `analyst`.
/// - `dc_subject`: each entry normalized via `normalize_concept`, deduped, length-capped.
/// - `ontology_tags`: each namespace key lowercased + trimmed; each concept
///   normalized, deduped per-namespace, length-capped, count-capped per namespace.
///
/// This function is the single point where LLM-produced strings become trusted
/// corpus tags. Downstream consumers (salience graph, embedding annotation,
/// QA prompt injection) rely on this normalization being applied uniformly.
fn validate_ontology_tags(mut tags: OntologyTags) -> OntologyTags {
    // Dimensions: allowlist filter, default to ["what"] if empty.
    let valid_dims: Vec<String> = tags
        .dimensions
        .iter()
        .filter(|d| {
            matches!(
                d.as_str(),
                "who" | "what" | "when" | "where" | "why" | "how"
            )
        })
        .cloned()
        .collect();
    tags.dimensions = if valid_dims.is_empty() {
        vec!["what".to_string()]
    } else {
        valid_dims
    };

    // Expertise level: the custom serde deserializer on ExpertiseLevel
    // already maps invalid strings to Analyst. No runtime validation needed
    // here — the type system enforces the invariant.

    // dc_subject: normalize + dedup + length cap.
    tags.dc_subject = normalize_and_cap_concept_list(&tags.dc_subject);

    // ontology_tags: normalize namespace keys, normalize + cap concept lists.
    let mut cleaned_tags: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for (ns, concepts) in tags.ontology_tags {
        let norm_ns = normalize_concept(&ns);
        if norm_ns.is_empty() {
            continue;
        }
        let cleaned_concepts = normalize_and_cap_concept_list(&concepts);
        if !cleaned_concepts.is_empty() {
            cleaned_tags.insert(norm_ns, cleaned_concepts);
        }
    }
    tags.ontology_tags = cleaned_tags;

    tags
}

/// Normalize a list of concept strings: lowercase + trim + collapse whitespace,
/// dedup preserving first-seen order, drop empties, cap each string at
/// `MAX_CONCEPT_LEN`, cap the list at `MAX_CONCEPTS_PER_NS`.
fn normalize_and_cap_concept_list(raw: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for c in raw {
        let mut norm = normalize_concept(c);
        if norm.len() > MAX_CONCEPT_LEN {
            // Truncate at a word boundary if possible, else hard truncate.
            let prefix = utf8_prefix(&norm, MAX_CONCEPT_LEN);
            let end = prefix.rfind(' ').unwrap_or(prefix.len());
            norm.truncate(end);
        }
        if !norm.is_empty() && seen.insert(norm.clone()) && out.len() < MAX_CONCEPTS_PER_NS {
            out.push(norm);
        }
    }
    out
}

#[tool_router(router = tagging_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Tag chunks with multi-dimensional ontology annotations: 5W1H interrogatory dimensions, Dublin Core metadata, PKO process concepts, FIBO/GOLEM domain concepts, and expertise level. Uses LLM-based extraction via Jinja2 template. Computes graph-centrality salience. Every chunk gets at least one 5W1H dimension — no zero-salience chunks."
    )]
    pub async fn corpus_tag_chunks(
        &self,
        Parameters(req): Parameters<TagChunksRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_tag_chunks", async {
            let chunks = read_input_chunks(&req.chunks_jsonl)?;
            let total = chunks.len();
            tracing::info!("  Tagging {} chunks with ontology dimensions...", total);

            if req.dry_run {
                return Ok(json!({
                    "total_chunks": total,
                    "dry_run": true,
                    "note": "Would tag each chunk with 5W1H + Dublin Core + PKO + FIBO + GOLEM + expertise level"
                }));
            }

            let limiter = AdaptiveLimiter::new(req.concurrency, ADAPTIVE_CONCURRENCY_FLOOR);
            let router = Arc::clone(&self.inference_router);
            // Fail-visible: no configured classifier model is a typed
            // error naming the setting — never a hidden constant.
            let model_override = classifier_model().ok_or_else(|| {
                McpToolError::permission_denied(
                    "no classifier model configured — set \\
                     kask.models.classifier_model (injected as \\
                     HKASK_CLASSIFIER_MODEL); kask never falls back to a \\
                     hidden code constant",
                )
            })?;
            let batch_size = req.tag_batch_size.max(1);

            let start_time = std::time::Instant::now();

            // Group chunks into batches of `batch_size` for batched LLM calls.
            // Each batch sends N chunks in a single prompt and expects a JSON
            // array of N tag objects back. This amortizes the ~2K-token system
            // prompt across multiple chunks, cutting API calls by ~10x.
            let batches: Vec<(usize, Vec<InputChunk>)> = chunks
                .chunks(batch_size)
                .map(|batch| batch.to_vec())
                .enumerate()
                .map(|(batch_idx, batch_chunks)| {
                    // Global start index for this batch
                    let start = batch_idx * batch_size;
                    (start, batch_chunks)
                })
                .collect();
            let num_batches = batches.len();
            tracing::info!(
                total_chunks = total,
                batches = num_batches,
                batch_size,
                concurrency = req.concurrency,
                "Batched tagging: {} batches of up to {} chunks",
                num_batches,
                batch_size
            );

            let mut handles = Vec::with_capacity(num_batches);

            for (batch_idx, (start_idx, batch_chunks)) in batches.into_iter().enumerate() {
                let router = Arc::clone(&router);
                let limiter = limiter.clone();

                let model_override = model_override.clone();
                let batch_len = batch_chunks.len();

                let handle = tokio::spawn(async move {
                    let slot = limiter.acquire().await;

                    // Render the batch tagging prompt from the Jinja2 template.
                    // The template handles the system prompt, passage formatting,
                    // and output contract — keeping the prompt versioned and
                    // maintainable rather than inlined as a string literal.
                    //
                    // Pre-render the passages block as a plain string because
                    // render_docproc_template accepts HashMap<&str, String> —
                    // passing a JSON-serialized array would make the template's
                    // {% for %} loop iterate over characters, not elements.
                    let passages_block: String = batch_chunks
                        .iter()
                        .enumerate()
                        .map(|(i, chunk)| {
                            format!(
                                "--- Passage {} (chunk_ref: {}, source: {}) ---\n{}\n",
                                i + 1,
                                chunk.entity_ref,
                                chunk.source,
                                chunk.text
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let mut template_vars: std::collections::HashMap<&str, String> =
                        std::collections::HashMap::new();
                    template_vars.insert("passages_block", passages_block);
                    template_vars.insert("batch_count", batch_len.to_string());
                    let prompt = render_docproc_template("tag-chunks-batch", &template_vars);
                    if prompt.is_empty() {
                        return Err("tag-chunks-batch template missing or failed to render".to_string());
                    }

                    let params = LLMParameters {
                        temperature: 0.1,
                        top_p: 0.95,
                        frequency_penalty: 0.0,
                        presence_penalty: 0.0,
                        top_k: 0,
                        min_p: 0.0,
                        typical_p: 0.0,
                        thinking_allowed: false,
                        ..Default::default()
                    };

                    let response = match retry_with_backoff(
                        MAX_RETRIES,
                        "hkask.mcp.docproc.tag_chunks",
                        &format!("batch {batch_idx} of {batch_len}"),
                        || router.generate_with_model(&prompt, &params, Some(&model_override), None),
                    )
                    .await
                    {
                        Ok(resp) => {
                            slot.report_success();
                            resp
                        }
                        Err(e) => {
                            slot.report_failure();
                            // An inference failure must not silently read as
                            // "chunks tagged with fallback" — warn so the
                            // operator can distinguish the two.
                            tracing::warn!(
                                target: "hkask.mcp.docproc.tag_chunks",
                                batch = batch_idx,
                                chunks = batch_len,
                                error = %e,
                                "LLM call failed after retries — chunks will get fallback tags"
                            );
                            return Err(format!("inference failed after retries: {e}"));
                        }
                    };

                    let result = correlate_tags(&response.text, &batch_chunks);
                    if let Err(error) = &result {
                        tracing::warn!(
                            target: "hkask.mcp.docproc.tag_chunks",
                            batch = batch_idx,
                            error = %error,
                            response_preview = %utf8_prefix(&response.text, 500),
                            "Tag response rejected — batch will get visible fallback outcomes"
                        );
                    }
                    result

                });
                handles.push((start_idx, batch_len, handle));
            }

            // Only the owner records terminal outcomes. Batch identities survive
            // a panic because they are retained outside the spawned task.
            let mut results = Vec::with_capacity(total);
            for (start_idx, batch_len, handle) in handles {
                let outcome = match handle.await {
                    Ok(result) => result,
                    Err(error) => Err(format!("tagging batch task join failed: {error}")),
                };
                match outcome {
                    Ok(mut tags) => {
                        for chunk in &chunks[start_idx..start_idx + batch_len] {
                            let tags = tags.remove(&chunk.entity_ref).ok_or_else(|| {
                                McpToolError::internal("validated tagging response lost chunk_ref")
                            })?;
                            results.push((tags, ClassificationOutcome::Classified));
                        }
                    }
                    Err(reason) => {
                        tracing::warn!(start_idx, batch_len, %reason, "Tagging batch failed");
                        for _ in 0..batch_len {
                            results.push((
                                fallback_tags(),
                                ClassificationOutcome::Failed { reason: reason.clone() },
                            ));
                        }
                    }
                }
            }

            let c = results.iter().filter(|(_, outcome)| matches!(outcome, ClassificationOutcome::Classified)).count();
            let f = results.iter().filter(|(_, outcome)| matches!(outcome, ClassificationOutcome::Failed { .. })).count();
            if c + f != total {
                return Err(McpToolError::internal("tagging outcome count does not match input count"));
            }
            let elapsed = start_time.elapsed().as_secs_f64();
            tracing::info!("  Tagged: {} ok, {} failed, {:.1}s", c, f, elapsed);

            // Build tagged chunk outputs with salience
            let mut tagged: Vec<TaggedChunk> = chunks
                .iter()
                .zip(results)
                .map(|(chunk, (tags, classification))| {

                    // Union all ontology_tags values, normalized for graph consistency.
                    // The salience graph keys on exact strings, so case/whitespace
                    // variants of the same concept would be disconnected nodes.
                    // Normalization (lowercase + trim + collapse) merges them.
                    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                    let mut concepts: Vec<String> = Vec::new();
                    for concept_list in tags.ontology_tags.values() {
                        for c in concept_list {
                            let norm = normalize_concept(c);
                            if !norm.is_empty() && seen.insert(norm.clone()) {
                                concepts.push(norm);
                            }
                        }
                    }

                    let ontology = hkask_types::corpus::ChunkOntology {
                        dc_type: tags.dc_type.clone(),
                        dc_subject: tags.dc_subject.clone(),
                        dc_source: chunk.source.clone(),
                        pko_extracted_from: vec![chunk.entity_ref.clone()],
                        method_signals: Some(hkask_memory::salience::compute_method_signals(&chunk.text)),
                    };
                    let mut dimensions = tags.dimensions;
                    if !dimensions.iter().any(|dimension| dimension == "how") {
                        dimensions.push("how".to_string());
                    }
                    TaggedChunk {
                        entity_ref: chunk.entity_ref.clone(),
                        classification,
                        source: chunk.source.clone(),
                        text: chunk.text.clone(),
                        word_count: chunk.word_count,
                        dimensions,
                        dc_type: tags.dc_type,
                        dc_subject: tags.dc_subject,
                        ontology_tags: tags.ontology_tags,
                        concepts,
                        expertise_level: tags.expertise_level,
                        salience: 0.0,
                        consolidated_from: Vec::new(),
                        ontology: Some(ontology),
                    }
                })
                .collect();


            // Compute salience
            let salience_scores = compute_salience(&tagged);
            for (chunk, score) in tagged.iter_mut().zip(salience_scores) {
                chunk.salience = score;
            }

            // Write output JSONL
            let mut out = String::new();
            for chunk in &tagged {
                out.push_str(&serde_json::to_string(chunk)
                    .map_err(|e| McpToolError::internal(format!("Serialize: {e}")))?); // rr0044-ok: serde serialization of own struct
                out.push('\n');
            }
            crate::helpers::write_contained(&req.output, &out)?;

            // Stats
            let dim_counts: std::collections::HashMap<&str, usize> = {
                let mut m = std::collections::HashMap::new();
                for chunk in &tagged {
                    for dim in &chunk.dimensions {
                        *m.entry(dim.as_str()).or_default() += 1;
                    }
                }
                m
            };
            let exp_counts: std::collections::HashMap<&str, usize> = {
                let mut m = std::collections::HashMap::new();
                for chunk in &tagged {
                    *m.entry(chunk.expertise_level.as_str()).or_default() += 1;
                }
                m
            };

            let result = json!({
                "total_chunks": total,
                "tagged": c,
                "failed": f,
                "dimensions": dim_counts,
                "expertise_levels": exp_counts,
                "time_seconds": elapsed,
            });

            let outcome = BatchOutcome::from_counts(f, total);
            outcome.log_if_degraded("hkask.mcp.docproc.tag_chunks", "Tagging");
            Ok(result)
        })
        .await
    }
}

// ── Tag chunks request (ontology annotation) ───────────────────────────────

/// Default number of chunks to send in a single LLM call. Batching amortizes
/// the system prompt across multiple chunks, cutting API calls by ~10x.
const DEFAULT_TAG_BATCH_SIZE: usize = 10;

fn default_tag_batch_size() -> usize {
    DEFAULT_TAG_BATCH_SIZE
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct TagChunksRequest {
    /// Path to chunks JSONL (entity_ref, source, text, word_count per line).
    pub chunks_jsonl: String,
    /// Output path for tagged chunks JSONL with ontology annotations.
    pub output: String,
    /// Max concurrent LLM tagging calls (each call tags `tag_batch_size` chunks).
    #[serde(default = "default_tag_concurrency")]
    pub concurrency: usize,
    /// Number of chunks to tag in a single LMM call. Higher values amortize
    /// the system prompt but may exceed the model's context window for long
    /// chunks. Default 10.
    #[serde(default = "default_tag_batch_size")]
    pub tag_batch_size: usize,
    /// If true, only report stats without LLM calls or writing output.
    #[serde(default)]
    pub dry_run: bool,
}

fn default_tag_concurrency() -> usize {
    crate::max_concurrency()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
