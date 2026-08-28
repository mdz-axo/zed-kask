//! Local swarm knowledge tools — the kask-vernacular analogs of ABW's
//! `swarm_search_knowledge`, `swarm_generate_prompt`, and `swarm_generate_ontology`.
//!
//! Where ABW backs these with fermi's per-agent dreaming-memory KG + fermi's
//! LLM generation, the local analogs back them with the operator's own
//! `hkask-memory` `MemoryStore` (the knowledge graph — entity-attribute-value
//! triples, scoped per agent by an `agent:<agent_id>:` prefix) and the local
//! `InferencePort` (Ollama/cloud via the zed IPC bridge). No ABW round-trips.
//!
//! Design rationale: `kask/docs/plans/local-swarm-knowledge-tools.md`.
//!
//! Graceful degradation: `LazyLocalMemory::get` opens the
//! `MemoryStore` lazily. The SQLCipher passphrase is resolved from the
//! canonical chain (env → keychain → `kask://credentials/hkask_swarm_memory_passphrase`)
//! by `SwarmConfig::from_env`. If the passphrase is empty or too short,
//! `get` returns an error and the search tool returns an empty
//! result with a `memory_unconfigured` note (never a panic, never a fabricated
//! hit — the `.rules` unwrap_or(0) trap), and the generate tools proceed
//! unseeded (memory is an enhancement, not a dependency).

use hkask_memory::MemoryStore;
use hkask_storage::HMem;
use hkask_types::{HMemOntology, Visibility, WebID};
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
                "swarm memory passphrase too short ({} chars — need >=8; set \
                 HKASK_SWARM_MEMORY_PASSPHRASE). Local knowledge tools will degrade.",
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

// ── Episodic turn memory (the shared knowledgebase) ───────────────────────
//
// `record_delegation` above is the stigmergy trail — the ACO pheromone signals
// (latency, task-success, response) stored as separate triples under the
// agent's namespace root (`agent:<agent_id>`) for fitness assessment. The
// functions below are the episodic complement: the FULL turn (task + response
// + model) stored as one coherent h_mem per turn, plus an embedding of the
// task so the turn is retrievable by semantic similarity. There is ONE
// `swarm_memory.db` for all swarms and all agents — `search_similar` has no
// entity filter, so a turn any agent produced is retrievable by any other.
// That is the shared knowledgebase: swarms build on each other's experience.

