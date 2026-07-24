//! Ontology tagging tools — multi-dimensional chunk annotation.
//!
//! `docproc_tag_chunks`: Tags each chunk with 5W1H interrogatory dimensions,
//! Dublin Core metadata, PKO process concepts, FIBO/GOLEM domain concepts,
//! and expertise level. Uses LLM-based extraction via a Jinja2 template.
//! Every chunk gets at least one 5W1H dimension — no zero-tag chunks.

use crate::tools::semantic::GUARD;
use crate::*;
use hkask_inference::model_constants::classifier_model;
use hkask_types::corpus::TaggedChunk;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Maximum length of a single concept string after normalization.
/// Guards against LLM-produced or injected oversized concept strings that
/// would bloat embedding annotation prefixes and QA system prompts.
const MAX_CONCEPT_LEN: usize = 80;

/// Maximum number of concepts per ontology namespace. Guards against
/// LLM-produced concept spam that would dominate the salience graph.
const MAX_CONCEPTS_PER_NS: usize = 30;

/// Failure-rate threshold above which the pipeline reports `degraded`
/// outcome. A run with more than 10% of chunks failing LLM extraction
/// indicates a systemic issue (model unavailable, prompt broken, or
/// adversarial input) and must not be reported as `success`.
const DEGRADED_FAILURE_THRESHOLD: usize = 10; // percent

/// Minimal chunk for tagging (from chunks.jsonl).
#[derive(Debug, Clone, Deserialize)]
struct InputChunk {
    entity_ref: String,
    source: String,
    text: String,
    #[serde(default)]
    #[allow(dead_code)]
    word_count: usize,
}

/// Ontology tags extracted by the LLM from a passage.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct OntologyTags {
    /// 5W1H interrogatory dimensions (at least one required).
    #[serde(default)]
    dimensions: Vec<String>,
    /// Dublin Core BIBO type (e.g., "bibo:Book").
    #[serde(default)]
    dc_type: String,
    /// Dublin Core subject keywords.
    #[serde(default)]
    dc_subject: Vec<String>,
    /// Flexible ontology tags keyed by namespace (e.g., "fibo", "golem", "omc", "pko", "other").
    #[serde(default)]
    ontology_tags: std::collections::HashMap<String, Vec<String>>,
    /// Expertise level — deserialized via `ExpertiseLevel`'s custom serde,
    /// which maps invalid strings to `Analyst`.
    #[serde(default)]
    expertise_level: hkask_types::corpus::ExpertiseLevel,
}

fn read_input_chunks(path: &str) -> Result<Vec<InputChunk>, McpToolError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        McpToolError::invalid_argument(format!("Cannot read chunks_jsonl '{path}': {e}"))
    })?;
    let total_lines = content.lines().filter(|l| !l.trim().is_empty()).count();
    let chunks: Vec<InputChunk> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let dropped = total_lines - chunks.len();
    if dropped > 0 {
        tracing::warn!("  Warning: dropped {dropped} malformed lines from input");
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
// NOTE: this path exercises ONLY the graph-centrality core. The richer parts of
// hkask_memory::salience (MethodSignals stylistic analysis, BudgetConfig h_mem
// budget gating, declared-entity tag_entities) are NOT used here — docproc
// tags are LLM-extracted ontology concepts, not declared named entities.
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
            if let Some(last_space) = norm[..MAX_CONCEPT_LEN].rfind(' ') {
                norm.truncate(last_space);
            } else {
                norm.truncate(MAX_CONCEPT_LEN);
            }
        }
        if !norm.is_empty() && seen.insert(norm.clone()) && out.len() < MAX_CONCEPTS_PER_NS {
            out.push(norm);
        }
    }
    out
}

