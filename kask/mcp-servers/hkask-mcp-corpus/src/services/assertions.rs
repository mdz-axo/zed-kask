//! Assertion extraction service — concurrent h_mem extraction from corpus chunks.
//!
//! Extracted from `CorpusServer::extract_passages_batch` in `tools/semantic/mod.rs`.
//! Opens the DB once, shares it across concurrent tasks, and stores assertions as
//! h_mems with ontology-aware confidence capping.

use std::sync::Arc;

use hkask_bridge_ontology::golem;
use hkask_bridge_ontology::rdf;
use hkask_bridge_ontology::schema_org;
use hkask_mcp_server::server::McpToolError;
use hkask_types::HMemOntology;
use hkask_types::InferencePort;
use hkask_types::Visibility;
use hkask_types::template::LLMParameters;
use serde_json::json;

use crate::batch::{MAX_RETRIES, retry_with_backoff};
use crate::helpers::read_jsonl;
use crate::tools::semantic::{
    abstract_namespace_tag_key, assertion_confidence, predicate_to_dimension,
    read_ontology_namespaces, read_ontology_tags,
};
use crate::{extract_json_from_response, owner_webid, render_docproc_template};

/// Input for [`AssertionsService::extract`].
pub(crate) struct AssertionsRequest {
    pub chunks_jsonl: String,
    pub tagged_jsonl: Option<String>,
    pub max_assertions: usize,
    pub db_path: String,
    pub passphrase: String,
    pub owner: String,
    pub concurrency: usize,
}

/// Concurrent h_mem extraction from corpus chunks.
///
/// Holds the shared inference router. Each call to [`extract`] opens the
/// memory DB, loads ontology context, and processes chunks concurrently with
/// 3-attempt retry and confidence capping.
pub(crate) struct AssertionsService {
    inference_router: Arc<dyn InferencePort>,
}

impl AssertionsService {
    pub fn new(inference_router: Arc<dyn InferencePort>) -> Self {
        Self { inference_router }
    }

