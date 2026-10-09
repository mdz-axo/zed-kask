//! Local swarm knowledge tools — the kask-vernacular analogs of ABW's
//! `swarm_search_knowledge`, `swarm_generate_prompt`, and `swarm_generate_ontology`.
//!
//! Where ABW backs these with fermi's per-agent dreaming-memory KG + fermi's
//! LLM generation, the local analogs back them with the operator's own
//! `hkask-memory` `MemoryStore` (the knowledge graph — entity-attribute-value
//! triples, scoped per agent by an `agent:<agent_id>:` prefix) and the local
//! `InferencePort` (Ollama/cloud via the zed IPC bridge). No ABW round-trips.
//!
//! Design rationale: `kask/docs/diataxis/swarm_system/reference.md`
//! (knowledge tools section).
//!
//! Graceful degradation: `LazyLocalMemory::get` opens the
//! `MemoryStore` lazily. The SQLCipher passphrase is the ONE shared DB
//! passphrase (`HKASK_DB_PASSPHRASE`, resolved by the server's `run()`
//! from ctx.credentials → env → keychain). If the passphrase is empty or
//! too short, `get` returns an error and the search tool returns an empty
//! result with a `memory_unconfigured` note (never a panic, never a fabricated
//! hit — the `.rules` unwrap_or(0) trap), and the generate tools proceed
//! unseeded (memory is an enhancement, not a dependency).

use hkask_bridge_ontology::term_resolution::{TERM_RESOLUTION_PROTOCOL, canonicalize_terms};
use hkask_inference::passage_tagging::{
    ExpertiseMode, Passage, PassageTag, PassageTaggingRequest, parse_tagging_response,
    render_deployed_tagging_prompt,
};
use hkask_memory::MemoryStore;
use hkask_storage::HMem;
use hkask_types::{Dimension, HMemOntology, Visibility, WebID};
use std::sync::Arc;

use crate::error::LocalSwarmError;

/// The per-agent memory prefix. A local agent's "knowledge graph" is its
/// prefix-scoped slice of the operator's semantic memory.
pub const AGENT_PREFIX: &str = "agent:";

/// A lazily-opened `MemoryStore` for the local swarm knowledge tools.
///
/// Mirrors `LazyLocalSwarmRuntime`: the `run_server` factory is sync, so the
/// async `MemoryStore::open` is deferred to the first tool call. The store
/// is the operator's consolidated semantic memory; per-agent scoping is a
/// prefix (`agent:<agent_id>:`) on the shared store (one store, many
/// namespaces — the deep-module choice over a per-agent store).
pub struct LazyLocalMemory {
    db_path: String,
    passphrase: String,
    dim: usize,
    /// Self-healing handle — mirrors `CuratorStore`'s pattern. A transient
    /// DB open failure sets this to `None`; the next `get` call retries.
    /// This replaces the old `OnceCell` which made transient failures
    /// permanent for the process lifetime.
    inner: tokio::sync::RwLock<Option<Arc<MemoryStore>>>,
}

impl LazyLocalMemory {
    /// Store the config without initializing. The memory is constructed on the
    /// first `get` call.
    pub(crate) fn lazy(db_path: String, passphrase: String, dim: usize) -> Self {
        Self {
            db_path,
            passphrase,
            dim,
            inner: tokio::sync::RwLock::new(None),
        }
    }

    /// Get the semantic memory, opening it on the first call or retrying
    /// after a transient failure. Returns `Err` if the passphrase is
    /// unset/too short or the store fails to open — callers degrade
    /// gracefully (the `.rules` startup-failure-signal rule: a missing
    /// memory is signaled, not silently empty).
    pub(crate) async fn get(&self) -> Result<Arc<MemoryStore>, LocalSwarmError> {
        // Fast path: already open.
        if let Some(store) = self.inner.read().await.as_ref() {
            return Ok(store.clone());
        }
        // Slow path: open (or re-open after a transient failure).
        let store = self.open()?;
        let mut guard = self.inner.write().await;
        *guard = Some(store.clone());
        Ok(store)
    }

    /// Open the store from disk. Called by `get` when the handle is `None`.
    fn open(&self) -> Result<Arc<MemoryStore>, LocalSwarmError> {
        if self.passphrase.len() < 8 {
            return Err(LocalSwarmError::InvalidInput(format!(
                "DB passphrase too short ({} chars — need >=8; set \
                 HKASK_DB_PASSPHRASE). Local knowledge tools will degrade.",
                self.passphrase.len()
            )));
        }
        // Create the parent directory so a first-run open does not fail
        // on a missing data dir.
        if let Some(parent) = std::path::Path::new(&self.db_path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    LocalSwarmError::Io(format!(
                        "failed to create swarm memory dir {}: {e}",
                        parent.display()
                    ))
                })?;
            }
        }
        MemoryStore::open(&self.db_path, &self.passphrase, self.dim)
            .map(Arc::new)
            .map_err(|e| {
                LocalSwarmError::Database(format!("failed to open swarm memory store: {e}"))
            })
    }
}

/// A knowledge fragment returned by `swarm_search_knowledge_local`. Mirrors
/// the ABW envelope (matching knowledge fragments) but in kask terms: the
/// agent's semantic-memory triples that match the query.
#[derive(Debug, Clone, serde::Serialize)]
pub struct KnowledgeFragment {
    pub entity: String,
    pub attribute: String,
    pub value: String,
    pub confidence: f64,
}