/// Ingest a completed local-swarm delegation as an episodic h_mem into the
/// shared `swarm_memory.db`, plus an embedding of the task for semantic recall.
///
/// The turn is stored under a unique per-turn entity
/// (`agent:<agent_id>:turn:<uuid>`) so each turn is individually retrievable by
/// embedding KNN. The h_mem value is the full turn JSON (`agent_id`, `task`,
/// `response`, `model`), so recall returns the complete provenance — not just
/// the response (which is what the stigmergy trail already stores). The
/// embedding is stored under the same entity, so `search_similar` →
/// `query_deduped_untouched(entity_ref)` recovers the turn text.
///
/// Graceful degradation mirrors `record_delegation`: a failed store open,
/// h_mem write, or embedding is logged with `tracing::warn!` and never fails
/// the delegation (memory is an enhancement, not a dependency). A turn
/// stored without an embedding is still in the KB but not the KNN index —
/// entity-reachable, not similarity-reachable.
pub(crate) async fn ingest_turn(
    memory: &LazyLocalMemory,
    inference: &std::sync::Arc<dyn hkask_types::InferencePort>,
    agent_id: &str,
    task: &str,
    response: &str,
    model: &str,
) {
    let store = match memory.get().await {
        Ok(store) => store,
        Err(reason) => {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                error = %reason,
                agent = %agent_id,
                "episodic turn ingest skipped — swarm memory unavailable (non-fatal)"
            );
            return;
        }
    };
    let owner = WebID::for_agent_name("swarm_delegate_local");
    let turn_id = uuid::Uuid::new_v4();
    let entity = format!("{AGENT_PREFIX}{agent_id}:turn:{turn_id}");

    // Cap the response to prevent unbounded memory growth — mirrors the cap
    // in `record_delegation` and `AgentExecutor::run`'s tool-result handling.
    let capped_response: String = if response.len() > 64 * 1024 {
        response.chars().take(64 * 1024).collect()
    } else {
        response.to_string()
    };
    let turn_value = serde_json::json!({
        "agent_id": agent_id,
        "task": task,
        "response": capped_response,
        "model": model,
    });
    // Store the turn as a JSON *string* (not an object) so the recall path's
    // `h_mem.value.as_str()` recovers the full text — mirrors the bridge's
    // `RealMemoryPort::ingest_turn`, which stringifies the turn JSON for the
    // same reason.
    let turn_record = serde_json::Value::String(turn_value.to_string());

    // Process-axis anchoring (P5.4): a swarm delegation is a PKO step
    // execution of the delegate procedure, anchored to the agent so recall
    // can distinguish turns by producer.
    let ontology = HMemOntology::process("swarm_delegate", "turn", agent_id);
    let mut h_mem = HMem::new(&entity, "chatted", turn_record, owner).with_ontology(ontology);
    // Shared visibility so the turn is part of the shared knowledgebase —
    // recallable across all agents/swarms, not just the producing agent.
    h_mem.access.visibility = Visibility::Shared;
    if let Err(error) = store.store(h_mem) {
        tracing::warn!(
            target: "hkask.mcp.swarm",
            error = %error,
            agent = %agent_id,
            "episodic turn h_mem write failed (non-fatal)"
        );
        // An embedding without its h_mem is an orphan the recall path cannot
        // resolve, so do not store one if the turn itself did not land.
        return;
    }

    // Embed the task so the turn is retrievable by semantic similarity. The
    // embedding is stored under the same entity as the h_mem, so
    // `search_similar` → `query_deduped_untouched(entity_ref)` recovers the
    // full turn text. A failed embed degrades the turn to entity-only recall.
    let embedding_model = hkask_inference::model_constants::embedding_model();
    match inference.embed(&embedding_model, &[task.to_string()]).await {
        Ok(vectors) => match vectors.into_iter().next() {
            Some(vector) => {
                if let Err(error) = store.store_embedding(&entity, &vector, &embedding_model, None)
                {
                    tracing::warn!(
                        target: "hkask.mcp.swarm",
                        error = %error,
                        agent = %agent_id,
                        "episodic turn embedding store failed — turn is in the KB but not the KNN index (non-fatal)"
                    );
                }
            }
            None => {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    agent = %agent_id,
                    "embedding model returned no vector for the task — turn is in the KB but not the KNN index (non-fatal)"
                );
            }
        },
        Err(error) => {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                error = %error,
                agent = %agent_id,
                "episodic turn embedding failed — turn is in the KB but not the KNN index (non-fatal)"
            );
        }
    }
}

/// A prior swarm turn recalled from the shared knowledgebase by semantic
/// similarity. `text` is the full turn JSON stored by `ingest_turn`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecalledTurn {
    /// The producing agent's id (which agent ran the turn).
    pub agent_id: String,
    /// The full turn JSON (`agent_id`, `task`, `response`, `model`).
    pub text: String,
    /// Cosine distance to the query (lower = more similar).
    pub distance: f64,
}