    /// Batch extract h_mems from chunks JSONL with concurrent LLM calls.
    ///
    /// Opens the DB once and shares it across all concurrent tasks via `Arc<MemoryStore>`.
    /// Each chunk gets a 3-attempt retry with backoff. Assertions are stored as h_mems
    /// with `entity = chunk.entity_ref`.
    ///
    /// When `tagged_jsonl` is provided, ontology tags from the tagging step are
    /// read and injected into the extraction prompt per-chunk, so the LLM uses
    /// the appropriate predicates (GOLEM for narrative, schema.org for expository).
    #[must_use = "result must be used"]
    pub async fn extract(
        &self,
        request: AssertionsRequest,
    ) -> Result<serde_json::Value, McpToolError> {
        let AssertionsRequest {
            chunks_jsonl: chunks_path,
            tagged_jsonl,
            max_assertions,
            db_path,
            passphrase,
            owner,
            concurrency,
        } = request;

        let chunks_values = read_jsonl::<serde_json::Value>(&chunks_path, "chunks_jsonl")?;

        // Parse chunks: each line has entity_ref and text
        let mut chunks: Vec<(String, String)> = Vec::new();
        for (i, v) in chunks_values.iter().enumerate() {
            let entity_ref = v
                .get("entity_ref")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let chunk_text = v
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if entity_ref.is_empty() || chunk_text.is_empty() {
                tracing::warn!(
                    target: "hkask.mcp.docproc.assertions",
                    line = i + 1,
                    "Skipping chunk with empty entity_ref or text"
                );
                continue;
            }
            chunks.push((entity_ref, chunk_text));
        }

        let total_chunks = chunks.len();
        if total_chunks == 0 {
            return Err(McpToolError::invalid_argument(
                "chunks_jsonl contains no valid chunks",
            ));
        }

        // Read ontology tags from tagged_jsonl (if provided) to inject into
        // extraction prompts. Maps entity_ref -> formatted ontology context.
        let ontology_map: std::collections::HashMap<String, String> =
            if let Some(tagged_path) = tagged_jsonl.as_deref() {
                read_ontology_tags(tagged_path)?
            } else {
                std::collections::HashMap::new()
            };
        let ontology_map = Arc::new(ontology_map);

        // Read ontology namespace sets per chunk (M4 fix). Used to cross-check
        // that a assertion's predicate namespace was actually tagged for the
        // chunk before bypassing the text-containment hallucination guard.
        // Without this, any `golem:`/`SEPIO:`/`pko:` predicate bypasses
        // the guard regardless of whether the chunk was tagged with that
        // ontology — allowing the LLM to emit abstract-namespace predicates
        // for chunks where that ontology was never detected.
        let namespace_map: std::collections::HashMap<String, std::collections::HashSet<String>> =
            if let Some(tagged_path) = tagged_jsonl.as_deref() {
                read_ontology_namespaces(tagged_path)?
            } else {
                std::collections::HashMap::new()
            };
        let namespace_map = Arc::new(namespace_map);

        // Open DB once, share across concurrent tasks
        let store = Arc::new(crate::helpers::open_memory_store(&db_path, &passphrase)?);
        let webid = owner_webid(&owner);
        let classifier = hkask_inference::model_constants::classifier_model();
        // Namespace is fixed to "doc" for corpus chunk extraction (no longer a request field).
        let ns = "doc".to_string();

        let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
        let router = Arc::clone(&self.inference_router);
        let succeeded = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let failed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h_mems_stored = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let mut handles = Vec::with_capacity(total_chunks);
        for (entity_ref, chunk_text) in chunks {
            let router = Arc::clone(&router);
            let sem = Arc::clone(&sem);
            let store = Arc::clone(&store);
            let classifier = classifier.clone();
            let ns = ns.clone();
            let succeeded = Arc::clone(&succeeded);
            let failed = Arc::clone(&failed);
            let h_mems_stored = Arc::clone(&h_mems_stored);
            let ontology_map = Arc::clone(&ontology_map);
            let namespace_map = Arc::clone(&namespace_map);

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await;

                // Build prompt from registry template
                let ontology_context = ontology_map.get(&entity_ref).cloned().unwrap_or_default();
                // Namespace set for this chunk (M4 cross-check). Empty if no
                // tagged_jsonl was provided or the chunk has no ontology tags.
                let chunk_namespaces = namespace_map.get(&entity_ref).cloned().unwrap_or_default();
                let mut vars: std::collections::HashMap<&str, String> =
                    std::collections::HashMap::new();
                vars.insert("limit", max_assertions.to_string());
                vars.insert("namespace", ns.clone());
                vars.insert("text", chunk_text.clone());
                vars.insert("ontology_context", ontology_context.clone());
                let prompt = render_docproc_template("extract-hmems", &vars);
                let prompt = if prompt.is_empty() {
                    // Fallback when the registry template is missing. The
                    // predicate lists are built from the same fixture-guarded
                    // bridge constants the template's vocabulary pins, so
                    // the fallback cannot drift from the verified term sets
                    // (GOLEM v1.1, schema.org release, RDF 1.1).
                    let golem_examples = [
                        golem::HAS_CHARACTER,
                        golem::PARTICIPANT_IN,
                        golem::HAS_SETTING,
                        golem::HAS_FEATURE,
                        golem::REFERS_TO,
                    ]
                    .join(", ");
                    let expository_predicates = schema_org::ALL_TERMS.join(", ");
                    let rdf_type = rdf::TYPE;
                    let ontology_hint = if ontology_context.is_empty() {
                        String::new()
                    } else {
                        format!(

"Ontology tags for this passage: {ontology_context}
Use GOLEM predicates ({golem_examples}, etc.) for narrative passages and standard RDF predicates ({rdf_type}, {expository_predicates}) for expository passages.")
                    };
                    format!(
                        "Extract up to {max_assertions} factual RDF triples from the following text.

First, classify the passage as narrative (story, characters, literary devices) or expository (concepts, analysis, arguments). Then extract assertions using the appropriate predicates:
  - Expository: {rdf_type}, {expository_predicates}
  - Narrative: {golem_examples}, etc.

Each triple: (subject, predicate, object, confidence). Prefix subjects with '{ns}:'.{ontology_hint}

Text:
{chunk_text}

Respond in JSON format: {{\"h_mems\": [{{\"subject\": \"...\", \"predicate\": \"...\", \"object\": \"...\", \"confidence\": 0.95}}]}}"
                    )
                } else {
                    prompt
                };

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
                    "hkask.mcp.docproc.assertions",
                    &entity_ref,
                    || router.generate_with_model(&prompt, &params, Some(&classifier), None),
                )
                .await
                {
                    Ok(resp) => resp,
                    Err(_) => {
                        failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                };

                // Output guard + JSON extraction
                let content = &response.text;
                let cleaned = extract_json_from_response(content);
                let h_mems: serde_json::Value = match serde_json::from_str(&cleaned) {
                    Ok(v) => v,
                    Err(_) => {
                        tracing::warn!(
                            target: "hkask.mcp.docproc.assertions",
                            entity = %entity_ref,
                            "LLM response was not valid JSON"
                        );
                        failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                };

                // Store assertions as h_mems — preserve subject in value for knowledge graph
                let mut stored = 0usize;
                if let Some(arr) = h_mems.get("h_mems").and_then(|v| v.as_array()) {
                    for assertion in arr {
                        let subject = assertion
                            .get("subject")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let predicate = assertion
                            .get("predicate")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let object = assertion.get("object").cloned().unwrap_or(json!(null));
                        let raw_confidence = assertion
                            .get("confidence")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.8);
                        let dimension = predicate_to_dimension(predicate);
                        let pred_ns = predicate.split(':').next().unwrap_or("").to_lowercase();

                        let confidence = assertion_confidence(
                            subject,
                            predicate,
                            &object,
                            raw_confidence,
                            &chunk_text,
                            &chunk_namespaces,
                        );
                        if confidence < raw_confidence {
                            let tag_key = abstract_namespace_tag_key(&pred_ns);
                            let untagged_abstract_ns =
                                matches!(tag_key, Some(key) if !chunk_namespaces.contains(key));
                            let reason = if untagged_abstract_ns {
                                format!(
                                    "abstract namespace '{}' (tag family '{}') not in chunk ontology tags {:?} — confidence capped at 0.5",
                                    pred_ns,
                                    tag_key.unwrap_or(pred_ns.as_str()),
                                    chunk_namespaces
                                )
                            } else {
                                "Triple subject/object not found in chunk text — confidence capped at 0.5".to_string()
                            };
                            tracing::warn!(
                                target: "hkask.mcp.docproc.assertions",
                                entity = %entity_ref,
                                subject = %subject,
                                predicate = %predicate,
                                "{reason}"
                            );
                        }

                        // Store subject + object in value so build_prompts can format
                        // triples as "subject --predicate--> object" with confidence.
                        let value = json!({
                            "subject": subject,
                            "object": object,
                        });
                        // The predicate's tag family is recorded as an
                        // open-world tag only when the chunk was actually
                        // tagged with it — the same cross-check that gates the
                        // confidence cap above, so a hallucinated namespace
                        // doesn't get an ontology anchor it never earned.
                        // GOLEM-family prefixes (gc/crm/dlp/lrmoo) are stored
                        // under the "golem" key to match the tagging-phase
                        // vocabulary.
                        //
                        // State-axis type: SEPIO's published `assertion`
                        // class — a statement that a proposition is true.
                        // (The former `dcterms:Assertion` was fabricated;
                        // Dublin Core publishes no such term.)
                        let mut ontology = HMemOntology::state(
                            hkask_bridge_ontology::sepio::ASSERTION,
                            vec![subject.to_string()],
                            entity_ref.clone(),
                        )
                        .with_dimension(dimension);
                        if let Some(tag_key) = abstract_namespace_tag_key(&pred_ns) {
                            if chunk_namespaces.contains(tag_key) {
                                ontology = ontology.with_ontology_tag(tag_key, predicate);
                            }
                        }

                        let h_mem = hkask_storage::HMem::new(&entity_ref, predicate, value, webid)
                            .with_visibility(Visibility::Public)
                            .with_confidence(confidence)
                            .with_ontology(ontology);
                        match store.store(h_mem) {
                            Ok(()) => stored += 1,
                            Err(e) => {
                                tracing::warn!(
                                    target: "hkask.mcp.docproc.assertions",
                                    entity = %entity_ref,
                                    error = %e,
                                    "Failed to store triple h_mem"
                                );
                            }
                        }
                    }
                }

                h_mems_stored.fetch_add(stored, std::sync::atomic::Ordering::Relaxed);
                succeeded.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            });
            handles.push(handle);
        }

        for handle in handles {
            if let Err(join_err) = handle.await {
                tracing::warn!(
                    target: "hkask.mcp.docproc.assertions",
                    error = %join_err,
                    "assertion extraction batch task join failed"
                );
            }
        }

        let succeeded = succeeded.load(std::sync::atomic::Ordering::Relaxed);
        let failed = failed.load(std::sync::atomic::Ordering::Relaxed);
        let h_mems_stored = h_mems_stored.load(std::sync::atomic::Ordering::Relaxed);

        let result = json!({
            "total_chunks": total_chunks,
            "succeeded": succeeded,
            "failed": failed,
            "h_mems_stored": h_mems_stored,
        });
        Ok(result)
    }
}