/// Search an agent's prefix-scoped semantic memory for triples whose
/// entity/attribute/value contain the query (case-insensitive substring).
///
/// This is the EAV (graph) retrieval path — "memory as a graph". It does not
/// require an embedding model, so it works whenever the memory store is
/// configured (passphrase set), independent of the embedding backend. Returns
/// an empty vec (not an error) when the agent has no matching memory.
pub(crate) async fn search_agent_knowledge(
    memory: &LazyLocalMemory,
    agent_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<KnowledgeFragment>, LocalSwarmError> {
    let store = match memory.get().await {
        Ok(s) => s,
        Err(reason) => {
            tracing::warn!(target: "hkask.mcp.swarm", error = %reason, "swarm memory unavailable — search returns empty");
            return Err(reason);
        }
    };
    let entity = format!("{AGENT_PREFIX}{agent_id}");
    let triples = store
        .query_deduped(&entity)
        .map_err(|e| LocalSwarmError::Database(format!("semantic memory query failed: {e}")))?;
    let needle = query.to_lowercase();
    let mut fragments: Vec<KnowledgeFragment> = triples
        .into_iter()
        .filter(|t| {
            if needle.is_empty() {
                return true;
            }
            t.entity.to_lowercase().contains(&needle)
                || t.attribute.to_lowercase().contains(&needle)
                || t.value.to_string().to_lowercase().contains(&needle)
        })
        .map(|t| KnowledgeFragment {
            entity: t.entity,
            attribute: t.attribute,
            value: t.value.to_string(),
            confidence: t.confidence.value(),
        })
        .collect();
    fragments.truncate(limit.max(1));
    Ok(fragments)
}

/// Retrieve an agent's seed memory as a prompt-context string (for the
/// generate tools). Returns an empty string when memory is unconfigured or the
/// agent has no memory — the generate tools then proceed unseeded.
pub(crate) async fn agent_memory_seed(
    memory: &LazyLocalMemory,
    agent_id: &str,
    limit: usize,
) -> String {
    match search_agent_knowledge(memory, agent_id, "", limit).await {
        Ok(fragments) if !fragments.is_empty() => {
            let lines: Vec<String> = fragments
                .into_iter()
                .map(|f| format!("- ({}, {}): {}", f.entity, f.attribute, f.value))
                .collect();
            format!(
                "Known facts about agent '{}' from consolidated memory:\n{}",
                agent_id,
                lines.join("\n")
            )
        }
        _ => String::new(),
    }
}

/// Record a delegation performance annotation to the agent's prefix-scoped
/// semantic memory — the ACO stigmergic pheromone trail. After each
/// `swarm_delegate_local`, the latency, task-success verdict, and response
/// text are written as `HMem` triples under `agent:<agent_id>:delegation`. The
/// SENSE phase (or any caller) can then query these via
/// `swarm_search_knowledge_local` to assess agent fitness across cascade
/// invocations, and the condenser's extraction pipeline can be applied to the
/// persisted responses as a second step.
///
/// Failures are logged with `tracing::warn!`, not swallowed (the `.rules` trap
/// on silent error discarding — a failed stigmergy write must be visible in
/// logs, not silently dropped). The delegation result is still returned to the
/// caller regardless of whether the annotation was written.
///
/// The stigmergy trail retains the latency, task-success, and response
/// annotations (the ACO pheromone signals + the dreaming substrate for
/// the condenser).
pub(crate) async fn record_delegation(
    memory: &LazyLocalMemory,
    agent_id: &str,
    latency_ms: u64,
    task_success_pass: Option<bool>,
    response: &str,
) {
    let store = match memory.get().await {
        Ok(s) => s,
        Err(reason) => {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                error = %reason,
                "stigmergy write skipped — swarm memory unavailable (non-fatal)"
            );
            return;
        }
    };
    let owner = WebID::for_agent_name("swarm_delegate_local");
    let entity = format!("{AGENT_PREFIX}{agent_id}");

    // Process-axis anchoring (P5.4): a stigmergy annotation is a PKO step
    // execution of the delegation procedure, not a standalone fact. Anchoring
    // it this way is what lets the SENSE phase distinguish pheromone trails
    // (process traces) from consolidated agent facts in the same store.
    let ontology = HMemOntology::process("swarm_delegate", "record", agent_id);

    // Write the latency annotation.
    let mut h_mem = HMem::new(
        &entity,
        "delegation:latency_ms",
        serde_json::json!(latency_ms),
        owner,
    )
    .with_ontology(ontology.clone());
    h_mem.access.visibility = Visibility::Shared;
    if let Err(e) = store.store(h_mem) {
        tracing::warn!(
            target: "hkask.mcp.swarm",
            error = %e,
            "stigmergy latency write failed (non-fatal)"
        );
    }

    // Write the task-success annotation only when a verdict was supplied
    // (null task_success = open task, no oracle — do not fabricate).
    if let Some(pass) = task_success_pass {
        let mut h_mem = HMem::new(
            &entity,
            "delegation:task_success",
            serde_json::json!(pass),
            owner,
        )
        .with_ontology(ontology.clone());
        h_mem.access.visibility = Visibility::Shared;
        if let Err(e) = store.store(h_mem) {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                error = %e,
                "stigmergy task_success write failed (non-fatal)"
            );
        }
    }

    // Write the delegation response as an experience record. This is the
    // dreaming substrate — the condenser's extraction pipeline can be applied
    // to these persisted responses as a second step, and the SENSE phase can
    // recall them via `swarm_search_knowledge_local`. The response is capped
    // at 64KB to prevent unbounded memory growth (mirrors the cap in
    // `AgentExecutor::run`'s tool-result handling).
    let capped_response: String = if response.len() > 64 * 1024 {
        response.chars().take(64 * 1024).collect()
    } else {
        response.to_string()
    };
    let mut h_mem = HMem::new(
        &entity,
        "delegation:response",
        serde_json::Value::String(capped_response),
        owner,
    )
    .with_ontology(ontology);
    h_mem.access.visibility = Visibility::Shared;
    if let Err(e) = store.store(h_mem) {
        tracing::warn!(
            target: "hkask.mcp.swarm",
            error = %e,
            "stigmergy response write failed (non-fatal)"
        );
    }
}