/// Recall prior swarm turns from the shared `swarm_memory.db` by semantic
/// similarity to the query. `search_similar` has no entity filter, so this
/// spans ALL agents and ALL swarms — the shared knowledgebase. A turn any
/// agent produced is retrievable here.
///
/// Returns turns ranked by similarity (most similar first). Only episodic
/// turns carry embeddings (the stigmergy triples from `record_delegation`
/// have none), so every KNN hit resolves to a turn. Degrades to an error when
/// the store is unavailable or the query cannot be embedded — callers
/// surface a `memory_unconfigured` note rather than fabricating empty hits
/// (the `.rules` unwrap_or(0) trap: a failed recall is not "no memory").
pub(crate) async fn recall_turns(
    memory: &LazyLocalMemory,
    inference: &std::sync::Arc<dyn hkask_types::InferencePort>,
    query: &str,
    limit: usize,
) -> Result<Vec<RecalledTurn>, LocalSwarmError> {
    let store = memory.get().await?;
    let embedding_model = hkask_inference::model_constants::embedding_model();
    let vectors = inference
        .embed(&embedding_model, &[query.to_string()])
        .await
        .map_err(|error| {
            LocalSwarmError::Unavailable(format!("embedding the recall query failed: {error}"))
        })?;
    let query_vector = vectors.into_iter().next().ok_or_else(|| {
        LocalSwarmError::Unavailable(
            "embedding model returned no vector for the recall query".to_string(),
        )
    })?;
    let results = store
        .search_similar(&query_vector, limit)
        .map_err(|error| {
            LocalSwarmError::Database(format!("semantic search over swarm memory failed: {error}"))
        })?;
    let mut turns = Vec::with_capacity(results.len());
    for result in results {
        let entity_ref = result.embedding.entity_ref.clone();
        match store.query_deduped_untouched(&entity_ref) {
            Ok(h_mems) => {
                for h_mem in h_mems {
                    let text = h_mem.value.as_str().unwrap_or("").to_string();
                    if text.is_empty() {
                        continue;
                    }
                    // Recover the producing agent from the turn JSON so the
                    // caller knows which agent produced the recalled turn.
                    let agent_id = serde_json::from_str::<serde_json::Value>(&text)
                        .ok()
                        .and_then(|value| {
                            value
                                .get("agent_id")
                                .and_then(|agent| agent.as_str())
                                .map(String::from)
                        })
                        .unwrap_or_default();
                    turns.push(RecalledTurn {
                        agent_id,
                        text,
                        distance: result.distance,
                    });
                }
            }
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    error = %error,
                    entity_ref = %entity_ref,
                    "failed to resolve KNN hit to its turn h_mem — skipping (non-fatal)"
                );
            }
        }
    }
    Ok(turns)
}