#[tool_router(router = tagging_router, vis = "pub")]
impl DocProcServer {
    #[tool(
        description = "Tag chunks with multi-dimensional ontology annotations: 5W1H interrogatory dimensions, Dublin Core metadata, PKO process concepts, FIBO/GOLEM domain concepts, and expertise level. Uses LLM-based extraction via Jinja2 template. Computes graph-centrality salience. Every chunk gets at least one 5W1H dimension — no zero-salience chunks."
    )]
    pub async fn docproc_tag_chunks(
        &self,
        Parameters(req): Parameters<TagChunksRequest>,
    ) -> String {
        execute_tool(self, "docproc_tag_chunks", async {
            let chunks = read_input_chunks(&req.chunks_jsonl)?;
            if chunks.is_empty() {
                return Err(McpToolError::invalid_argument("chunks_jsonl is empty"));
            }
            let total = chunks.len();
            tracing::info!("  Tagging {} chunks with ontology dimensions...", total);

            if req.dry_run {
                return Ok(json!({
                    "total_chunks": total,
                    "dry_run": true,
                    "note": "Would tag each chunk with 5W1H + Dublin Core + PKO + FIBO + GOLEM + expertise level"
                }));
            }

            let sem = Arc::new(tokio::sync::Semaphore::new(req.concurrency.max(1)));
            let router = Arc::clone(&self.inference_router);
            let model_override = classifier_model();

            // Results: index → OntologyTags
            let results: Arc<std::sync::Mutex<Vec<Option<OntologyTags>>>> =
                Arc::new(std::sync::Mutex::new(
                    (0..total).map(|_| None).collect(),
                ));

            let start_time = std::time::Instant::now();
            let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let failed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

            let mut handles = Vec::with_capacity(total);

            for (i, chunk) in chunks.iter().enumerate() {
                let router = Arc::clone(&router);
                let sem = Arc::clone(&sem);
                let results = Arc::clone(&results);
                let completed = Arc::clone(&completed);
                let failed = Arc::clone(&failed);
                let model_override = model_override.clone();
                let text = chunk.text.clone();
                let source = chunk.source.clone();
                let chunk_id = chunk.entity_ref.clone();
                let handle = tokio::spawn(async move {
                    let _permit = sem.acquire().await;

                    // Render tagging prompt from Jinja2 template
                    let mut vars = std::collections::HashMap::new();
                    vars.insert("text", text.clone());
                    vars.insert("source", source.clone());
                    vars.insert("chunk_id", chunk_id.clone());
                    let prompt = render_docproc_template("tag-chunks", &vars);
                    let prompt = if prompt.is_empty() {
                        // Fallback if template not found
                        format!(
                            "Analyze this passage and extract ontology tags.\n\nPassage (chunk {chunk_id}, source: {source}):\n{text}\n\nRespond with JSON: {{\"dimensions\": [\"what\"], \"dc_type\": \"bibo:Document\", \"dc_subject\": [], \"pko_concepts\": [], \"fibo_concepts\": [], \"golem_concepts\": [], \"other_concepts\": [], \"expertise_level\": \"analyst\"}}"
                        )
                    } else {
                        prompt
                    };

                    // ContentGuard input scan — ALWAYS active on the tagging boundary.
                    // The docproc pipeline ingests PDFs, HTML, and plain text — "operator-curated"
                    // is a trust assumption, not a guarantee. A poisoned PDF chunk can
                    // contain prompt-injection text that reaches the LLM unfiltered if the
                    // guard is disabled. The output guard (scan_output) only strips secrets;
                    // it cannot detect that the LLM's JSON output was hijacked. Therefore the
                    // input guard on this boundary is non-disableable. (M2 fix.)
                    let input_scan = GUARD.scan_input(&prompt);
                    if !input_scan.passed {
                        failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }

                    let params = LLMParameters {
                        temperature: 0.1,
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

                    // H2: Retry with exponential backoff (3 attempts)
                    let mut _last_error = String::new();
                    let response: Option<_> = {
                        let mut attempts = 0u32;
                        loop {
                            match router
                                .generate_with_model(&prompt, &params, Some(&model_override), None)
                                .await
                            {
                                Ok(resp) => break Some(resp),
                                Err(e) => {
                                    attempts += 1;
                                    _last_error = format!("{}", e);
                                    if attempts >= 3 {
                                        break None;
                                    }
                                    let backoff = std::time::Duration::from_secs(
                                        2u64.pow(attempts) * 5
                                    );
                                    tokio::time::sleep(backoff).await;
                                }
                            }
                        }
                    };

                    let parse_result = if let Some(response) = response {
                        // Got a response — parse it
                        let output_scan = GUARD.scan_output(&response.text);
                        let content = output_scan.output.content(&response.text);
                        let cleaned = extract_json_from_response(content);
                        serde_json::from_str::<OntologyTags>(&cleaned)
                            .map(validate_ontology_tags)
                            .ok()
                    } else {
                        None
                    };

                    if let Some(tags) = parse_result {
                        let mut results = results.lock().unwrap();
                        results[i] = Some(tags);
                        completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    } else {
                        // Fallback: minimal tags after retries exhausted
                        let fallback = OntologyTags {
                            dimensions: vec!["what".to_string()],
                            dc_type: "bibo:Document".to_string(),
                            dc_subject: Vec::new(),
                            ontology_tags: std::collections::HashMap::new(),
                            expertise_level: hkask_types::corpus::ExpertiseLevel::default(),
                        };
                        let mut results = results.lock().unwrap();
                        results[i] = Some(fallback);
                        failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }


                });
                handles.push(handle);
            }

            for handle in handles {
                let _ = handle.await;
            }

            let c = completed.load(std::sync::atomic::Ordering::Relaxed);
            let f = failed.load(std::sync::atomic::Ordering::Relaxed);
            let elapsed = start_time.elapsed().as_secs_f64();
            tracing::info!("  Tagged: {} ok, {} failed, {:.1}s", c, f, elapsed);

            // Build tagged chunk outputs with salience
            let tags_guard = results.lock().unwrap();
            let mut tagged: Vec<TaggedChunk> = chunks
                .iter()
                .enumerate()
                .map(|(i, chunk)| {
                    let tags = tags_guard[i].clone().unwrap_or_else(|| OntologyTags {
                        dimensions: vec!["what".to_string()],
                        dc_type: "bibo:Document".to_string(),
                        dc_subject: Vec::new(),
                        ontology_tags: std::collections::HashMap::new(),
                        expertise_level: hkask_types::corpus::ExpertiseLevel::default(),
                    });

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

                    TaggedChunk {
                        entity_ref: chunk.entity_ref.clone(),
                        source: chunk.source.clone(),
                        text: chunk.text.clone(),
                        word_count: chunk.word_count,
                        dimensions: tags.dimensions,
                        dc_type: tags.dc_type,
                        dc_subject: tags.dc_subject,
                        ontology_tags: tags.ontology_tags,
                        concepts,
                        expertise_level: tags.expertise_level,
                        salience: 0.0,
                        consolidated_from: Vec::new(),
                        ontology: None,
                    }
                })
                .collect();
            drop(tags_guard);

            // Compute salience
            let salience_scores = compute_salience(&tagged);
            for (i, s) in salience_scores.iter().enumerate() {
                tagged[i].salience = *s;
            }

            // Write output JSONL
            let mut out = String::new();
            for chunk in &tagged {
                out.push_str(&serde_json::to_string(chunk)
                    .map_err(|e| McpToolError::internal(format!("Serialize: {e}")))?);
                out.push('\n');
            }
            std::fs::write(&req.output, &out).map_err(|e| {
                McpToolError::internal(format!("Cannot write output '{}': {}", req.output, e))
            })?;

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

            // Outcome classification. A run is `degraded` when the failure rate
            // exceeds the threshold (default 10%). The old `f > total / 2` bar
            // silently reported a 49% failure rate as `success` — masking
            // systemic issues (model unavailable, prompt broken, adversarial
            // input). The 10% threshold is conservative: any sustained failure
            // rate above it indicates the pipeline should not be trusted to
            // produce training data without operator review. (M1 fix.)
            let failure_pct = if total == 0 { 0 } else { (f * 100) / total };
            let outcome = if failure_pct >= DEGRADED_FAILURE_THRESHOLD {
                "degraded"
            } else {
                "success"
            };
            if outcome == "degraded" {
                tracing::warn!(
                    target: "hkask.mcp.docproc.tag_chunks",
                    failed = f,
                    total = total,
                    failure_pct = failure_pct,
                    threshold_pct = DEGRADED_FAILURE_THRESHOLD,
                    "Tagging run degraded — failure rate exceeds threshold"
                );
            }
            self.record_experience(
                "docproc_tag_chunks",
                &format!("{} chunks", total),
                outcome,
                result.clone(),
            );
            Ok(result)
        })
        .await
    }
}