// ── Narrative response passage memory ─────────────────────────────────────

const MIN_RESPONSE_CHUNK_WORDS: usize = 30;
const MAX_RESPONSE_CHUNK_WORDS: usize = 400;
const RESPONSE_SENTENCE_BOUNDARY: &str = ".!?";

/// Model-facing tagging outcome for one narrative-turn ingestion.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "status", content = "reason", rename_all = "snake_case")]
pub enum TaggingOutcome {
    Tagged,
    Degraded(String),
    NotAttempted,
}

/// A visible, typed failure that reduced the durable memory produced for a turn.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum MemoryDegradation {
    StoreUnavailable(String),
    HMemStoreFailed(String),
    EmbeddingModelUnconfigured,
    EmbeddingFailed(String),
    EmbeddingCountMismatch { expected: usize, actual: usize },
    EmbeddingStoreFailed(String),
}

/// Exact durable accounting for one narrative-turn ingestion attempt.
#[derive(
    Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct TurnIngestionReport {
    pub attempted: usize,
    pub stored: usize,
    pub embedded: usize,
    pub failed: usize,
    pub tagging: TaggingOutcome,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degradations: Vec<MemoryDegradation>,
}

impl Default for TurnIngestionReport {
    fn default() -> Self {
        Self {
            attempted: 0,
            stored: 0,
            embedded: 0,
            failed: 0,
            tagging: TaggingOutcome::NotAttempted,
            degradations: Vec::new(),
        }
    }
}

impl TurnIngestionReport {
    pub fn degraded(&self) -> bool {
        self.failed > 0 || matches!(self.tagging, TaggingOutcome::Degraded(_))
    }
}

async fn tag_response_chunks(
    inference: &Arc<dyn hkask_types::InferencePort>,
    chunk_texts: &[String],
    classifier_model: Option<&str>,
) -> (Option<Vec<PassageTag>>, TaggingOutcome) {
    let Some(classifier_model) = classifier_model else {
        return (
            None,
            TaggingOutcome::Degraded(
                "classifier model unconfigured (HKASK_CLASSIFIER_MODEL)".to_string(),
            ),
        );
    };
    let passages = chunk_texts
        .iter()
        .enumerate()
        .map(|(index, text)| Passage::new(format!("chunk-{index}"), text.clone()))
        .collect();
    let request = match PassageTaggingRequest::new(
        passages,
        vec!["what", "why"],
        vec!["who", "when", "where", "how"],
        ExpertiseMode::ModelAssigned,
    ) {
        Ok(request) => request,
        Err(error) => {
            return (
                None,
                TaggingOutcome::Degraded(format!("tagging request rejected: {error}")),
            );
        }
    };
    let prompt = match render_deployed_tagging_prompt(&request) {
        Ok(prompt) => prompt,
        Err(error) => {
            return (
                None,
                TaggingOutcome::Degraded(format!("tagging template unavailable: {error}")),
            );
        }
    };
    let parameters = hkask_types::LLMParameters {
        temperature: 0.1,
        thinking_allowed: false,
        ..Default::default()
    };
    let result = match inference
        .generate_with_model(&prompt, &parameters, Some(classifier_model), None)
        .await
    {
        Ok(result) => result,
        Err(error) => {
            return (
                None,
                TaggingOutcome::Degraded(format!("tagging inference failed: {error}")),
            );
        }
    };
    match parse_tagging_response(&request, &result.text) {
        Ok(tags) => (Some(tags), TaggingOutcome::Tagged),
        Err(error) => (
            None,
            TaggingOutcome::Degraded(format!("tagging response rejected: {error}")),
        ),
    }
}

fn response_chunk_ontology(
    agent_id: &str,
    turn_id: &str,
    chunk_index: usize,
    tags: Option<&PassageTag>,
) -> HMemOntology {
    let mut ontology = HMemOntology {
        dimensions: vec![
            Dimension::Who.as_str().to_string(),
            Dimension::When.as_str().to_string(),
            Dimension::Where.as_str().to_string(),
            Dimension::How.as_str().to_string(),
        ],
        dc_source: format!("swarm:{agent_id}:turn:{turn_id}"),
        pko_procedure: Some("swarm_delegate".to_string()),
        pko_step: Some(format!("response_chunk:{chunk_index}")),
        ..HMemOntology::default()
    };
    if let Some(tags) = tags {
        for dimension in &tags.dimensions {
            if !ontology.dimensions.contains(dimension) {
                ontology.dimensions.push(dimension.clone());
            }
        }
        let canonical = canonicalize_terms(&tags.candidate_terms);
        ontology.dc_subject = canonical.candidate_terms.clone();
        ontology.candidate_terms = canonical.candidate_terms;
        ontology.ontology_tags = canonical.ontology_tags;
        ontology.ontology_protocol = Some(TERM_RESOLUTION_PROTOCOL.to_string());
        ontology.expertise_level = tags.expertise;
    }
    ontology
}