/// A one-shot LLM generate over the local inference port.
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
    let result = inference
        .generate(prompt, &params, None)
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
    use std::sync::Arc;

    /// Embedding dimension used by the test stubs. Must match the store's
    /// `vec0` table dim, which `Database::open` creates from
    /// `hkask_storage::embedding_dim()` (the schema's `$DIM`). Reading the
    /// same resolver keeps the stub in sync with the store regardless of
    /// `HKASK_EMBEDDING_DIM`.
    fn test_dim() -> usize {
        hkask_storage::embedding_dim()
    }
    const TEST_PASSPHRASE: &str = "test-passphrase";

    /// A stub `InferencePort` whose `embed` returns a deterministic unit
    /// vector of length `dim`, so ingest and recall round-trip through the
    /// real sqlite-vec KNN index. `generate` is unused by the memory path.
    struct EmbedStubInference {
        dim: usize,
    }

    impl hkask_types::InferencePort for EmbedStubInference {
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
                    text: "stub".into(),
                    model: "stub-model".into(),
                    usage: hkask_types::InferenceUsage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    },
                    finish_reason: "stop".into(),
                    tool_calls: vec![],
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }

        fn embed<'a>(&'a self, _model: &str, texts: &[String]) -> hkask_types::EmbedFuture<'a> {
            // Capture only the count (owned, `Copy`) so the future borrows
            // nothing — the trait ties the returned future's lifetime to
            // `&self`, and `texts` carries an unrelated lifetime.
            let count = texts.len();
            let dim = self.dim;
            Box::pin(async move {
                Ok((0..count)
                    .map(|_| {
                        let mut vector = vec![0.0f32; dim];
                        if dim > 0 {
                            vector[0] = 1.0;
                        }
                        vector
                    })
                    .collect())
            })
        }
    }

    /// A `LazyLocalMemory` backed by a unique temp SQLCipher file. Each test
    /// gets its own DB so ingest/recall round-trips never collide. The files
    /// leak in the temp dir (the path is owned by `LazyLocalMemory`); this
    /// mirrors the production open path exactly, including sqlite-vec.
    fn temp_memory() -> LazyLocalMemory {
        let path =
            std::env::temp_dir().join(format!("kask-swarm-mem-test-{}.db", uuid::Uuid::new_v4()));
        LazyLocalMemory::lazy(
            path.to_string_lossy().to_string(),
            TEST_PASSPHRASE.to_string(),
            test_dim(),
        )
    }

    /// `ingest_turn` stores the full turn (task + response + model) as an h_mem
    /// AND an embedding of the task, so `recall_turns` retrieves it by
    /// semantic similarity. Pins the round-trip end-to-end through sqlite-vec.
    #[tokio::test]
    async fn ingest_turn_stores_full_turn_retrievable_by_recall() {
        let memory = temp_memory();
        let inference: Arc<dyn hkask_types::InferencePort> =
            Arc::new(EmbedStubInference { dim: test_dim() });
        ingest_turn(
            &memory,
            &inference,
            "market_analyst",
            "analyze the market",
            "the market is up",
            "test-model",
        )
        .await;

        let turns = recall_turns(&memory, &inference, "market", 10)
            .await
            .expect("recall succeeds on a configured store");
        assert_eq!(turns.len(), 1, "the ingested turn is the only KNN hit");
        assert_eq!(turns[0].agent_id, "market_analyst");
        let parsed: serde_json::Value =
            serde_json::from_str(&turns[0].text).expect("turn text is the turn JSON");
        assert_eq!(parsed["agent_id"], "market_analyst");
        assert_eq!(parsed["task"], "analyze the market");
        assert_eq!(parsed["response"], "the market is up");
        assert_eq!(parsed["model"], "test-model");
    }

    /// The knowledgebase is SHARED: `search_similar` has no agent filter, so a
    /// turn one agent produced is retrievable by any other. This pins that
    /// cross-agent/cross-swarm property — the whole point of one DB for all
    /// swarms.
    #[tokio::test]
    async fn recall_spans_all_agents_shared_knowledgebase() {
        let memory = temp_memory();
        let inference: Arc<dyn hkask_types::InferencePort> =
            Arc::new(EmbedStubInference { dim: test_dim() });
        // A turn produced by `agent_alpha`.
        ingest_turn(
            &memory,
            &inference,
            "agent_alpha",
            "shared research task",
            "shared finding",
            "m",
        )
        .await;

        // `recall_turns` takes no agent argument — it spans the whole KB. A
        // turn `agent_alpha` produced is retrievable here ("by" any agent).
        let turns = recall_turns(&memory, &inference, "anything", 10)
            .await
            .expect("recall succeeds");
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].agent_id, "agent_alpha");
    }

    /// `ingest_turn` degrades gracefully when the store cannot be opened (short
    /// passphrase): it must not panic and must write nothing. `recall_turns`
    /// surfaces the unavailability as an error (callers show a
    /// `memory_unconfigured` note, never fabricated empty hits — the `.rules`
    /// unwrap_or(0) trap).
    #[tokio::test]
    async fn ingest_turn_skips_and_recall_errors_when_store_unavailable() {
        let path =
            std::env::temp_dir().join(format!("kask-swarm-mem-test-{}.db", uuid::Uuid::new_v4()));
        // A passphrase shorter than 8 chars makes `get` error every
        // time, so the store never opens.
        let memory = LazyLocalMemory::lazy(
            path.to_string_lossy().to_string(),
            "short".to_string(),
            test_dim(),
        );
        let inference: Arc<dyn hkask_types::InferencePort> =
            Arc::new(EmbedStubInference { dim: test_dim() });

        // Must not panic and must not fail the call (memory is non-fatal).
        ingest_turn(&memory, &inference, "agent", "task", "response", "m").await;

        let recall_error = recall_turns(&memory, &inference, "q", 10).await;
        assert!(
            recall_error.is_err(),
            "recall surfaces unavailability, not empty hits"
        );
    }

    /// Multiple turns from multiple agents accumulate in the shared KB and are
    /// all retrievable — the knowledgebase grows across delegations.
    #[tokio::test]
    async fn multiple_turns_accumulate_in_shared_knowledgebase() {
        let memory = temp_memory();
        let inference: Arc<dyn hkask_types::InferencePort> =
            Arc::new(EmbedStubInference { dim: test_dim() });
        ingest_turn(
            &memory,
            &inference,
            "agent_a",
            "task one",
            "response one",
            "m",
        )
        .await;
        ingest_turn(
            &memory,
            &inference,
            "agent_b",
            "task two",
            "response two",
            "m",
        )
        .await;
        ingest_turn(
            &memory,
            &inference,
            "agent_a",
            "task three",
            "response three",
            "m",
        )
        .await;

        let turns = recall_turns(&memory, &inference, "query", 50)
            .await
            .expect("recall succeeds");
        assert_eq!(
            turns.len(),
            3,
            "all three turns accumulated and are retrievable"
        );
        let agent_ids: std::collections::HashSet<String> =
            turns.into_iter().map(|turn| turn.agent_id).collect();
        assert!(agent_ids.contains("agent_a"));
        assert!(agent_ids.contains("agent_b"));
    }

    /// `record_delegation` writes stigmergy annotations (latency, task_success,
    /// response) to the agent's entity prefix. This test pins that the
    /// stigmergy path produces retrievable h_mems — the parallel fan-out path
    /// now calls `record_delegation` alongside `ingest_turn`, and this test
    /// verifies the stigmergy write is not a no-op.
    #[tokio::test]
    async fn record_delegation_writes_stigmergy_annotations() {
        let memory = temp_memory();
        let inference: Arc<dyn hkask_types::InferencePort> =
            Arc::new(EmbedStubInference { dim: test_dim() });

        record_delegation(
            &memory,
            "test_agent",
            42,
            Some(true),
            "the agent succeeded",
        )
        .await;

        // The stigmergy annotations are stored under the agent's entity prefix.
        // We verify by recalling — the embedding stub returns a unit vector,
        // so recall returns all entries (KNN with dim>0 matches everything).
        // Instead, query the store directly for the agent's entity.
        let store = memory.get().await.expect("store opens");
        let h_mems = store
            .h_mems_by_entity_prefix("agent:test_agent")
            .expect("query succeeds");
        assert!(
            !h_mems.is_empty(),
            "record_delegation must write stigmergy h_mems under the agent prefix"
        );
        // Verify the latency annotation is present.
        let has_latency = h_mems
            .iter()
            .any(|h| h.attribute == "delegation:latency_ms");
        assert!(
            has_latency,
            "record_delegation must write the latency annotation"
        );
        // Verify the task_success annotation is present (only when a verdict
        // was supplied).
        let has_success = h_mems
            .iter()
            .any(|h| h.attribute == "delegation:task_success");
        assert!(
            has_success,
            "record_delegation must write the task_success annotation when a verdict is supplied"
        );
    }

    /// `ingest_turn` + `record_delegation` together produce both episodic
    /// turn memory (retrievable by semantic recall) and stigmergy annotations
    /// (retrievable by entity prefix). This pins that the parallel fan-out
    /// path's side effects are not silent no-ops — both writes land in the
    /// shared KB.
    #[tokio::test]
    async fn ingest_turn_and_record_delegation_both_write_to_shared_kb() {
        let memory = temp_memory();
        let inference: Arc<dyn hkask_types::InferencePort> =
            Arc::new(EmbedStubInference { dim: test_dim() });

        // Simulate what the parallel fan-out path does after delegate_batch
        // returns a successful result.
        ingest_turn(
            &memory,
            &inference,
            "parallel_agent",
            "analyze market trends",
            "market is bullish",
            "test-model",
        )
        .await;
        record_delegation(
            &memory,
            "parallel_agent",
            100,
            None, // no evaluator in fan-out
            "market is bullish",
        )
        .await;

        // Episodic turn memory: retrievable by semantic recall.
        let turns = recall_turns(&memory, &inference, "market", 10)
            .await
            .expect("recall succeeds");
        assert_eq!(turns.len(), 1, "ingest_turn wrote a retrievable turn");
        assert_eq!(turns[0].agent_id, "parallel_agent");

        // Stigmergy: retrievable by entity prefix.
        let store = memory.get().await.expect("store opens");
        let h_mems = store
            .h_mems_by_entity_prefix("agent:parallel_agent")
            .expect("query succeeds");
        assert!(
            !h_mems.is_empty(),
            "record_delegation wrote stigmergy annotations"
        );
    }
}
