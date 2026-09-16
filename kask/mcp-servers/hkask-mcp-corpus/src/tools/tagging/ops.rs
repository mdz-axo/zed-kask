//! Ontology tagging tools — multi-dimensional chunk annotation.
//!
//! `corpus_tag_chunks`: extracts exceptional 5W1H dimensions and descriptive
//! candidate terms. The server supplies universal dimensions, document type,
//! default expertise and published ontology anchors.

use crate::batch::{
    ADAPTIVE_CONCURRENCY_FLOOR, AdaptiveLimiter, BatchOutcome, MAX_RETRIES, retry_with_backoff,
};
use crate::{
    Arc, CorpusServer, LLMParameters, McpToolError, Parameters, execute_tool, json,
    normalize_concept, read_jsonl_stream, tool, tool_router,
};
use hkask_bridge_ontology::term_resolution::{
    CanonicalTerms, TERM_RESOLUTION_PROTOCOL, canonicalize_terms,
};
use hkask_inference::model_constants::classifier_model;
use hkask_inference::passage_tagging::{
    ExpertiseMode, Passage, PassageTag, PassageTaggingRequest, parse_tagging_response,
    render_deployed_tagging_prompt,
};
use hkask_types::corpus::{ClassificationOutcome, ExpertiseLevel, TaggedChunk};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

/// Maximum length of a single concept string after normalization.
/// Guards against LLM-produced or injected oversized concept strings that
/// would bloat embedding annotation prefixes and QA system prompts.
const MAX_CONCEPT_LEN: usize = 80;

/// Maximum candidate terms accepted from one classifier response.
const MAX_CANDIDATE_TERMS: usize = 5;

/// Minimal chunk for tagging (from chunks.jsonl).
#[derive(Debug, Clone, Deserialize)]
struct InputChunk {
    entity_ref: String,
    source: String,
    text: String,
    #[serde(default)]
    word_count: usize,
}

/// Validated structural judgments plus server-resolved ontology terms.
#[derive(Debug, Clone)]
struct ValidatedTags {
    dimensions: Vec<String>,
    dc_type: String,
    dc_subject: Vec<String>,
    canonical_terms: CanonicalTerms,
    expertise_level: ExpertiseLevel,
}

fn correlation_id(index: usize) -> String {
    format!("item-{index}")
}

fn tagging_request(chunks: &[InputChunk]) -> Result<PassageTaggingRequest, String> {
    let passages = chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| Passage::new(correlation_id(index), chunk.text.clone()))
        .collect();
    PassageTaggingRequest::new(
        passages,
        vec!["who", "when", "where", "why"],
        vec!["what", "how"],
        ExpertiseMode::ServerDerived,
    )
    .map_err(|error| error.to_string())
}