/// Ingest a completed delegation response as bounded, response-bearing passages.
/// Every chunk has one provenance-rich h_mem and, when available, one embedding
/// under the exact same entity ref with `passage_text` equal to the chunk text.
pub(crate) async fn ingest_turn(
    memory: &LazyLocalMemory,
    inference: &Arc<dyn hkask_types::InferencePort>,
    agent_id: &str,
    task: &str,
    response: &str,
    model: &str,
    classifier_model: Option<&str>,
    embedding_model: Option<&str>,
) -> TurnIngestionReport {
    let turn_id = uuid::Uuid::new_v4().to_string();
    let entity_prefix = format!("{AGENT_PREFIX}{agent_id}:turn:{turn_id}:chunk");
    let chunk_config = match hkask_memory::text_chunking::ChunkConfig::new(
        MIN_RESPONSE_CHUNK_WORDS,
        MAX_RESPONSE_CHUNK_WORDS,
        0,
        RESPONSE_SENTENCE_BOUNDARY,
    ) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(target: "hkask.mcp.swarm", %error, "invalid response chunk contract");
            return TurnIngestionReport::default();
        }
    };
    let chunking =
        hkask_memory::text_chunking::chunk_text_with_config(response, &entity_prefix, chunk_config);
    if chunking.chunks.is_empty() {
        return TurnIngestionReport::default();
    }
    let chunk_texts: Vec<String> = chunking
        .chunks
        .iter()
        .map(|chunk| chunk.text.clone())
        .collect();
    let attempted = chunking.chunks.len();
    let (tags, tagging) = tag_response_chunks(inference, &chunk_texts, classifier_model).await;
    if let TaggingOutcome::Degraded(reason) = &tagging {
        tracing::warn!(target: "hkask.mcp.swarm", agent = %agent_id, %reason, "response passage tagging degraded (non-fatal)");
    }

    let store = match memory.get().await {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(target: "hkask.mcp.swarm", agent = %agent_id, %error, "response passage store unavailable (non-fatal)");
            return TurnIngestionReport {
                attempted,
                failed: attempted,
                tagging,
                degradations: vec![MemoryDegradation::StoreUnavailable(error.to_string())],
                ..TurnIngestionReport::default()
            };
        }
    };

    let mut degradations = Vec::new();
    let vectors = match embedding_model {
        Some(embedding_model) => match inference
            .embed_with_dimensions(embedding_model, &chunk_texts, Some(memory.dim as u32))
            .await
        {
            Ok(batch) if batch.vectors.len() == attempted => Some(batch.vectors),
            Ok(batch) => {
                degradations.push(MemoryDegradation::EmbeddingCountMismatch {
                    expected: attempted,
                    actual: batch.vectors.len(),
                });
                None
            }
            Err(error) => {
                degradations.push(MemoryDegradation::EmbeddingFailed(error.to_string()));
                None
            }
        },
        None => {
            degradations.push(MemoryDegradation::EmbeddingModelUnconfigured);
            None
        }
    };

    let owner = WebID::for_agent_name("swarm_delegate_local");
    let mut report = TurnIngestionReport {
        attempted,
        tagging,
        degradations,
        ..TurnIngestionReport::default()
    };
    for (index, chunk) in chunking.chunks.iter().enumerate() {
        let value = serde_json::json!({
            "agent_id": agent_id,
            "task": task,
            "model": model,
            "turn_id": turn_id,
            "chunk_index": index,
            "text": chunk.text,
        });
        let mut h_mem = HMem::new(&chunk.entity_ref, "delegation:response_chunk", value, owner)
            .with_ontology(response_chunk_ontology(
                agent_id,
                &turn_id,
                index,
                tags.as_ref().and_then(|all| all.get(index)),
            ));
        h_mem.access.visibility = Visibility::Shared;
        let stored = match store.store(h_mem) {
            Ok(()) => {
                report.stored += 1;
                true
            }
            Err(error) => {
                report
                    .degradations
                    .push(MemoryDegradation::HMemStoreFailed(error.to_string()));
                false
            }
        };
        let embedded = if stored {
            match (
                vectors.as_ref().and_then(|all| all.get(index)),
                embedding_model,
            ) {
                (Some(vector), Some(embedding_model)) => match store.store_embedding(
                    &chunk.entity_ref,
                    vector,
                    embedding_model,
                    Some(&chunk.text),
                ) {
                    Ok(_) => {
                        report.embedded += 1;
                        true
                    }
                    Err(error) => {
                        report
                            .degradations
                            .push(MemoryDegradation::EmbeddingStoreFailed(error.to_string()));
                        false
                    }
                },
                _ => false,
            }
        } else {
            false
        };
        if !stored || !embedded {
            report.failed += 1;
        }
    }
    if report.degraded() {
        tracing::warn!(target: "hkask.mcp.swarm", agent = %agent_id, attempted = report.attempted, stored = report.stored, embedded = report.embedded, failed = report.failed, "response passage ingestion completed with degradation");
    }
    report
}