// ── Tag chunks request (ontology annotation) ───────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TagChunksRequest {
    /// Path to chunks JSONL (entity_ref, source, text, word_count per line).
    pub chunks_jsonl: String,
    /// Output path for tagged chunks JSONL with ontology annotations.
    pub output: String,
    /// Max concurrent LLM tagging calls.
    #[serde(default = "default_tag_concurrency")]
    pub concurrency: usize,
    /// If true, only report stats without LLM calls or writing output.
    #[serde(default)]
    pub dry_run: bool,
}

fn default_tag_concurrency() -> usize {
    128
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::corpus::TaggedChunk;

    #[test]
    fn salience_routes_through_memory_service_concepts_only() {
        // Two chunks share one concept -> connected (positive salience).
        // A chunk with no concepts is an isolate (0.0) — 5W1H dimensions alone
        // do NOT rescue it, confirming dimensions are excluded from the graph.
        let tagged = vec![
            TaggedChunk {
                entity_ref: "a".into(),
                source: "s".into(),
                text: "".into(),
                concepts: vec!["return on capital".into()],
                dimensions: vec!["what".into()],
                ..Default::default()
            },
            TaggedChunk {
                entity_ref: "b".into(),
                source: "s".into(),
                text: "".into(),
                concepts: vec!["return on capital".into()],
                dimensions: vec!["what".into()],
                ..Default::default()
            },
            TaggedChunk {
                entity_ref: "c".into(),
                source: "s".into(),
                text: "".into(),
                concepts: vec![],
                dimensions: vec!["what".into()],
                ..Default::default()
            },
        ];
        let scores = compute_salience(&tagged);
        assert_eq!(scores.len(), 3);
        assert!(
            scores[0] > 0.0,
            "connected chunk must have positive salience"
        );
        assert!(
            scores[1] > 0.0,
            "connected chunk must have positive salience"
        );
        assert_eq!(
            scores[2], 0.0,
            "concept-less chunk must be an isolate (0.0)"
        );
    }

    #[test]
    fn validate_ontology_tags_filters_invalid_dimensions() {
        let tags = OntologyTags {
            dimensions: vec!["who".into(), "invalid".into(), "what".into()],
            expertise_level: hkask_types::corpus::ExpertiseLevel::default(),
            ..Default::default()
        };
        let out = validate_ontology_tags(tags);
        assert_eq!(out.dimensions, vec!["who".to_string(), "what".to_string()]);
    }

    #[test]
    fn validate_ontology_tags_defaults_empty_dimensions_to_what() {
        let tags = OntologyTags::default();
        let out = validate_ontology_tags(tags);
        assert_eq!(out.dimensions, vec!["what".to_string()]);
    }

    #[test]
    fn validate_ontology_tags_defaults_invalid_expertise_to_analyst() {
        // The custom serde deserializer on ExpertiseLevel maps unknown
        // strings to Analyst. Test via JSON deserialization to exercise
        // the deserializer path.
        let json = r#"{"expertise_level":"guru"}"#;
        let tags: OntologyTags = serde_json::from_str(json).expect("parse");
        assert_eq!(
            tags.expertise_level,
            hkask_types::corpus::ExpertiseLevel::Analyst
        );
    }

    #[test]
    fn validate_ontology_tags_normalizes_concept_case_and_whitespace() {
        // C2 fix: case variants of the same concept must merge into one graph node.
        let mut ontology_tags = std::collections::HashMap::new();
        ontology_tags.insert(
            "fibo".to_string(),
            vec!["ROIC".into(), "roic ".into(), "Return On Capital".into()],
        );
        let tags = OntologyTags {
            ontology_tags,
            ..Default::default()
        };
        let out = validate_ontology_tags(tags);
        let fibo = out.ontology_tags.get("fibo").unwrap();
        assert_eq!(
            fibo.len(),
            2,
            "ROIC and roic merge; Return On Capital stays separate"
        );
        assert!(fibo.contains(&"roic".to_string()));
        assert!(fibo.contains(&"return on capital".to_string()));
    }

    #[test]
    fn validate_ontology_tags_normalizes_namespace_keys() {
        let mut ontology_tags = std::collections::HashMap::new();
        ontology_tags.insert("FIBO".to_string(), vec!["roic".into()]);
        let tags = OntologyTags {
            ontology_tags,
            ..Default::default()
        };
        let out = validate_ontology_tags(tags);
        assert!(
            out.ontology_tags.contains_key("fibo"),
            "namespace key must be lowercased"
        );
        assert!(!out.ontology_tags.contains_key("FIBO"));
    }

    #[test]
    fn validate_ontology_tags_caps_concept_length() {
        let long_concept = "a".repeat(MAX_CONCEPT_LEN + 50);
        let mut ontology_tags = std::collections::HashMap::new();
        ontology_tags.insert("other".to_string(), vec![long_concept]);
        let tags = OntologyTags {
            ontology_tags,
            ..Default::default()
        };
        let out = validate_ontology_tags(tags);
        let other = out.ontology_tags.get("other").unwrap();
        assert_eq!(other.len(), 1);
        assert!(
            other[0].len() <= MAX_CONCEPT_LEN,
            "concept must be truncated to MAX_CONCEPT_LEN"
        );
    }

    #[test]
    fn validate_ontology_tags_caps_concepts_per_namespace() {
        let many: Vec<String> = (0..(MAX_CONCEPTS_PER_NS + 10))
            .map(|i| format!("concept {i}"))
            .collect();
        let mut ontology_tags = std::collections::HashMap::new();
        ontology_tags.insert("other".to_string(), many);
        let tags = OntologyTags {
            ontology_tags,
            ..Default::default()
        };
        let out = validate_ontology_tags(tags);
        let other = out.ontology_tags.get("other").unwrap();
        assert_eq!(
            other.len(),
            MAX_CONCEPTS_PER_NS,
            "concept count must be capped"
        );
    }

    #[test]
    fn validate_ontology_tags_drops_empty_namespaces() {
        let mut ontology_tags = std::collections::HashMap::new();
        ontology_tags.insert("fibo".to_string(), vec!["   ".into(), "".into()]);
        let tags = OntologyTags {
            ontology_tags,
            ..Default::default()
        };
        let out = validate_ontology_tags(tags);
        assert!(
            out.ontology_tags.is_empty(),
            "namespace with only empty concepts must be dropped"
        );
    }

    #[test]
    fn validate_ontology_tags_normalizes_dc_subject() {
        let tags = OntologyTags {
            dc_subject: vec!["ROIC".into(), "roic".into(), "  Return On Capital  ".into()],
            ..Default::default()
        };
        let out = validate_ontology_tags(tags);
        assert_eq!(out.dc_subject.len(), 2);
        assert!(out.dc_subject.contains(&"roic".to_string()));
        assert!(out.dc_subject.contains(&"return on capital".to_string()));
    }
}