/// Accept a response only when it covers exactly this batch's correlation set.
/// Canonical entity refs never enter model authority: validated short IDs are
/// restored to their original refs before any `TaggedChunk` is published.
fn correlate_tags(
    text: &str,
    chunks: &[InputChunk],
) -> Result<(HashMap<String, ValidatedTags>, bool), String> {
    let request = tagging_request(chunks)?;
    let (response, repaired_outer_array) = repair_missing_outer_array(text);
    let tags = parse_tagging_response(&request, &response).map_err(|error| error.to_string())?;
    let correlated = chunks
        .iter()
        .zip(tags)
        .map(|(chunk, tags)| {
            validate_candidate_tags(tags).map(|validated| (chunk.entity_ref.clone(), validated))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok((correlated, repaired_outer_array))
}

/// Preserve the corpus path's one narrow provider repair: complete tuples that
/// are missing only the outer array's final bracket. The shared parser still
/// enforces the exact tuple shape, correlation set, and field policy afterward.
fn repair_missing_outer_array(text: &str) -> (String, bool) {
    let cleaned = hkask_types::json_extract::extract_json_from_response(text);
    match serde_json::from_str::<serde_json::Value>(&cleaned) {
        Ok(_) => (cleaned, false),
        Err(error) if error.is_eof() && cleaned.trim_start().starts_with('[') => {
            let mut repaired = cleaned.trim_end().to_string();
            repaired.push(']');
            (repaired, true)
        }
        Err(_) => (cleaned, false),
    }
}

fn fallback_tags() -> ValidatedTags {
    ValidatedTags {
        dimensions: vec!["what".into()],
        dc_type: hkask_bridge_ontology::dc_bibo::DOCUMENT.into(),
        dc_subject: Vec::new(),
        canonical_terms: CanonicalTerms::default(),
        expertise_level: ExpertiseLevel::Analyst,
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
// connectedness × (1 − redundancy) graph-centrality core. Descriptive
// candidate terms feed the graph; canonical ontology anchors are too coarse for
// this purpose when exact resolution correctly reaches the shared 5W1H core.
// Candidates remain semantic metadata, never namespace or URI authority. 5W1H
// dimensions are excluded because their six-value clique drowns useful signal.
//
// Method signals are stored separately as numeric ontology metadata, not
// mixed into this concept graph as if measurements were shared concepts.
fn compute_salience(tagged: &[TaggedChunk]) -> Vec<f32> {
    let all_tags: Vec<hkask_memory::salience::EntityTags> = tagged
        .iter()
        .map(|chunk| hkask_memory::salience::EntityTags {
            concepts: chunk.candidate_terms.clone(),
            ..Default::default()
        })
        .collect();
    hkask_memory::salience::compute_salience_batch(&all_tags)
}

/// Validate classifier judgments and resolve descriptive terms through the
/// shared published-ontology authority. Malformed responses fail visibly;
/// values are never silently promoted through fallback defaults.
fn validate_candidate_tags(tags: PassageTag) -> Result<ValidatedTags, String> {
    let candidate_terms = trim_and_cap_candidate_terms(&tags.candidate_terms);
    if candidate_terms.len() < 3 {
        return Err("candidate_terms must contain 3-5 descriptive terms".to_string());
    }
    let canonical_terms = canonicalize_terms(&candidate_terms);
    let dc_subject = normalize_and_cap_concept_list(&canonical_terms.candidate_terms);

    Ok(ValidatedTags {
        dimensions: tags.dimensions,
        dc_type: hkask_bridge_ontology::dc_bibo::DOCUMENT.to_string(),
        dc_subject,
        canonical_terms,
        expertise_level: ExpertiseLevel::Analyst,
    })
}

fn trim_and_cap_candidate_terms(raw: &[String]) -> Vec<String> {
    raw.iter()
        .map(|term| term.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|term| !term.is_empty())
        .map(|mut term| {
            if term.len() > MAX_CONCEPT_LEN {
                let prefix = utf8_prefix(&term, MAX_CONCEPT_LEN);
                let end = prefix.rfind(' ').unwrap_or(prefix.len());
                term.truncate(end);
            }
            term
        })
        .take(MAX_CANDIDATE_TERMS)
        .collect()
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
        if !norm.is_empty() && seen.insert(norm.clone()) && out.len() < MAX_CANDIDATE_TERMS {
            out.push(norm);
        }
    }
    out
}

#[tool_router(router = tagging_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Classify chunks with model-extracted exceptional 5W1H dimensions and descriptive candidate terms. The server supplies universal dimensions, document type and default expertise, resolves published ontology anchors, and computes candidate-term graph salience."
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
                    "note": "Would extract exceptional 5W1H dimensions + descriptive candidate terms, then derive document metadata and published ontology anchors server-side"
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

                    let tagging_request = tagging_request(&batch_chunks)?;
                    let prompt = render_deployed_tagging_prompt(&tagging_request)
                        .map_err(|error| error.to_string())?;

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
                    Ok((result, response.usage, response.cost_usd))

                });
                handles.push((start_idx, batch_len, handle));
            }

            // Only the owner records terminal outcomes. Batch identities survive
            // a panic because they are retained outside the spawned task.
            let mut results = Vec::with_capacity(total);
            let mut provider_responses = 0usize;
            let mut cost_reports = 0usize;
            let mut prompt_tokens = 0u64;
            let mut completion_tokens = 0u64;
            let mut total_tokens = 0u64;
            let mut reported_cost_usd = 0.0f64;
            let mut repaired_outer_arrays = 0usize;
            for (start_idx, batch_len, handle) in handles {
                let outcome = match handle.await {
                    Ok(Ok((result, usage, cost_usd))) => {
                        provider_responses += 1;
                        prompt_tokens += u64::from(usage.prompt_tokens);
                        completion_tokens += u64::from(usage.completion_tokens);
                        total_tokens += u64::from(usage.total_tokens);
                        if let Some(cost_usd) = cost_usd {
                            cost_reports += 1;
                            reported_cost_usd += cost_usd;
                        }
                        result
                    }
                    Ok(Err(error)) => Err(error),
                    Err(error) => Err(format!("tagging batch task join failed: {error}")),
                };
                match outcome {
                    Ok((mut tags, repaired_outer_array)) => {
                        if repaired_outer_array {
                            repaired_outer_arrays += 1;
                        }
                        for chunk in &chunks[start_idx..start_idx + batch_len] {
                            let tags = tags.remove(&chunk.entity_ref).ok_or_else(|| {
                                McpToolError::internal("validated tagging response lost chunk_ref")
                            })?;
                            results.push((
                                tags,
                                ClassificationOutcome::Classified {
                                    ontology_protocol: TERM_RESOLUTION_PROTOCOL.to_string(),
                                },
                            ));
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

            let c = results.iter().filter(|(_, outcome)| matches!(outcome, ClassificationOutcome::Classified { .. })).count();
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

                    let ontology = hkask_types::corpus::ChunkOntology {
                        dc_type: tags.dc_type.clone(),
                        dc_subject: tags.dc_subject.clone(),
                        dc_source: chunk.source.clone(),
                        pko_extracted_from: vec![chunk.entity_ref.clone()],
                        method_signals: Some(hkask_memory::salience::compute_method_signals(&chunk.text)),
                    };
                    let mut dimensions = vec!["what".to_string(), "how".to_string()];
                    for dimension in tags.dimensions {
                        if !dimensions.contains(&dimension) {
                            dimensions.push(dimension);
                        }
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
                        candidate_terms: tags.canonical_terms.candidate_terms,
                        ontology_tags: tags.canonical_terms.ontology_tags,
                        concepts: tags.canonical_terms.concepts,
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

            let cost_reporting_complete =
                provider_responses == num_batches && cost_reports == provider_responses;
            let reported_cost = cost_reporting_complete.then_some(reported_cost_usd);
            let result = json!({
                "total_chunks": total,
                "tagged": c,
                "failed": f,
                "dimensions": dim_counts,
                "expertise_levels": exp_counts,
                "time_seconds": elapsed,
                "planned_batches": num_batches,
                "provider_responses": provider_responses,
                "successful_response_usage": {
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "total_tokens": total_tokens,
                },
                "reported_cost_usd": reported_cost,
                "cost_reporting_complete": cost_reporting_complete,
                "repaired_outer_arrays": repaired_outer_arrays,
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