/// One response passage recalled from the shared local-swarm knowledgebase.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecalledPassage {
    pub agent_id: String,
    pub task: String,
    pub model: String,
    pub turn_id: String,
    pub chunk_index: usize,
    pub passage: String,
    pub distance: f64,
}

/// Recall result plus the model-gate degradation count:
/// `excluded_model_mismatch` counts stored rows dropped from the KNN window
/// because their recorded model matches neither the query's requested nor
/// its provider-confirmed actual model — surfaced by the recall tool so an
/// embedding-model migration reads as degradation, never as an empty store.
pub(crate) struct RecallOutcome {
    pub passages: Vec<RecalledPassage>,
    pub excluded_model_mismatch: usize,
}

/// Recall response passages across all agents, or only one producing agent.
pub(crate) async fn recall_turns(
    memory: &LazyLocalMemory,
    inference: &Arc<dyn hkask_types::InferencePort>,
    query: &str,
    limit: usize,
    agent_filter: Option<&str>,
    embedding_model: Option<&str>,
) -> Result<RecallOutcome, LocalSwarmError> {
    let store = memory.get().await?;
    let embedding_model = embedding_model.ok_or_else(|| {
        LocalSwarmError::Unavailable(
            "no embedding model configured — set kask.models.embedding_model (injected as HKASK_EMBEDDING_MODEL)"
                .to_string(),
        )
    })?;
    let batch = inference
        .embed_with_dimensions(
            embedding_model,
            &[query.to_string()],
            Some(memory.dim as u32),
        )
        .await
        .map_err(|error| {
            LocalSwarmError::Unavailable(format!("embedding the recall query failed: {error}"))
        })?;
    if batch.vectors.len() != 1 {
        return Err(LocalSwarmError::Unavailable(format!(
            "embedding the recall query returned {} vectors; expected exactly 1",
            batch.vectors.len()
        )));
    }
    let query_vector = batch.vectors.into_iter().next().ok_or_else(|| {
        LocalSwarmError::Unavailable(
            "embedding model returned no vector for the recall query".to_string(),
        )
    })?;
    let knn_limit = if agent_filter.is_some() {
        limit.saturating_mul(5).max(50)
    } else {
        limit
    };
    let outcome = store
        .search_similar(
            &query_vector,
            knn_limit,
            embedding_model,
            batch.actual_model.as_deref(),
        )
        .map_err(|error| {
            LocalSwarmError::Database(format!("semantic search over swarm memory failed: {error}"))
        })?;
    if outcome.excluded_model_mismatch > 0 {
        tracing::warn!(
            target: "hkask.mcp.swarm",
            excluded = outcome.excluded_model_mismatch,
            query_model = embedding_model,
            "KNN excluded stored swarm embeddings from a different model — re-embed pending"
        );
    }
    let results = outcome.results;
    let scope_prefix = agent_filter.map(|agent_id| format!("{AGENT_PREFIX}{agent_id}:turn:"));
    let mut passages = Vec::with_capacity(results.len());
    for result in results {
        let entity_ref = &result.embedding.entity_ref;
        if let Some(prefix) = &scope_prefix
            && !entity_ref.starts_with(prefix)
        {
            continue;
        }
        let Some(passage) = result.embedding.passage_text.clone() else {
            tracing::warn!(target: "hkask.mcp.swarm", %entity_ref, "KNN hit has no passage_text — skipping (non-fatal)");
            continue;
        };
        let h_mems = match store.query_deduped_untouched(entity_ref) {
            Ok(h_mems) => h_mems,
            Err(error) => {
                tracing::warn!(target: "hkask.mcp.swarm", %error, %entity_ref, "failed to resolve KNN hit provenance — skipping (non-fatal)");
                continue;
            }
        };
        let Some(h_mem) = h_mems
            .into_iter()
            .find(|h_mem| h_mem.attribute == "delegation:response_chunk")
        else {
            tracing::warn!(target: "hkask.mcp.swarm", %entity_ref, "KNN hit has no response chunk h_mem — skipping (non-fatal)");
            continue;
        };
        let text = h_mem.value.get("text").and_then(serde_json::Value::as_str);
        if text != Some(passage.as_str()) {
            tracing::warn!(target: "hkask.mcp.swarm", %entity_ref, "KNN passage_text does not match h_mem chunk text — skipping (non-fatal)");
            continue;
        }
        let Some(agent_id) = h_mem
            .value
            .get("agent_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let Some(task) = h_mem
            .value
            .get("task")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let Some(model) = h_mem
            .value
            .get("model")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let Some(turn_id) = h_mem
            .value
            .get("turn_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let Some(chunk_index) = h_mem
            .value
            .get("chunk_index")
            .and_then(serde_json::Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
        else {
            continue;
        };
        passages.push(RecalledPassage {
            agent_id,
            task,
            model,
            turn_id,
            chunk_index,
            passage,
            distance: result.distance,
        });
    }
    passages.truncate(limit);
    Ok(RecallOutcome {
        passages,
        excluded_model_mismatch: outcome.excluded_model_mismatch,
    })
}

///
/// `inference` is the resolved local `InferencePort` (from `LocalSwarmRuntime`).
/// Returns the generated text.
pub(crate) async fn one_shot_generate(
    inference: &Arc<dyn hkask_types::InferencePort>,
    prompt: &str,
    temperature: f32,
) -> Result<String, LocalSwarmError> {
    let params = hkask_types::template::LLMParameters {
        temperature,
        ..hkask_types::template::LLMParameters::default()
    };
    // Route through the classifier-class model (`kask.models.classifier_model`,
    // env `HKASK_CLASSIFIER_MODEL`), resolved or refused — never a silent
    // None fallback to the host default chat model, which can be
    // thinking-mandatory (OpenRouter 400 — the d58fd87b class;
    // refuse-on-None ruling 2026-10-09). The same resolution the ai-assist
    // paths and the eval runtime use.
    let classifier = hkask_inference::model_constants::resolve_classifier_model().map_err(|e| {
        LocalSwarmError::Unavailable(format!(
            "one-shot generate requires the classifier model: {e}"
        ))
    })?;
    let result = inference
        .generate_with_model(prompt, &params, Some(classifier.as_str()), None)
        .await
        .map_err(|e| {
            LocalSwarmError::Unavailable(format!("local inference generate failed: {e}"))
        })?;
    Ok(result.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;

    fn test_dim() -> usize {
        hkask_storage::embedding_dim()
    }

    const TEST_PASSPHRASE: &str = "test-passphrase";

    /// Card d58fd87b class + the 2026-10-09 refuse-on-None ruling:
    /// `one_shot_generate` (the shared helper behind
    /// `swarm_generate_prompt_local` / `swarm_generate_ontology_local`)
    /// routes through `generate_with_model` with the env-resolved classifier
    /// override, and refuses when the classifier model is unset — never a
    /// silent None fallback to the host default chat model (which can be
    /// thinking-mandatory → OpenRouter 400, both authoring aids dead).
    /// Both pins run as subprocess legs (the corpus retrieval_tests
    /// precedent): in-process env mutation is unsafe (edition 2024) and racy
    /// under parallel test threads.
    #[tokio::test]
    async fn one_shot_generate_routes_through_the_classifier_override()
    -> Result<(), Box<dyn std::error::Error>> {
        const LEG: &str = "KASS_ONESHOT_ROUTE_LEG";
        if std::env::var_os(LEG).is_some() {
            let recorder = Arc::new(crate::test_support::OverrideRecordingInference::default());
            let inference: Arc<dyn hkask_types::InferencePort> = recorder.clone();
            let text = one_shot_generate(&inference, "probe prompt", 0.4).await?;
            assert!(!text.is_empty(), "the stub's fixture text must come back");
            let calls = recorder.calls.lock().expect("calls");
            assert_eq!(calls.len(), 1, "one inference call per one-shot generate");
            assert_eq!(
                calls[0].0, "generate_with_model",
                "one_shot_generate must route through generate_with_model — bare generate() \
                 lands on the host default chat model, which can be thinking-mandatory (the \
                 d58fd87b drop shape)"
            );
            assert_eq!(
                calls[0].1.as_deref(),
                Some(crate::test_support::PROBE_CLASSIFIER_MODEL),
                "the override must be the env-resolved classifier value — never silently absent"
            );
            return Ok(());
        }
        crate::test_support::spawn_env_leg(
            LEG,
            "local_knowledge::tests::one_shot_generate_routes_through_the_classifier_override",
            true,
        )
        .await
    }

    /// An unset classifier model refuses: a typed error naming the setting,
    /// zero port calls — the default chat model is never served (RED on the
    /// pre-ruling tree, where the None arm silently succeeded).
    #[tokio::test]
    async fn one_shot_generate_refuses_without_the_classifier_model()
    -> Result<(), Box<dyn std::error::Error>> {
        const LEG: &str = "KASS_ONESHOT_REFUSE_LEG";
        if std::env::var_os(LEG).is_some() {
            let recorder = Arc::new(crate::test_support::OverrideRecordingInference::default());
            let inference: Arc<dyn hkask_types::InferencePort> = recorder.clone();
            let error = one_shot_generate(&inference, "probe prompt", 0.4)
                .await
                .expect_err("one-shot generate must refuse when the classifier model is unset");
            let message = error.to_string();
            assert!(
                message.contains("HKASK_CLASSIFIER_MODEL")
                    || message.contains("kask.models.classifier_model"),
                "the refusal must name the setting, got: {message}"
            );
            assert!(
                recorder.calls.lock().expect("calls").is_empty(),
                "no port call on refusal — the default chat model is never served"
            );
            return Ok(());
        }
        crate::test_support::spawn_env_leg(
            LEG,
            "local_knowledge::tests::one_shot_generate_refuses_without_the_classifier_model",
            false,
        )
        .await
    }

    #[derive(Clone, Copy)]
    enum EmbedMode {
        Exact,
        OneShort,
        SemanticRanking,
    }

    struct StubInference {
        dim: usize,
        mode: EmbedMode,
    }

    impl hkask_types::InferencePort for StubInference {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &hkask_types::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Ok(hkask_types::InferenceResult {
                    text: "[]".into(),
                    model: "stub-model".into(),
                    usage: hkask_types::InferenceUsage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                        reported: true,
                    },
                    finish_reason: "stop".into(),
                    tool_calls: vec![],
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }

        fn embed<'a>(&'a self, model: &str, texts: &[String]) -> hkask_types::EmbedFuture<'a> {
            let dim = self.dim;
            let requested_model = model.to_string();
            let vectors = match self.mode {
                EmbedMode::Exact => constant_vectors(texts.len(), dim),
                EmbedMode::OneShort => constant_vectors(texts.len().saturating_sub(1), dim),
                EmbedMode::SemanticRanking => texts
                    .iter()
                    .map(|text| {
                        let mut vector = vec![0.0f32; dim];
                        let axis = if text.contains("crimson compass")
                            || text.contains("future navigation")
                        {
                            0
                        } else {
                            1
                        };
                        if let Some(value) = vector.get_mut(axis) {
                            *value = 1.0;
                        }
                        vector
                    })
                    .collect(),
            };
            Box::pin(async move {
                Ok(hkask_types::EmbeddingBatch {
                    vectors,
                    requested_model,
                    actual_model: None,
                    usage: hkask_types::InferenceUsage::default(),
                    cost_usd: None,
                })
            })
        }
    }

    fn constant_vectors(count: usize, dim: usize) -> Vec<Vec<f32>> {
        (0..count)
            .map(|_| {
                let mut vector = vec![0.0f32; dim];
                if let Some(first) = vector.first_mut() {
                    *first = 1.0;
                }
                vector
            })
            .collect()
    }

    fn inference(mode: EmbedMode) -> Arc<dyn hkask_types::InferencePort> {
        Arc::new(StubInference {
            dim: test_dim(),
            mode,
        })
    }

    fn temp_memory() -> LazyLocalMemory {
        let path =
            std::env::temp_dir().join(format!("kask-swarm-mem-test-{}.db", uuid::Uuid::new_v4()));
        LazyLocalMemory::lazy(
            path.to_string_lossy().to_string(),
            TEST_PASSPHRASE.to_string(),
            test_dim(),
        )
    }

    fn long_response(words: usize) -> String {
        (0..words)
            .map(|index| format!("response_word_{index}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// expect: A long response produces one HMem and one exact-text embedding per bounded chunk.
    #[tokio::test]
    async fn long_response_reconciles_chunk_hmem_and_embedding_counts() {
        let memory = temp_memory();
        let report = ingest_turn(
            &memory,
            &inference(EmbedMode::Exact),
            "writer",
            "produce a long answer",
            &long_response(900),
            "answer-model",
            None,
            Some("embedding-model"),
        )
        .await;

        assert_eq!(report.attempted, 3);
        assert_eq!(report.stored, report.attempted);
        assert_eq!(report.embedded, report.attempted);
        assert_eq!(report.failed, 0);
        assert!(matches!(report.tagging, TaggingOutcome::Degraded(_)));

        let store = memory.get().await.expect("store opens");
        let h_mems = store
            .h_mems_by_entity_prefix("agent:writer:turn:")
            .expect("h_mems query succeeds");
        let embeddings = store
            .all_embeddings_with_text()
            .expect("embeddings query succeeds");
        assert_eq!(h_mems.len(), report.attempted);
        assert_eq!(embeddings.len(), report.attempted);
        for h_mem in h_mems {
            let chunk_text = h_mem.value["text"]
                .as_str()
                .expect("chunk h_mem carries text");
            let embedding = embeddings
                .iter()
                .find(|(entity_ref, _, _, _)| entity_ref == &h_mem.entity)
                .expect("embedding uses the exact h_mem entity");
            assert_eq!(embedding.2.as_deref(), Some(chunk_text));
            assert_eq!(h_mem.value["agent_id"], "writer");
            assert_eq!(h_mem.value["task"], "produce a long answer");
            assert_eq!(h_mem.value["model"], "answer-model");
        }
    }

    /// expect: Recall returns the matched response passage and producer provenance, never whole-turn JSON.
    #[tokio::test]
    async fn recall_returns_response_content_and_producer_provenance() {
        let memory = temp_memory();
        let inference = inference(EmbedMode::Exact);
        let response = "A response-only fact says the launch window is October.";
        let report = ingest_turn(
            &memory,
            &inference,
            "planner",
            "identify the launch window",
            response,
            "planner-model",
            None,
            Some("embedding-model"),
        )
        .await;
        assert_eq!(report.embedded, 1);

        let passages = recall_turns(
            &memory,
            &inference,
            "October launch",
            10,
            None,
            Some("embedding-model"),
        )
        .await
        .expect("recall succeeds")
        .passages;
        assert_eq!(passages.len(), 1);
        assert_eq!(passages[0].passage, response);
        assert_eq!(passages[0].agent_id, "planner");
        assert_eq!(passages[0].task, "identify the launch window");
        assert_eq!(passages[0].model, "planner-model");
        assert!(!passages[0].passage.starts_with('{'));
    }

    /// expect: A held-out future task ranks the relevant response passage and producer ahead of a competing passage.
    #[tokio::test]
    async fn future_task_ranks_expected_response_and_producer_first() {
        let memory = temp_memory();
        let inference = inference(EmbedMode::SemanticRanking);
        for (agent, task, response) in [
            (
                "navigator",
                "prepare a route decision",
                "The crimson compass marks the safe route through the northern pass.",
            ),
            (
                "caterer",
                "prepare a lunch decision",
                "The silver kettle is reserved for the afternoon tea service.",
            ),
        ] {
            let report = ingest_turn(
                &memory,
                &inference,
                agent,
                task,
                response,
                "test-model",
                None,
                Some("embedding-model"),
            )
            .await;
            assert_eq!(report.embedded, 1);
        }

        let passages = recall_turns(
            &memory,
            &inference,
            "Which prior answer helps with future navigation?",
            2,
            None,
            Some("embedding-model"),
        )
        .await
        .expect("recall succeeds")
        .passages;

        assert_eq!(passages.len(), 2, "fixture must retain a competing passage");
        assert_eq!(passages[0].agent_id, "navigator");
        assert_eq!(passages[0].task, "prepare a route decision");
        assert_eq!(passages[0].chunk_index, 0);
        assert!(passages[0].passage.contains("crimson compass"));
        assert!(passages[0].distance < passages[1].distance);
    }

    /// expect: Producing-agent filtering is exact while unscoped recall remains shared across agents.
    #[tokio::test]
    async fn recall_preserves_agent_filtering_and_shared_scope() {
        let memory = temp_memory();
        let inference = inference(EmbedMode::Exact);
        for (agent, response) in [
            ("alpha", "alpha response passage"),
            ("beta", "beta response passage"),
            ("alpha_fan", "alpha fan response passage"),
        ] {
            ingest_turn(
                &memory,
                &inference,
                agent,
                "shared task",
                response,
                "model",
                None,
                Some("embedding-model"),
            )
            .await;
        }

        let scoped = recall_turns(
            &memory,
            &inference,
            "response",
            10,
            Some("alpha"),
            Some("embedding-model"),
        )
        .await
        .expect("scoped recall succeeds")
        .passages;
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].agent_id, "alpha");

        let shared = recall_turns(
            &memory,
            &inference,
            "response",
            10,
            None,
            Some("embedding-model"),
        )
        .await
        .expect("shared recall succeeds")
        .passages;
        assert_eq!(shared.len(), 3);
    }

    /// expect: Missing embedding configuration is visible and stores no partial semantic index.
    #[tokio::test]
    async fn missing_embedding_model_is_reported_without_failing_ingest() {
        let memory = temp_memory();
        let report = ingest_turn(
            &memory,
            &inference(EmbedMode::Exact),
            "agent",
            "task",
            "response passage",
            "model",
            None,
            None,
        )
        .await;
        assert_eq!(report.attempted, 1);
        assert_eq!(report.stored, 1);
        assert_eq!(report.embedded, 0);
        assert_eq!(report.failed, 1);
        assert!(
            report
                .degradations
                .contains(&MemoryDegradation::EmbeddingModelUnconfigured)
        );
    }

    /// expect: An unavailable store returns exact failed accounting instead of failing the delegation path.
    #[tokio::test]
    async fn unavailable_store_is_reported_non_fatally() {
        let path =
            std::env::temp_dir().join(format!("kask-swarm-mem-test-{}.db", uuid::Uuid::new_v4()));
        let memory = LazyLocalMemory::lazy(
            path.to_string_lossy().to_string(),
            "short".to_string(),
            test_dim(),
        );
        let report = ingest_turn(
            &memory,
            &inference(EmbedMode::Exact),
            "agent",
            "task",
            "response passage",
            "model",
            None,
            Some("embedding-model"),
        )
        .await;
        assert_eq!(report.attempted, 1);
        assert_eq!(report.stored, 0);
        assert_eq!(report.embedded, 0);
        assert_eq!(report.failed, 1);
        assert!(matches!(
            report.degradations.as_slice(),
            [MemoryDegradation::StoreUnavailable(_)]
        ));
    }

    /// expect: A short embedding batch is rejected wholesale; chunks are never positionally mispaired.
    #[tokio::test]
    async fn embedding_count_mismatch_stores_zero_embeddings() {
        let memory = temp_memory();
        let report = ingest_turn(
            &memory,
            &inference(EmbedMode::OneShort),
            "agent",
            "task",
            &long_response(500),
            "model",
            None,
            Some("embedding-model"),
        )
        .await;
        assert_eq!(report.attempted, 2);
        assert_eq!(report.stored, 2);
        assert_eq!(report.embedded, 0);
        assert_eq!(report.failed, 2);
        assert!(
            report
                .degradations
                .contains(&MemoryDegradation::EmbeddingCountMismatch {
                    expected: 2,
                    actual: 1,
                })
        );
        let store = memory.get().await.expect("store opens");
        assert_eq!(store.embedding_count().expect("count succeeds"), 0);
    }

    /// expect: Atomic delegation metrics remain the same three task-level records and gain no embeddings.
    #[tokio::test]
    async fn record_delegation_atomic_records_are_unchanged() {
        let memory = temp_memory();
        record_delegation(&memory, "test_agent", 42, Some(true), "succeeded").await;
        let store = memory.get().await.expect("store opens");
        let h_mems = store
            .h_mems_by_entity_prefix("agent:test_agent")
            .expect("query succeeds");
        let attributes: std::collections::HashSet<&str> = h_mems
            .iter()
            .map(|h_mem| h_mem.attribute.as_str())
            .collect();
        assert_eq!(h_mems.len(), 3);
        assert_eq!(
            attributes,
            std::collections::HashSet::from([
                "delegation:latency_ms",
                "delegation:task_success",
                "delegation:response",
            ])
        );
        assert_eq!(store.embedding_count().expect("count succeeds"), 0);
    }
}
