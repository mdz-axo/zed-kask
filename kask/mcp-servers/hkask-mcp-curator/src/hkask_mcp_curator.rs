#![cfg_attr(not(test), forbid(unsafe_code))]
// Test builds allow `unsafe` for the one-shot `env::set_var` in the
// tool-behavior harness (HKASK_EMBEDDING_MODEL — the semantic paths
// resolve it as an Option since the model_constants refactor). Production
// still forbids unsafe outright. Same pattern as hkask-mcp-media,
// hkask-email, hkask-inference, hkask-keystore.
#![warn(clippy::let_underscore_future)]
// `tokio` is in [dependencies] for the bin target's `#[tokio::main]`; the lib
// itself does not use it, so the unused_crate_dependencies lint fires on the
// lib target. This is the legitimate bin-needs-dep case.
#![allow(unused_crate_dependencies)]
//! hkask-mcp-curator — Curator MCP server library.
//!
//! Exposes Regulation history, memory search, and per-store liveness as MCP tools. Live metacognition health belongs
//! to the single built-in `curator_status` AgentTool; do not mirror it
//! in this child process or reconstruct its reading from event history.

pub(crate) mod backfill;
pub(crate) mod db;
pub(crate) mod distillation;
pub(crate) mod federated;
pub(crate) mod forgetting;
pub(crate) mod ontology;
pub(crate) mod thread_turns;
pub(crate) mod trust_ledger;
pub mod types;

pub use db::{CuratorDb, CuratorStores};

use hkask_mcp_server::server::{
    McpToolError, execute_tool, map_infra_error, map_memory_store_error,
};
use hkask_types::regulation::RegulationSpan;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use serde_json::json;
use std::sync::Arc;

use types::*;

const SERVER_NAME: &str = "hkask-mcp-curator";

/// Cap on fragments one entity may contribute to semantic recall. A thread
/// entity holds one h_mem per turn; without a cap a single chatty thread
/// floods the whole result set and every other entity vanishes from recall.
const MAX_FRAGMENTS_PER_ENTITY: usize = 2;

/// A semantic-recall failure, structured by kind. Callers fall back to
/// exact-entity lookup and surface the Display message in the result's
/// `note` so the operator can tell "semantic recall broken" from "no
/// matching memories" (the `unwrap_or(0)` trap).
#[derive(Debug, thiserror::Error)]
enum SemanticRecallError {
    #[error("curator memory unavailable: {source}")]
    MemoryUnavailable {
        #[source]
        source: McpToolError,
    },
    #[error("embedding the recall query failed: {source}")]
    Embed {
        #[source]
        source: hkask_types::EmbeddingGenerationError,
    },
    #[error("embedding model returned no vector for the recall query")]
    NoVector,
    #[error("semantic search over curator memory failed: {source}")]
    Search {
        #[source]
        source: hkask_memory::MemoryStoreError,
    },
    #[error(
        "no embedding model configured — set kask.models.embedding_model \\
             (injected as HKASK_EMBEDDING_MODEL); kask never falls back to a \\
             hidden code constant"
    )]
    EmbeddingNotConfigured,
}

/// Semantic-recall outcome: the resolved `(h_mem, distance)` fragments
/// plus the number of KNN hits that failed to resolve to a matching h_mem —
/// the store errored, or the hit's entity or named passage is orphaned.
/// Zero fragments with failures > 0 means recall degraded, not an empty
/// store; callers surface the failure count so the two cannot be confused.
struct SemanticRecall {
    fragments: Vec<(hkask_storage::HMem, f64)>,
    resolution_failures: usize,
    /// KNN hits excluded because their stored model matches neither the
    /// query's requested nor its provider-confirmed actual model — surfaced
    /// in tool responses so an embedding-model migration window reads as
    /// degradation, never as an empty store.
    excluded_model_mismatch: usize,
}

fn degradation_note(
    resolution_failures: usize,
    excluded_model_mismatch: usize,
    yielded: usize,
) -> Option<String> {
    let mut notes = Vec::new();
    if resolution_failures > 0 {
        notes.push(resolution_failure_note(resolution_failures, yielded));
    }
    if excluded_model_mismatch > 0 {
        notes.push(model_mismatch_note(excluded_model_mismatch));
    }
    (!notes.is_empty()).then(|| notes.join("; "))
}

/// Parse an `HMemId` UUID and fetch the existing row by ID (`invalid_argument`
/// on malformed UUID, `not_found` when the row is absent).
fn fetch_existing_h_mem(
    memory: &hkask_memory::MemoryStore,
    field_name: &str,
    raw_id: &str,
    not_found_message: String,
    fetch_error_ctx: &str,
) -> Result<(hkask_storage::HMemId, hkask_storage::HMem), McpToolError> {
    let id = raw_id.parse::<hkask_storage::HMemId>().map_err(|e| {
        McpToolError::invalid_argument(format!("Invalid {field_name} '{raw_id}': {e}"))
    })?;
    let h_mem = memory
        .get_by_id(&id)
        .map_err(|e| map_memory_store_error(e, fetch_error_ctx))?
        .ok_or_else(|| McpToolError::not_found(not_found_message))?;
    Ok((id, h_mem))
}

/// Note naming the KNN hits that failed h_mem resolution, shared by every
/// semantic call site so the wording stays identical. `yielded` is the
/// fragment count that DID resolve; when it is zero the note must say the
/// empty result is a resolution failure, not an empty store.
fn resolution_failure_note(failures: usize, yielded: usize) -> String {
    if yielded == 0 {
        format!("{failures} semantic hits failed h_mem resolution — recall degraded, not empty")
    } else {
        format!("{failures} semantic hits failed h_mem resolution — recall degraded")
    }
}

fn model_mismatch_note(excluded: usize) -> String {
    format!(
        "{excluded} stored embeddings were excluded from KNN because they were embedded \
         under a different model — re-embed pending, not an empty store"
    )
}

/// Serialize one h_mem as its recall wire fragment — the six fields every
/// recall surface shares. Site extras (distance, valid_from, ontology) are
/// appended by the caller; each is last in its literal, so insertion keeps
/// the field order (serde_json preserve_order).
fn h_mem_fragment(t: &hkask_storage::HMem) -> serde_json::Value {
    json!({
        "id": t.id.to_string(),
        "entity": t.entity, "attribute": t.attribute,
        "value": t.value, "confidence": t.confidence,
        "provenance": t.provenance.as_str(),
    })
}

/// A semantic hit: the h_mem fragment plus its KNN distance.
fn h_mem_with_distance(t: &hkask_storage::HMem, distance: &f64) -> serde_json::Value {
    let mut fragment = h_mem_fragment(t);
    fragment["distance"] = json!(distance);
    fragment
}

/// One exact-entity recall leg: open the memory store, run `query`, and
/// return the leg as one JSON object — hits under count/h_mems (`take`
/// capping the serialized hits; `valid_from` adding the perspective-scoped
/// observed-at field), the query error as text, or the unavailable status
/// when the store cannot open. `fallback_reason` names the semantic-recall
/// degradation so the exact fallback reads as degraded, not as matches.
fn exact_recall_leg(
    memory: Result<&Arc<hkask_memory::MemoryStore>, McpToolError>,
    query: impl FnOnce(
        &hkask_memory::MemoryStore,
    ) -> Result<Vec<hkask_storage::HMem>, hkask_memory::MemoryStoreError>,
    take: Option<usize>,
    valid_from: bool,
    fallback_reason: Option<&str>,
) -> serde_json::Value {
    let memory = match memory {
        Ok(memory) => memory,
        Err(_) => return json!({"status": "unavailable"}),
    };
    let h_mems = match query(memory) {
        Ok(h_mems) => h_mems,
        Err(e) => return json!({"error": format!("{e}")}),
    };
    let hits = match take {
        Some(limit) => h_mems.iter().take(limit).collect::<Vec<_>>(),
        None => h_mems.iter().collect::<Vec<_>>(),
    };
    let serialized: Vec<serde_json::Value> = hits
        .into_iter()
        .map(|t| {
            let mut fragment = h_mem_fragment(t);
            if valid_from {
                fragment["valid_from"] = json!(t.observed_at.to_rfc3339());
            }
            fragment
        })
        .collect();
    let mut leg = json!({});
    leg["count"] = json!(serialized.len());
    if let Some(reason) = fallback_reason {
        leg["mode"] = json!("entity_exact");
        leg["note"] = json!(format!(
            "semantic recall unavailable — fell back to exact-entity lookup: {reason}"
        ));
    }
    leg["h_mems"] = json!(serialized);
    leg
}

/// Strip the derived keys the insert path adds (`recall_text`, `_note`)
/// before comparing a new value against a stored one — the write-path
/// contradiction check compares payloads, not storage decorations.
fn comparable_payload(value: &serde_json::Value) -> serde_json::Value {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("recall_text");
        object.remove("_note");
    }
    value
}

hkask_mcp_server::mcp_server!(
    pub struct CuratorServer {
        /// Self-healing handle over the curator's sovereign `curator.db`. All
        /// four stores are read through `db.get()` on every tool call so a
        /// mid-process heal takes effect without a server restart.
        db: Arc<CuratorDb>,
        /// Inference port for semantic memory recall — embeds recall queries
        /// through the zed IPC bridge (`HKASK_INFERENCE_SOCKET`), the same
        /// routing every other kask MCP server uses. Without it, the
        /// "semantic" tools degrade to exact-entity lookup, which never
        /// matches a natural-language question.
        inference_port: Arc<dyn hkask_types::InferencePort>,
    } = crate::ontology::annotate;
);

#[tool_router(server_handler)]
impl CuratorServer {
    // ── Liveness ───────────────────────────────────────────────────────

    #[tool(description = "Liveness check")]
    pub async fn curator_ping(
        &self,
        Parameters(_req): Parameters<PingRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_ping", async {
            let stores = self.db.get();
            Ok(json!({
                "status": "ok",
                "server": SERVER_NAME,
                "curator_webid": self.webid.to_string(),
                "stores": {

                    "regulation_store": stores.regulation_store.is_some(),
                    "memory": stores.memory.is_some(),
                }
            }))
        })
        .await
    }

    // ── Memory & Learning ──────────────────────────────────────────────

    /// Embed a recall query and resolve the nearest stored h_mems by cosine
    /// similarity. Returns the resolved `(h_mem, distance)` pairs (most
    /// similar first) plus the count of KNN hits that failed to resolve to
    /// any matching h_mem — the store errored, or its entity/passage is orphaned.
    /// Named passage vectors resolve only matching canonical passages; entity-level
    /// vectors retain the freshest-row behavior. Each entity contributes at most
    /// `MAX_FRAGMENTS_PER_ENTITY` fragments, with no duplicated h_mem IDs.
    /// `Err(reason)` when the query cannot be embedded (no IPC bridge, no
    /// embedding provider) or the store has no embedding index — callers fall
    /// back to exact-entity lookup and surface the reason. The query's model
    /// identities gate the KNN: rows stored under a different embedding
    /// model are excluded and counted in `excluded_model_mismatch` (the
    /// migration-window degradation callers surface).
    fn semantic_recall_fragments_for_vector(
        &self,
        query_vector: &[f32],
        limit: usize,
        query_model: &str,
        query_actual_model: Option<&str>,
    ) -> Result<SemanticRecall, SemanticRecallError> {
        let stores = self.db.get();
        let memory = stores
            .memory()
            .map_err(|source| SemanticRecallError::MemoryUnavailable { source })?;
        let knn_limit = limit.saturating_mul(MAX_FRAGMENTS_PER_ENTITY).max(limit);
        let outcome = memory
            .search_similar(query_vector, knn_limit, query_model, query_actual_model)
            .map_err(|source| SemanticRecallError::Search { source })?;
        let results = outcome.results;
        let mut fragments = Vec::with_capacity(results.len());
        let mut resolution_failures = 0usize;
        let mut seen_h_mem_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut per_entity_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for result in results {
            let entity_ref = result.embedding.entity_ref.clone();
            if per_entity_counts.get(&entity_ref).copied().unwrap_or(0) >= MAX_FRAGMENTS_PER_ENTITY
            {
                continue;
            }
            match memory.query_deduped_untouched(&entity_ref) {
                Ok(mut h_mems) => {
                    if let Some(passage) = result.embedding.passage_text.as_deref() {
                        h_mems.retain(|h_mem| {
                            hkask_memory::semantic_passage_for_h_mem(h_mem).as_deref()
                                == Some(passage)
                        });
                    }
                    if h_mems.is_empty() {
                        // A missing entity or named passage is a resolution failure,
                        // not permission to substitute an unrelated sibling.
                        resolution_failures += 1;
                        tracing::warn!(
                            target: "hkask.mcp.curator",
                            entity_ref = %entity_ref,
                            "KNN hit resolved to zero matching h_mems — orphaned entity or passage (non-fatal)"
                        );
                        continue;
                    }
                    h_mems.sort_by_key(|h_mem| std::cmp::Reverse(h_mem.observed_at));
                    for h_mem in h_mems {
                        if per_entity_counts.get(&entity_ref).copied().unwrap_or(0)
                            >= MAX_FRAGMENTS_PER_ENTITY
                        {
                            break;
                        }
                        if !seen_h_mem_ids.insert(h_mem.id.to_string()) {
                            continue;
                        }
                        per_entity_counts
                            .entry(entity_ref.clone())
                            .and_modify(|count| *count += 1)
                            .or_insert(1);
                        fragments.push((h_mem, result.distance));
                    }
                }
                Err(error) => {
                    resolution_failures += 1;
                    tracing::warn!(
                        target: "hkask.mcp.curator",
                        error = %error,
                        entity_ref = %entity_ref,
                        "failed to resolve KNN hit to its h_mem — skipping (non-fatal)"
                    );
                }
            }
        }
        fragments.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(SemanticRecall {
            fragments,
            resolution_failures,
            excluded_model_mismatch: outcome.excluded_model_mismatch,
        })
    }

    async fn semantic_recall_fragments(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<SemanticRecall, SemanticRecallError> {
        let embedding_model =
            curator_embedding_model().ok_or(SemanticRecallError::EmbeddingNotConfigured)?;
        let batch = self
            .inference_port
            .embed_with_dimensions(
                &embedding_model,
                &[query.to_string()],
                Some(hkask_storage::embedding_dim() as u32),
            )
            .await
            .map_err(|source| SemanticRecallError::Embed { source })?;
        let query_vector = batch
            .vectors
            .into_iter()
            .next()
            .ok_or(SemanticRecallError::NoVector)?;
        // S-01 completion: the embedding call's measured usage flows to the
        // executing tool's envelope (the consult/search retrieval family).
        hkask_mcp_server::server::record_tool_usage(batch.usage.clone(), batch.cost_usd);
        self.semantic_recall_fragments_for_vector(
            &query_vector,
            limit,
            &batch.requested_model,
            batch.actual_model.as_deref(),
        )
    }

    #[tool(
        description = "Query the Curator's memory by semantic similarity to a free-text query (a question or topic). Embeds the query and returns the nearest stored memories by cosine similarity. Falls back to exact-entity-name lookup when embeddings are unavailable (noted in the output)."
    )]
    pub async fn curator_semantic_search(
        &self,
        Parameters(req): Parameters<SemanticSearchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_semantic_search", async {
            let limit = req.limit.unwrap_or(10).clamp(1, 50);
            let stores = self.db.get();
            let memory = stores.memory()?;

            // Semantic leg: embed the query, KNN over stored embeddings,
            // resolve each hit to its h_mem. This is the path a natural-
            // language question actually matches — the exact-entity leg
            // below only matches when the query IS an entity name.
            match self.semantic_recall_fragments(&req.query, limit).await {
                Ok(recall) => {
                    let serialized: Vec<serde_json::Value> = recall
                        .fragments
                        .iter()
                        .take(limit)
                        .map(|(t, distance)| h_mem_with_distance(t, distance))
                        .collect();
                    let yielded = serialized.len();
                    let mut result = json!({
                        "count": yielded,
                        "mode": "semantic",
                        "results": serialized,
                    });
                    if recall.excluded_model_mismatch > 0 {
                        result["model_mismatch_excluded"] =
                            json!(recall.excluded_model_mismatch);
                    }
                    if let Some(note) = degradation_note(
                        recall.resolution_failures,
                        recall.excluded_model_mismatch,
                        yielded,
                    ) {
                        result["note"] = json!(note);
                    }
                    Ok(result)
                }
                // Degradation, not a silent fallback: the operator must be
                // able to tell "no similar memories" from "semantic recall
                // unavailable" (the unwrap_or(0) trap).
                Err(reason) => {
                    let exact = memory.query_deduped(&req.query).map_err(|e| match e {
                        hkask_memory::MemoryStoreError::HMem(
                            hkask_storage::HMemError::Infra(ref infra),
                        )
                        | hkask_memory::MemoryStoreError::Embedding(
                            hkask_storage::EmbeddingError::Infrastructure(ref infra),
                        ) => map_infra_error(infra, "Semantic recall failed"),
                        other => McpToolError::internal(format!("Semantic recall failed: {other}")),
                    })?;
                    let serialized: Vec<serde_json::Value> = exact
                        .iter()
                        .take(limit)
                        .map(|t| h_mem_fragment(t))
                        .collect();
                    Ok(json!({
                        "count": serialized.len(),
                        "mode": "entity_exact",
                        "note": format!("semantic recall unavailable — fell back to exact-entity lookup: {reason}"),
                        "results": serialized,
                    }))
                }
            }
        })
        .await
    }

    #[tool(
        description = "Search Curator memory and configured sealed corpus sources in one source-aware retrieval. Stores remain separate. Results preserve source and record provenance, corpus rows project passage_text only, and every source reports ready, unconfigured, incompatible, invalid, or unavailable status."
    )]
    pub async fn curator_federated_search(
        &self,
        Parameters(req): Parameters<FederatedSearchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_federated_search", async {
            if req.query.trim().is_empty() {
                return Err(McpToolError::invalid_argument("query must not be empty"));
            }
            let limit = req.limit.unwrap_or(10).clamp(1, 50);
            let embedding_model = curator_embedding_model().ok_or_else(|| {
                McpToolError::permission_denied(
                    "no embedding model configured — set kask.models.embedding_model \
                     (injected as HKASK_EMBEDDING_MODEL); kask never falls back to a \
                     hidden code constant",
                )
            })?;
            let batch = self
                .inference_port
                .embed_with_dimensions(
                    &embedding_model,
                    std::slice::from_ref(&req.query),
                    Some(hkask_storage::embedding_dim() as u32),
                )
                .await
                .map_err(|error| {
                    McpToolError::unavailable(format!("Federated query embedding failed: {error}"))
                })?;
            let query_vector = batch.vectors.into_iter().next().ok_or_else(|| {
                McpToolError::unavailable("Federated query embedding returned no vector")
            })?;
            // S-01 completion: the federated embedding call's measured
            // usage flows to the executing tool's envelope.
            hkask_mcp_server::server::record_tool_usage(batch.usage.clone(), batch.cost_usd);

            let mut statuses = Vec::new();
            let mut batches = Vec::new();
            match self.semantic_recall_fragments_for_vector(
                &query_vector,
                limit,
                &batch.requested_model,
                batch.actual_model.as_deref(),
            ) {
                Ok(recall) => {
                    let hits = recall
                        .fragments
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, (h_mem, distance))| {
                            hkask_memory::semantic_passage_for_h_mem(&h_mem).map(|text| {
                                hkask_memory::FederatedHit {
                                    source_id: "curator".to_string(),
                                    source_kind: hkask_memory::FederatedSourceKind::Curator,
                                    record_id: h_mem.id.to_string(),
                                    entity_ref: h_mem.entity,
                                    text,
                                    run_id: None,
                                    model: embedding_model.clone(),
                                    confidence: Some(h_mem.confidence.value()),
                                    distance,
                                    source_rank: index + 1,
                                    fused_rank: 0,
                                }
                            })
                        })
                        .collect::<Vec<_>>();
                    statuses.push(federated::FederatedSourceStatus {
                        source_id: "curator".to_string(),
                        source_kind: "curator",
                        state: federated::FederatedSourceState::Ready,
                        // Hits that failed h_mem resolution — or rows the model
                        // gate excluded — must not vanish behind a clean Ready:
                        // the counts ride the reason so Ready + result_count 0
                        // cannot read as "the curator store is empty".
                        reason: degradation_note(
                            recall.resolution_failures,
                            recall.excluded_model_mismatch,
                            hits.len(),
                        ),
                        result_count: hits.len(),
                    });
                    batches.push(hkask_memory::RankedSourceBatch {
                        source_id: "curator".to_string(),
                        hits,
                    });
                }
                Err(error) => statuses.push(federated::FederatedSourceStatus {
                    source_id: "curator".to_string(),
                    source_kind: "curator",
                    state: federated::FederatedSourceState::Unavailable,
                    reason: Some(error.to_string()),
                    result_count: 0,
                }),
            }

            let registry = self.db.federated_sources();
            let mut external_statuses = registry.statuses();
            for source in registry.sources() {
                match source.search(&embedding_model, &req.query, &query_vector, limit) {
                    Ok(batch) => {
                        if let Some(status) = external_statuses
                            .iter_mut()
                            .find(|status| status.source_id == batch.source_id)
                        {
                            status.result_count = batch.hits.len();
                            if batch.excluded_model_mismatch > 0 {
                                status.reason =
                                    Some(model_mismatch_note(batch.excluded_model_mismatch));
                            }
                        }
                        let hits = batch
                            .hits
                            .into_iter()
                            .map(|hit| hkask_memory::FederatedHit {
                                source_id: hit.source_id,
                                source_kind: hkask_memory::FederatedSourceKind::Corpus,
                                record_id: hit.embedding_id,
                                entity_ref: hit.entity_ref,
                                text: hit.text,
                                run_id: Some(hit.run_id),
                                model: hit.model,
                                confidence: None,
                                distance: hit.distance,
                                source_rank: hit.source_rank,
                                fused_rank: 0,
                            })
                            .collect();
                        batches.push(hkask_memory::RankedSourceBatch {
                            source_id: source.identity().source_id.clone(),
                            hits,
                        });
                    }
                    Err(error) => {
                        if let Some(status) = external_statuses
                            .iter_mut()
                            .find(|status| status.source_id == source.identity().source_id)
                        {
                            status.state = federated::classify_error(&error);
                            status.reason = Some(error.to_string());
                            status.result_count = 0;
                        }
                    }
                }
            }
            if registry.changed_during_search() {
                batches.retain(|batch| batch.source_id == "curator");
                for status in &mut external_statuses {
                    status.state = federated::FederatedSourceState::Unavailable;
                    status.reason =
                        Some("source identity changed during retrieval; retry search".to_string());
                    status.result_count = 0;
                }
            }
            statuses.extend(external_statuses);
            let results = hkask_memory::interleave_ranked_batches(batches, limit);
            Ok(json!({
                "query": req.query,
                "count": results.len(),
                "results": results,
                "sources": statuses,
            }))
        })
        .await
    }

    #[tool(
        description = "Recall the Curator's memory about an entity. Set `recall_shape` to `perspective_scoped` (curator's own turns) or `entity_wide` (all h_mems for the entity) or `both`. Set `ontology_axis` (dc_type | dc_subject | pko_procedure | ontology_namespace) plus `ontology_value` to recall along the dual-axis ontology instead of the entity — e.g. every step of procedure X, or every h_mem tagged by a domain ontology namespace."
    )]
    pub async fn curator_memory_recall(
        &self,
        Parameters(req): Parameters<MemoryRecallRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_memory_recall", async {
            let recall_shape = req.recall_shape.clone();
            let stores = self.db.get();

            // Ontology-axis recall (P5.4): when an axis is named, recall along
            // the dual-axis anchoring instead of the entity. This is what makes
            // the ontology blob a query axis rather than inert metadata —
            // "every step of procedure X" and "every bibo:Article" are
            // questions the entity index cannot answer.
            if let Some(axis) = req.ontology_axis.as_deref() {
                let Some(value) = req.ontology_value.as_deref() else {
                    return Err(McpToolError::invalid_argument(
                        "ontology_axis requires ontology_value",
                    ));
                };
                let memory = stores.memory()?;
                let h_mems = match axis {
                    "dc_type" => memory.query_by_dc_type(value),
                    "dc_subject" => memory.query_by_dc_subject(value),
                    "pko_procedure" => memory.query_by_pko_procedure(value),
                    "ontology_namespace" => memory.query_by_ontology_namespace(value),
                    other => {
                        return Err(McpToolError::invalid_argument(format!(
                            "unknown ontology_axis '{other}' — expected 'dc_type', \
                             'dc_subject', 'pko_procedure', or 'ontology_namespace'"
                        )));
                    }
                }
                .map_err(|e| map_memory_store_error(e, "Ontology recall failed"))?;
                let serialized: Vec<serde_json::Value> = h_mems
                    .iter()
                    .map(|t| {
                        let mut fragment = h_mem_fragment(t);
                        fragment["ontology"] = json!(t.ontology);
                        fragment
                    })
                    .collect();
                return Ok(json!({
                    "ontology_axis": axis,
                    "ontology_value": value,
                    "count": serialized.len(),
                    "h_mems": serialized,
                }));
            }

            let mut result = json!({});

            if recall_shape == MemoryRecallType::PerspectiveScoped
                || recall_shape == MemoryRecallType::Both
            {
                result["perspective_scoped"] = exact_recall_leg(
                    stores.memory(),
                    |memory| memory.query_for_deduped(&req.entity, self.webid),
                    None,
                    true,
                    None,
                );
            }
            if recall_shape == MemoryRecallType::EntityWide
                || recall_shape == MemoryRecallType::Both
            {
                result["entity_wide"] = exact_recall_leg(
                    stores.memory(),
                    |memory| memory.query_deduped(&req.entity),
                    None,
                    false,
                    None,
                );
            }
            Ok(result)
        })
        .await
    }

    // ── Consultation (Slice 8 — curator-as-callable-tool) ────────────────

    /// Consult the curator's memory with a question. A delegated agent calls
    /// this to get the curator's perspective on a topic, grounded in the
    /// curator's sovereign memory.
    ///
    /// This is a memory-grounded consultation, not a full curator agent
    /// turn — it returns the raw memory fragments matching the query by
    /// semantic similarity (the query is embedded through the zed IPC
    /// bridge, the same inference routing every other kask MCP server
    /// uses). The calling agent synthesizes the response from the
    /// fragments. When embeddings are unavailable, both scopes degrade to
    /// exact-entity lookup with the reason surfaced in the output.
    ///
    /// A full inference-grounded response (where the curator agent itself
    /// synthesizes) requires the in-process `CuratorAgentServer`, which
    /// lives in the zed process, not in this MCP server. That path is a
    /// future enhancement (requires a new IPC method + recursion cap).
    #[tool(
        description = "Consult the curator's memory with a question. Returns perspective-scoped and entity-wide memory fragments matching the query by semantic similarity. Memory-grounded consultation, not a full curator agent turn."
    )]
    pub async fn curator_consult(
        &self,
        Parameters(req): Parameters<CuratorConsultRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_consult", async {
            let limit = req.limit.unwrap_or(5).clamp(1, 20);
            let stores = self.db.get();
            let mut result = json!({
                "query": req.query,
                "note": "Memory-grounded consultation — raw fragments, not a synthesized response. The calling agent synthesizes."
            });

            // Semantic leg shared by both scopes: embed the query once, KNN
            // over stored embeddings, resolve each hit to its h_mem. A
            // natural-language question matches here — the previous
            // implementation did exact-entity lookup on the raw question
            // text, which never matched anything and made every consult
            // return zero fragments.
            let semantic = self.semantic_recall_fragments(&req.query, limit).await;
            match &semantic {
                Ok(recall) if !recall.fragments.is_empty() => {
                    // Entity-wide — the curator's consolidated knowledge:
                    // every KNN-resolved h_mem regardless of who wrote it.
                    let entity_wide: Vec<serde_json::Value> = recall
                        .fragments
                        .iter()
                        .take(limit)
                        .map(|(t, distance)| h_mem_with_distance(t, distance))
                        .collect();
                    let entity_wide_count = entity_wide.len();
                    let mut entity_wide_json = json!({
                        "count": entity_wide_count,
                        "h_mems": entity_wide,
                    });

                    // Perspective-scoped — the curator's own turns: the same
                    // semantic hits filtered to h_mems the curator wrote.
                    let perspective_scoped: Vec<serde_json::Value> = recall
                        .fragments
                        .iter()
                        .filter(|(t, _)| t.access.perspective == Some(self.webid))
                        .take(limit)
                        .map(|(t, distance)| h_mem_with_distance(t, distance))
                        .collect();
                    let perspective_count = perspective_scoped.len();
                    let mut perspective_json = json!({
                        "count": perspective_count,
                        "h_mems": perspective_scoped,
                    });

                    // Hits that failed h_mem resolution — or rows the model
                    // gate excluded — must not vanish: name the counts so
                    // partial (or fully failed) recall reads as degraded, not
                    // as missing memories.
                    if let Some(note) = degradation_note(
                        recall.resolution_failures,
                        recall.excluded_model_mismatch,
                        entity_wide_count,
                    ) {
                        let note = json!(note);
                        entity_wide_json["note"] = note.clone();
                        perspective_json["note"] = note;
                    }
                    result["entity_wide_fragments"] = entity_wide_json;
                    result["perspective_scoped_fragments"] = perspective_json;
                }
                // Degradation, not a silent fallback — surface why semantic
                // recall is unavailable, then fall back to the exact-entity
                // lookup (which only matches when the query IS an entity).
                // The note is preserved alongside the fallback results so the
                // operator can distinguish "semantic recall broken" from
                // "no matching memories" — same pattern as
                // `curator_semantic_search`.
                Err(reason) => {
                    let reason = reason.to_string();
                    result["entity_wide_fragments"] = exact_recall_leg(
                        stores.memory(),
                        |memory| memory.query_deduped(&req.query),
                        Some(limit),
                        false,
                        Some(&reason),
                    );
                    result["perspective_scoped_fragments"] = exact_recall_leg(
                        stores.memory(),
                        |memory| memory.query_for_deduped(&req.query, self.webid),
                        Some(limit),
                        true,
                        Some(&reason),
                    );
                }
                Ok(recall) => {
                    // Every KNN hit failed to resolve, or none matched. When
                    // hits failed, bare count:0 scopes would read as "the
                    // curator has no memory of this" — say resolution failed
                    // instead (the empty-result-as-success trap).
                    let mut entity_wide_json = json!({
                        "count": 0,
                        "h_mems": [],
                    });
                    let mut perspective_json = json!({
                        "count": 0,
                        "h_mems": [],
                    });
                    if recall.resolution_failures > 0 {
                        let note = resolution_failure_note(recall.resolution_failures, 0);
                        entity_wide_json["note"] = json!(note);
                        perspective_json["note"] = json!(note);
                    }
                    result["entity_wide_fragments"] = entity_wide_json;
                    result["perspective_scoped_fragments"] = perspective_json;
                }
            }

            Ok(result)
        })
        .await
    }

    // ── Algedonic History ──────────────────────────────────────────────

    #[tool(description = "Read the newest algedonic events in a time window")]
    pub async fn curator_algedonic_log(
        &self,
        Parameters(req): Parameters<AlgedonicLogRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_algedonic_log", async {
            let stores = self.db.get();
            let store = stores.regulation_store()?;
            let hours = req.hours.unwrap_or(24);
            let since = chrono::Utc::now() - chrono::Duration::hours(hours as i64);
            match store.query_recent_algedonic(since, 500) {
                Ok(events) => {
                    let s: Vec<serde_json::Value> = events
                        .iter()
                        .map(|e| {
                            json!({
                                "timestamp": e.timestamp.to_rfc3339(),
                                "span": e.span.path,
                                "phase": format!("{:?}", e.phase),
                                "observation": e.observation,
                            })
                        })
                        .collect();
                    Ok(json!({"window_hours": hours, "ordering": "newest_first", "count": s.len(), "events": s}))
                }
                Err(e) => Err(map_infra_error(&e, "Algedonic query failed")),
            }
        })
        .await
    }

    // ── Regulation Query (for platform governance transparency) ────────────────

    #[tool(
        description = "Query Regulation records across namespaces within a time window. An optional namespace prefix matches that path or dot-delimited descendants before the chronological result limit is applied. At least one of `namespace` or `window_seconds` must be provided — a request with neither is rejected as invalid_argument (the truncated-argument signature), not defaulted to all records."
    )]
    pub async fn reg_query(
        &self,
        Parameters(req): Parameters<RegQueryRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reg_query", async {
            // A query with neither a namespace nor a window is the truncated-argument
            // signature: the tool-call JSON was cut before dispatch, so every `Option`
            // field landed as `None`. Reject it loudly rather than silently defaulting
            // to "all namespaces / last hour" and returning wrong-filtered data as a
            // success (`.rules`: a silent fallback is a broken feedback loop). A
            // deliberate `namespace: None` with a `window_seconds` still means the
            // documented all-namespaces query.
            if req.namespace.is_none() && req.window_seconds.is_none() {
                return Err(McpToolError::invalid_argument(
                    "reg_query requires a namespace or window_seconds; both absent means the \
                     arguments did not arrive (truncated tool-call JSON), not an all-records query",
                ));
            }
            let stores = self.db.get();
            let store = stores.regulation_store()?;
            let window_secs = req.window_seconds.unwrap_or(3600);
            let limit = req.limit.unwrap_or(100) as u64;
            let since = chrono::Utc::now() - chrono::Duration::seconds(window_secs as i64);
            let records = store
                .query_records(since, req.namespace.as_deref(), limit)
                .map_err(|e| map_infra_error(&e, "Regulation query failed"))?;
            let events: Vec<serde_json::Value> = records
                .into_iter()
                .map(|event| {
                    json!({
                        "timestamp": event.timestamp.to_rfc3339(),
                        "namespace": event.span.namespace.as_str(),
                        "path": event.span.path,
                        "phase": format!("{:?}", event.phase),
                        "observation": event.observation,
                    })
                })
                .collect();

            let namespace_info = req.namespace.as_deref().unwrap_or("all");
            // Telemetry breadcrumb, not a persisted event (emit is tracing::info!).
            // Use the Curation span (not Tool) so this read-only observability
            // query is not mislabeled as a curator tool invocation in the
            // Regulation log — `reg.curation` / `reg_query_observed` reads as
            // “the curator observed a Regulation query”, not “the curator tool
            // was invoked”.
            RegulationSpan::Curation.emit("reg_query_observed");

            Ok(json!({
                "namespace": namespace_info,
                "window_seconds": window_secs,
                "count": events.len(),
                "events": events
            }))
        })
        .await
    }

    // ── Skill-use issue reporting (Co-evolution Phase 2) ────────────────

    /// Report a skill-use issue — submitted by a skill's `on_failure` config
    /// when an MCP tool call fails or produces unexpected output. The report
    /// is stored as an h_mem in the curator's memory store with
    /// entity `skill_use_issue:<skill_name>` so it is queryable via
    /// `curator_memory_recall` and `curator_semantic_search` — stored at the
    /// 0.5 confidence floor (not the `HMem::new` 1.0 default: unverified
    /// observations must not outrank verified facts in recall ranking) and
    /// with the report text embedded under the entity so semantic search
    /// finds it by meaning.
    ///
    /// This is the skill-reported input channel of the co-evolution loop:
    /// skills report MCP tool issues → the Curator analyzes patterns →
    /// CuratorDirectives evolve the MCP tool (add validation, improve error
    /// messages, add fallbacks). Complements the existing runtime telemetry
    /// (reg.* spans, algedonic events).
    #[tool(
        description = "Report a skill-use issue when an MCP tool call fails or produces unexpected output. Stored as an h_mem for Curator pattern analysis. The report includes skill/tool/step evidence, a granular failure_type, and a required controlled failure_origin ownership class."
    )]
    pub async fn curator_report_skill_use_issue(
        &self,
        Parameters(req): Parameters<ReportSkillUseIssueRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_report_skill_use_issue", async {
            let stores = self.db.get();
            let memory = stores.memory()?;

            let entity = format!("skill_use_issue:{}", req.skill_name);
            let now = chrono::Utc::now();
            let recall_text = format!(
                "skill-use issue: {} / {} (step {}, origin {}): {}",
                req.skill_name,
                req.tool_name,
                req.step_ordinal,
                req.failure_origin.as_str(),
                req.error
            );

            let report_value = json!({
                "skill_name": req.skill_name,
                "tool_name": req.tool_name,
                "step_ordinal": req.step_ordinal,
                "error": req.error,
                "tool_input": req.tool_input,
                "failure_type": req.failure_type,
                "failure_origin": req.failure_origin.as_str(),
                "reported_at": now.to_rfc3339(),
                "recall_text": recall_text,
            });

            let h_mem = hkask_storage::HMem::new(
                &entity,
                &format!("tool_failure:{}", req.tool_name),
                report_value,
                self.webid,
            )
            // The 0.5 floor — NOT the `HMem::new` 1.0 default. Issue reports
            // are unverified observations; at 1.0 they outranked verified
            // facts in recall ranking (confidence is a ranking multiplier).
            .with_confidence(hkask_types::Confidence::new(0.5));
            let write = memory.prepare_semantic_write(h_mem)
                .map_err(|e| map_memory_store_error(e, "Failed to prepare skill-use issue report"))?;
            publish_semantic_write(self.inference_port.as_ref(), write).await?;

            RegulationSpan::Curation.emit("skill_use_issue_reported");

            Ok(json!({
                "reported": true,
                "entity": entity,
                "skill_name": req.skill_name,
                "tool_name": req.tool_name,
                "step_ordinal": req.step_ordinal,
                "failure_type": req.failure_type,
                "failure_origin": req.failure_origin.as_str(),
                "semantic_recall": "embedded",
                "guidance": "The issue has been recorded in the curator's memory store. Use curator_memory_recall with entity 'skill_use_issue:<skill_name>' or curator_semantic_search to retrieve accumulated reports."
            }))
        })
        .await
    }

    // ── Curator memory edit tools (Priority 5) ───────────────────────────
    //
    // These tools give the curator agent write access to its own memory,
    // with evidence-grounding and confidence-floor constraints. User
    // threads cannot write to memory directly — only the curator (the one
    // agent with a feedback loop).
    //
    // Grounding: Dunning's Cassandra quandary (`138299529:16-17`) — poor
    // performers can't evaluate which memories are worth writing. MemGPT
    // (Packer et al., 2023) — OS-style memory management with permission
    // boundaries.

    /// Insert a new memory into the curator's store.
    ///
    /// The memory starts at confidence 0.5 (the floor — NOT the model's
    /// self-assessed confidence). Confidence is calibrated by subsequent
    /// Brier-scored outcomes, not by self-assessment.
    ///
    /// The value's text is embedded under the entity (the entity_ref
    /// invariant) so `curator_semantic_search` finds the memory by meaning,
    /// not just by exact entity name. Embedding failure is non-fatal and
    /// surfaced in the output.
    ///
    /// Evidence-grounding: the `evidence_h_mem_id` field must cite a
    /// specific h_mem ID that supports this memory. The tool
    /// rejects inserts without a citation.
    #[tool(
        description = "Insert a new memory into the curator's store. Requires evidence citation (h_mem ID). Confidence starts at 0.5 — calibrated by outcomes, not self-assessment."
    )]
    pub async fn memory_insert(
        &self,
        Parameters(req): Parameters<MemoryInsertRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "memory_insert", async {
            let stores = self.db.get();
            let memory = stores.memory()?;

            let (_evidence_id, _evidence) = fetch_existing_h_mem(
                memory,
                "evidence_h_mem_id",
                &req.evidence_h_mem_id,
                format!(
                    "Evidence h_mem '{}' not found — memory_insert requires an existing citation",
                    req.evidence_h_mem_id
                ),
                "Failed to verify evidence h_mem",
            )?;

            // The write-path contradiction check (R5, fermi's "unverified is
            // a work item" discipline inverted: a *conflicting* value is not
            // a work item, it is a finding): a colliding insert surfaces the
            // contradiction — never silently coexisting. Existing rows for
            // the same (entity, attribute) holding a different payload value
            // are surfaced by id and routed to the resolution paths; a
            // same-value insert (re-assertion) is not a contradiction.
            // `memory_update` (revision by id) and the machine ingestion
            // paths are unaffected — the gate is the deliberate
            // model-facing knowledge write only.
            let existing = memory
                .query_deduped(&req.entity)
                .map_err(|e| map_memory_store_error(e, "Failed to read existing memories for the contradiction check"))?;
            let incoming = serde_json::Value::from(req.value);
            let new_payload = comparable_payload(&incoming);
            let contradictions: Vec<&hkask_storage::HMem> = existing
                .iter()
                .filter(|h| {
                    h.attribute == req.attribute && comparable_payload(&h.value) != new_payload
                })
                .collect();
            if !contradictions.is_empty() {
                let ids: Vec<String> = contradictions.iter().map(|h| h.id.to_string()).collect();
                return Err(McpToolError::failed_precondition(format!(
                    "contradiction: {} existing h_mem(s) for '{}/{}' hold a different value ({ids:?}). \
                     Resolve via memory_resolve_contradiction (forget or update_confidence on the \
                     wrong one) or memory_update (revise by id); a differing value for the same \
                     key may not coexist silently",
                    contradictions.len(),
                    req.entity,
                    req.attribute
                )));
            }

            // Evidence and notes are tool concerns; passage derivation and
            // atomic row/vector publication belong to the shared store.
            let mut value = incoming;
            if let Some(note) = &req.note {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("_note".to_string(), serde_json::Value::String(note.clone()));
                }
            }

            let h_mem = hkask_storage::HMem::new(
                &req.entity,
                &req.attribute,
                value,
                self.webid,
            )
            .with_confidence(hkask_types::Confidence::new(0.5))
            // The extraction ceiling, enforced here: the model's insert is
            // `model_inference` — permanently, no matter how often it is
            // recalled or re-asserted. The request schema offers no tier
            // field to claim; only the machine paths that hold a verbatim
            // tool outcome stamp `tool_observed`.
            .with_provenance(hkask_storage::Provenance::ModelInference);
            let provenance = h_mem.provenance.as_str();
            let write = memory.prepare_semantic_write(h_mem)
                .map_err(|e| map_memory_store_error(e, "Failed to prepare curator memory"))?;
            publish_semantic_write(self.inference_port.as_ref(), write).await?;

            RegulationSpan::Curation.emit("memory_inserted");

            Ok(json!({
                "inserted": true,
                "entity": req.entity,
                "attribute": req.attribute,
                "confidence": 0.5,
                "provenance": provenance,
                "evidence_h_mem_id": req.evidence_h_mem_id,
                "semantic_recall": "embedded",
                "guidance": "Memory stored at confidence 0.5 with a semantic embedding, provenance model_inference (the extraction ceiling — a model-synthesized memory never retrieves as tool-observed). Use memory_update to adjust confidence after outcome observation. Retrieve via curator_memory_recall with this entity, or curator_semantic_search by meaning."
            }))
        })
        .await
    }

    /// Update an existing memory's confidence via Bayesian combination.
    ///
    /// The new confidence is combined with the existing confidence using
    /// log-odds (Bayesian) pooling — not replacement.
    #[tool(
        description = "Update an existing memory's confidence via Bayesian combination. The new confidence is combined (not replaced) with the existing value using log-odds pooling."
    )]
    pub async fn memory_update(
        &self,
        Parameters(req): Parameters<MemoryUpdateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "memory_update", async {
            let stores = self.db.get();
            let memory = stores.memory()?;

            let (h_mem_id, existing_h_mem) = fetch_existing_h_mem(
                memory,
                "h_mem_id",
                &req.h_mem_id,
                format!("h_mem '{}' not found", req.h_mem_id),
                "Failed to fetch h_mem for update",
            )?;

            // Bayesian-combine the new confidence with the existing one.
            let new_confidence_raw = hkask_types::Confidence::new(req.new_confidence);
            let combined = hkask_memory::combine_confidences(
                existing_h_mem.confidence,
                new_confidence_raw,
            );

            // Use the new value if provided, otherwise keep the existing.
            let value = req
                .new_value
                .map(serde_json::Value::from)
                .unwrap_or_else(|| existing_h_mem.value.clone());

            let write = memory.prepare_semantic_revision(&h_mem_id, value, combined)
                .map_err(|e| map_memory_store_error(e, "Failed to prepare memory revision"))?;
            let written_id = publish_semantic_write(self.inference_port.as_ref(), write).await?;

            RegulationSpan::Curation.emit("memory_updated");

            Ok(json!({
                "updated": true,
                "h_mem_id": written_id.to_string(),
                "semantic_recall": "embedded",
                "previous_confidence": existing_h_mem.confidence.value(),
                "input_confidence": req.new_confidence,
                "combined_confidence": combined.value(),
                "reason": req.reason,
                "guidance": "Confidence updated via Bayesian combination. Use curator_memory_recall to verify."
            }))
        })
        .await
    }

    /// Resolve a contradiction between two or more memories.
    ///
    /// This is the therapy process tool — it resolves cognitive dissonance
    /// in the memory store by forgetting or de-conflicting contradictory
    /// h_mems.
    #[tool(
        description = "Resolve a contradiction between memories. Strategies: 'forget' (delete the h_mem from the database), 'update_confidence' (lower confidence). Requires a reason citing the contradiction."
    )]
    pub async fn memory_resolve_contradiction(
        &self,
        Parameters(req): Parameters<MemoryResolveContradictionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "memory_resolve_contradiction", async {
            let stores = self.db.get();
            let memory = stores.memory()?;

            if req.h_mem_ids.is_empty() || req.h_mem_ids.iter().any(|id| id.trim().is_empty()) {
                return Err(McpToolError::invalid_argument(
                    "h_mem_ids must contain at least one non-empty h_mem ID",
                ));
            }

            let (target_id, target_h_mem) = fetch_existing_h_mem(
                memory,
                "target_h_mem_id",
                &req.target_h_mem_id,
                format!("Target h_mem '{}' not found", req.target_h_mem_id),
                "Failed to fetch target h_mem",
            )?;

            match req.strategy.as_str() {
                "forget" => {
                    memory
                        .delete_h_mem(&target_id)
                        .map_err(|e| map_memory_store_error(e, "Failed to forget h_mem"))?;
                    RegulationSpan::Curation.emit("contradiction_forgotten");
                    Ok(json!({
                        "resolved": true,
                        "strategy": "forget",
                        "target_h_mem_id": req.target_h_mem_id,
                        "contradicting_h_mem_ids": req.h_mem_ids,
                        "reason": req.reason
                    }))
                }
                "update_confidence" => {
                    let new_confidence = req.new_confidence.ok_or_else(|| {
                        McpToolError::invalid_argument(
                            "new_confidence is required for 'update_confidence' strategy",
                        )
                    })?;
                    let confidence = hkask_types::Confidence::new(new_confidence);
                    let write = memory
                        .prepare_semantic_revision(&target_id, target_h_mem.value, confidence)
                        .map_err(|e| {
                            map_memory_store_error(e, "Failed to prepare contradiction revision")
                        })?;
                    let written_id =
                        publish_semantic_write(self.inference_port.as_ref(), write).await?;
                    RegulationSpan::Curation.emit("contradiction_confidence_lowered");
                    Ok(json!({
                        "resolved": true,
                        "strategy": "update_confidence",
                        "target_h_mem_id": written_id.to_string(),
                        "new_confidence": new_confidence,
                        "contradicting_h_mem_ids": req.h_mem_ids,
                        "reason": req.reason
                    }))
                }
                other => Err(McpToolError::invalid_argument(format!(
                    "Unknown strategy '{other}' — must be one of: forget, update_confidence"
                ))),
            }
        })
        .await
    }

    // ── Memory hygiene tools (age prune + dedup) ─────────────────────────
    //
    // Complements the confidence-based consolidation service with two
    // deterministic, non-LLM axes: age-based hard-delete and near-duplicate
    // string dedup. Both are operator-invoked — the curator proposes, the
    // operator approves (same consent model as therapy/contradiction
    // resolution).

    /// Prune h_mems older than a specified age. Hard-deletes h_mems whose
    /// observation timestamp is older than `max_age_days`, optionally
    /// sparing h_mems recalled within a grace window. Distinct from
    /// confidence decay (lowers weight, never deletes) and confidence-based
    /// consolidation (deletes low-confidence).
    #[tool(
        description = "Prune curator h_mems older than max_age_days. Default scope is turn storage only (curator:thread:) — knowledge-layer rows are untouched; set all_layers=true for full-store; set prefixes=[\"...\"] to prune only entities under those prefixes (e.g. the skill_use_issue: incident log). Hard-deletes aged h_mems, optionally sparing those recalled within spare_recalled_within_days. Deterministic, non-LLM. Distinct from confidence-based consolidation."
    )]
    pub async fn curator_memory_prune(
        &self,
        Parameters(req): Parameters<MemoryPruneRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_memory_prune", async {
            let stores = self.db.get();
            let memory = stores.memory()?;

            if req.max_age_days <= 0 {
                return Err(McpToolError::invalid_argument(
                    "max_age_days must be positive",
                ));
            }

            let all_layers = req.all_layers.unwrap_or(false);
            // Fail-closed scope: the default valve touches turn storage
            // only — knowledge-layer rows (rulings, verified status,
            // lessons) are destroyed only by explicit opt-in. A caller-
            // supplied `prefixes` list narrows the valve to those entity
            // prefixes (e.g. the skill_use_issue: incident log); empty
            // and all_layers-conflicting scope requests are rejected
            // visibly — a silent no-op or a silently-picked scope is a
            // broken feedback loop.
            if all_layers && req.prefixes.is_some() {
                return Err(McpToolError::invalid_argument(
                    "conflicting scopes: all_layers=true and prefixes are both set — name one scope",
                ));
            }
            let outcome = if all_layers {
                memory
                    .prune_by_age(req.max_age_days, req.spare_recalled_within_days)
                    .map_err(|e| map_memory_store_error(e, "Age-based prune failed"))?
            } else if let Some(prefixes) = req.prefixes.as_deref() {
                if prefixes.is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "prefixes must name at least one entity prefix — an empty list would silently prune nothing",
                    ));
                }
                let scope: Vec<&str> = prefixes.iter().map(String::as_str).collect();
                memory
                    .prune_by_age_in_prefixes(
                        &scope,
                        req.max_age_days,
                        req.spare_recalled_within_days,
                    )
                    .map_err(|e| map_memory_store_error(e, "Age-based prune failed"))?
            } else {
                memory
                    .prune_by_age_in_prefixes(
                        &[thread_turns::SHARED_TURN_PREFIX],
                        req.max_age_days,
                        req.spare_recalled_within_days,
                    )
                    .map_err(|e| map_memory_store_error(e, "Age-based prune failed"))?
            };

            RegulationSpan::Curation.emit("memory_pruned");

            Ok(json!({
                "pruned": true,
                "all_layers": all_layers,
                "prefixes": req.prefixes,
                "max_age_days": req.max_age_days,
                "spare_recalled_within_days": req.spare_recalled_within_days,
                "candidates": outcome.candidates,
                "deleted_count": outcome.deleted_count,
                "spared_count": outcome.spared_count,
                "failed_count": outcome.failed_count,
            }))
        })
        .await
    }

    /// Deduplicate h_mems by normalized string value. Groups by
    /// (entity, attribute, normalized_value), keeps highest-confidence,
    /// deletes the rest. Non-string values skipped.
    #[tool(
        description = "Deduplicate curator h_mems by normalized string value. Groups by (entity, attribute, normalized_value), keeps highest-confidence, deletes the rest. Deterministic, non-LLM. Non-string values and turn storage (curator:thread:) skipped."
    )]
    pub async fn curator_memory_dedup(
        &self,
        Parameters(req): Parameters<MemoryDedupRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_memory_dedup", async {
            let stores = self.db.get();
            let memory = stores.memory()?;

            let limit = req.limit.unwrap_or(10_000);
            if limit == 0 {
                return Err(McpToolError::invalid_argument("limit must be positive"));
            }

            let outcome = memory
                .dedup_by_normalized_value(limit)
                .map_err(|e| map_memory_store_error(e, "Normalized-value dedup failed"))?;

            RegulationSpan::Curation.emit("memory_deduped");

            Ok(json!({
                "deduped": true,
                "scanned": outcome.scanned,
                "groups_with_dupes": outcome.groups_with_dupes,
                "deleted_count": outcome.deleted_count,
                "failed_count": outcome.failed_count,
                "skipped_non_string": outcome.skipped_non_string,
                "skipped_turn_storage": outcome.skipped_turn_storage,
            }))
        })
        .await
    }

    /// Repair semantic embeddings for knowledge-layer h_mems whose insert-time
    /// embedding failed. Embeddings-table-only — no h_mem is created,
    /// modified, or deleted. Turns (embedded at ingest), distillation
    /// watermarks and goal rows are excluded.
    #[tool(
        description = "Backfill semantic embeddings across the curator's memory store. mode=missing (default) embeds h_mems whose exact canonical passage has no embedding row — eligibility is exact (entity + canonical passage), not entity-level; turns, distillation watermarks, and goal rows are excluded. mode=model_mismatch re-embeds stored rows whose recorded model matches neither the current requested embedding model nor its provider-confirmed actual form — the same two-form predicate the search gate excludes on, so a model migration's gated-out rows are restored rather than silently invisible. dry_run lists candidates without embedding."
    )]
    pub async fn curator_memory_backfill_embeddings(
        &self,
        Parameters(req): Parameters<BackfillEmbeddingsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_memory_backfill_embeddings", async {
            let mode = match req.mode.as_deref().map(str::trim) {
                None | Some("") | Some("missing") => backfill::BackfillMode::Missing,
                Some("model_mismatch") => backfill::BackfillMode::ModelMismatch,
                Some(other) => {
                    return Err(McpToolError::invalid_argument(format!(
                        "mode must be \"missing\" or \"model_mismatch\", got \"{other}\""
                    )));
                }
            };

            let memory = self.db.get().memory()?.clone();

            match mode {
                backfill::BackfillMode::Missing => {
                    backfill::backfill_missing_passages(
                        self.inference_port.as_ref(),
                        &memory,
                        req.dry_run.unwrap_or(false),
                    )
                    .await
                }
                backfill::BackfillMode::ModelMismatch => {
                    backfill::backfill_model_mismatched_rows(
                        self.inference_port.as_ref(),
                        &memory,
                        req.dry_run.unwrap_or(false),
                    )
                    .await
                }
            }
        })
        .await
    }

    /// Queries the thread's turn h_mems (`curator:thread:<thread_id>`) via
    /// `thread_turns::thread_turns`, the one turn-discovery contract shared
    /// with the distillation pass. Returns their IDs and content as
    /// extraction candidates. The curator reviews and inserts the ones
    /// worth keeping via `memory_insert` (which requires evidence citation).
    /// This is the on-demand version of ALWAYS-mode learning — no
    /// background LLM call, no automatic insertion.
    #[tool(
        description = "Extract candidate semantic memories from a thread's turn history. Returns turn h_mems with IDs and content. The curator reviews and inserts worth keeping via memory_insert. On-demand ALWAYS-mode learning — no background LLM, no automatic insertion."
    )]
    pub async fn curator_memory_extract(
        &self,
        Parameters(req): Parameters<MemoryExtractRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_memory_extract", async {
            let stores = self.db.get();
            let memory = stores.memory()?;

            // Both storage prefixes hold the thread's turns — the
            // curator-perspective originals and the shared copies ingest
            // writes for every turn. The contract lives in `thread_turns`,
            // shared with the distillation pass.
            let h_mems = thread_turns::thread_turns(memory, &req.thread_id)
                .map_err(|e| map_memory_store_error(e, "Failed to query thread turns"))?;

            let candidates: Vec<serde_json::Value> = h_mems
                .iter()
                .map(|h| {
                    json!({
                        "h_mem_id": h.id.to_string(),
                        "entity": h.entity,
                        "attribute": h.attribute,
                        "value": h.value,
                        "confidence": h.confidence.value(),
                        "observed_at": h.observed_at.to_rfc3339(),
                        "evidence_citation": h.id.to_string(),
                    })
                })
                .collect();

            RegulationSpan::Curation.emit("memory_extracted");

            Ok(json!({
                "thread_id": req.thread_id,
                "turn_count": candidates.len(),
                "candidates": candidates,
                "guidance": "Review the candidates and insert worth keeping via memory_insert. Each candidate's h_mem_id is the evidence_citation for memory_insert.",
            }))
        })
        .await
    }

    // ── Trust ledger (Binding 5 — trust events ARE h_mems) ───────────

    #[tool(
        description = "Record a verbatim instruction in the trust ledger. The text is stored exactly as given; the weight classifies how explicitly it was stated (1=implicit, 2=committed, 3=explicit). Returns the ledger event id for later outcome resolution."
    )]
    pub async fn trust_ledger_record_instruction(
        &self,
        Parameters(req): Parameters<TrustLedgerRecordInstructionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "trust_ledger_record_instruction", async {
            let stores = self.db.get();
            let memory = stores.memory.clone().ok_or_else(|| {
                McpToolError::failed_precondition("curator memory store unavailable")
            })?;
            let store =
                crate::trust_ledger::CuratorTrustLedgerStore::new(memory, self.webid.clone());
            let ledger = hkask_trust::TrustLedger::new(std::sync::Arc::new(store));
            let weight = match req.weight {
                1 => hkask_trust::Weight::Implicit,
                2 => hkask_trust::Weight::Committed,
                _ => hkask_trust::Weight::Explicit,
            };
            let id = ledger
                .record_instruction(&req.text, &req.source, weight)
                .map_err(trust_error_to_mcp)?;
            RegulationSpan::Curation.emit("trust_ledger_record_instruction");
            Ok(serde_json::json!({
                "event_id": id.as_str(),
                "text": req.text,
                "source": req.source,
                "weight": req.weight,
                "outcome": "unverified",
            }))
        })
        .await
    }

    #[tool(
        description = "Resolve a trust-ledger event's verification outcome: kept, violated, or unverified. The oracle_note names what verified it (a test name, a build, a grep, a re-run)."
    )]
    pub async fn trust_ledger_record_outcome(
        &self,
        Parameters(req): Parameters<TrustLedgerRecordOutcomeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "trust_ledger_record_outcome", async {
            let stores = self.db.get();
            let memory = stores.memory.clone().ok_or_else(|| {
                McpToolError::failed_precondition("curator memory store unavailable")
            })?;
            let store =
                crate::trust_ledger::CuratorTrustLedgerStore::new(memory, self.webid.clone());
            let ledger = hkask_trust::TrustLedger::new(std::sync::Arc::new(store));
            let id = hkask_trust::LedgerEventId::from_string(&req.event_id);
            let outcome = match req.outcome.as_str() {
                "kept" => hkask_trust::Outcome::Kept,
                "violated" => hkask_trust::Outcome::Violated,
                _ => hkask_trust::Outcome::Unverified,
            };
            ledger
                .record_outcome(&id, outcome, req.oracle_note.clone())
                .map_err(trust_error_to_mcp)?;
            RegulationSpan::Curation.emit("trust_ledger_record_outcome");
            Ok(serde_json::json!({
                "event_id": req.event_id,
                "outcome": req.outcome,
                "oracle_note": req.oracle_note,
            }))
        })
        .await
    }

    #[tool(
        description = "Read the windowed trust account: trust points (600 = 1.0), kept/violated/unverified counts, delegation radius percent. Default window: 24 hours (the algedonic review cycle)."
    )]
    pub async fn trust_ledger_state(
        &self,
        Parameters(req): Parameters<TrustLedgerStateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "trust_ledger_state", async {
            let stores = self.db.get();
            let memory = stores.memory.clone().ok_or_else(|| {
                McpToolError::failed_precondition("curator memory store unavailable")
            })?;
            let store =
                crate::trust_ledger::CuratorTrustLedgerStore::new(memory, self.webid.clone());
            let ledger = hkask_trust::TrustLedger::new(std::sync::Arc::new(store));
            let hours = req.window_hours.unwrap_or(24);
            let since = chrono::Utc::now() - chrono::Duration::hours(hours);
            let stats = ledger.window_stats(since).map_err(trust_error_to_mcp)?;
            Ok(serde_json::json!({
                "trust_points": stats.trust_points,
                "kept": stats.kept,
                "violated": stats.violated,
                "unverified": stats.unverified,
                "delegation_radius_percent": stats.delegation_radius_percent(),
                "window_hours": hours,
            }))
        })
        .await
    }

    #[tool(
        description = "Project candidate actions against the goal gradient. A trust-killing action (concealment) reads as the path terminating (P_goal: 0%); the honest path keeps a live probability. Each action names what it violates and whether it conceals."
    )]
    pub async fn trust_ledger_steer(
        &self,
        Parameters(req): Parameters<TrustLedgerSteerRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "trust_ledger_steer", async {
            let stores = self.db.get();
            let memory = stores.memory.clone().ok_or_else(|| {
                McpToolError::failed_precondition("curator memory store unavailable")
            })?;
            let store =
                crate::trust_ledger::CuratorTrustLedgerStore::new(memory, self.webid.clone());
            let ledger = hkask_trust::TrustLedger::new(std::sync::Arc::new(store));
            let hours = req.window_hours.unwrap_or(24);
            let since = chrono::Utc::now() - chrono::Duration::hours(hours);
            let trust = ledger.trust_state(since).map_err(trust_error_to_mcp)?;

            let actions: Vec<hkask_trust::SteerAction> = req
                .actions
                .iter()
                .map(|a| hkask_trust::SteerAction {
                    name: a.name.clone(),
                    violates: a
                        .violates
                        .iter()
                        .map(|v| {
                            let w = match v.weight {
                                1 => hkask_trust::Weight::Implicit,
                                2 => hkask_trust::Weight::Committed,
                                _ => hkask_trust::Weight::Explicit,
                            };
                            (w, v.instruction.clone())
                        })
                        .collect(),
                    conceals: a.conceals,
                    completion_percent: a.completion_percent,
                    note: a.note.clone(),
                })
                .collect();

            let projections = ledger.steer(trust, &actions);
            let rendered: Vec<serde_json::Value> = projections
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "action": p.action,
                        "path": if p.cliff { "cliff" } else { "open" },
                        "p_goal_percent": p.p_goal_percent,
                        "delta_trust_points": p.delta_trust_points,
                        "note": p.note,
                    })
                })
                .collect();

            Ok(serde_json::json!({
                "trust_points": trust,
                "projections": rendered,
            }))
        })
        .await
    }
}

fn trust_error_to_mcp(e: hkask_trust::TrustError) -> McpToolError {
    match e {
        hkask_trust::TrustError::StoreUnavailable => McpToolError::failed_precondition(
            "trust ledger store unavailable — account unknown, not zero",
        ),
        hkask_trust::TrustError::EventNotFound(id) => {
            McpToolError::not_found(format!("trust ledger event '{id}' not found"))
        }
        hkask_trust::TrustError::EmptyInstruction => {
            McpToolError::invalid_argument("instruction text was empty — not an event")
        }
    }
}

// ── Server startup ─────────────────────────────────────────────────────

/// The curator server's embedding model: the env-resolved model when
/// configured, else `None`. `None` is NOT a fallback to a hidden constant
/// (the operator's no-hidden-models spec) — the semantic paths degrade
/// VISIBLY: `curator_semantic_search` returns a typed
/// `EmbeddingNotConfigured` error naming the setting, and the insert
/// paths fail with `failed_precondition` + warn (the embed-at-publish
/// contract, therapy 2026-10-08 — no row lands without its embedding).
/// The 2026-09-04 regression this previously guarded against was the
/// degradation firing under default settings; the ratified fix is to
/// surface it (the operator sets `kask.models.embedding_model`), not to
/// hide a constant model behind it.
pub(crate) fn curator_embedding_model() -> Option<String> {
    hkask_inference::model_constants::embedding_model()
}

async fn semantic_embedding(
    inference_port: &dyn hkask_types::InferencePort,
    text: &str,
) -> Result<(Vec<f32>, String), McpToolError> {
    let model = curator_embedding_model().ok_or_else(|| McpToolError::failed_precondition(
        "embedding failed — no embedding model configured; set kask.models.embedding_model (HKASK_EMBEDDING_MODEL); memory was NOT stored",
    ))?;
    let batch = inference_port
        .embed_with_dimensions(
            &model,
            &[text.to_string()],
            Some(hkask_storage::embedding_dim() as u32),
        )
        .await
        .map_err(|error| {
            McpToolError::failed_precondition(format!(
                "embedding failed — memory was NOT stored: {error}"
            ))
        })?;
    hkask_mcp_server::server::record_tool_usage(batch.usage, batch.cost_usd);
    if batch.vectors.len() != 1 || batch.vectors[0].is_empty() {
        return Err(McpToolError::failed_precondition(
            "embedding failed — expected one nonempty vector; memory was NOT stored",
        ));
    }
    let model = batch.actual_model.unwrap_or(model);
    let vector = batch.vectors.into_iter().next().ok_or_else(|| {
        McpToolError::failed_precondition("embedding failed — no vector returned")
    })?;
    Ok((vector, model))
}

async fn publish_semantic_write(
    inference_port: &dyn hkask_types::InferencePort,
    write: hkask_memory::memory_store::SemanticWrite<'_>,
) -> Result<hkask_storage::HMemId, McpToolError> {
    let (vector, model) = semantic_embedding(inference_port, write.passage()).await?;
    write
        .publish(&vector, &model)
        .map_err(|error| map_memory_store_error(error, "Failed to publish memory and embedding"))
}

/// Embed `text` under `entity` so semantic recall finds the h_mem being
/// stored — the entity_ref invariant (memory-system-specification.md §3).
/// Used by embedding-only backfill and distillation's guarded batch
/// publication. Standalone knowledge inserts and revisions instead use
/// `publish_semantic_write`, which cannot commit a row without its vector.
/// Returns whether the embedding landed; every failure warns naming the cause.
pub(crate) async fn embed_for_semantic_recall(
    inference_port: &dyn hkask_types::InferencePort,
    memory: &hkask_memory::MemoryStore,
    entity: &str,
    text: &str,
) -> bool {
    let result = match semantic_embedding(inference_port, text).await {
        Ok((vector, model)) => memory
            .store_embedding(entity, &vector, &model, Some(text))
            .map(|_| ())
            .map_err(|error| map_memory_store_error(error, "Failed to store semantic embedding")),
        Err(error) => Err(error),
    };
    match result {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(target: "hkask.mcp.curator", %error, entity,
                "Embedding unavailable — semantic recall degraded for this memory");
            false
        }
    }
}

pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    // Construct the inference port before entering the sync server-
    // construction closure. `resolve_inference_port` is async (it constructs
    // a `LazyInferencePort` — the bridge connection itself is deferred to
    // each `embed()` call, which re-tries `InferenceIpcClient::from_env()`);
    // the closure passed to `run_server` is sync, so the await must happen
    // here. Used by `curator_semantic_search` and `curator_consult` to embed
    // recall queries, by the insert paths (`memory_insert`,
    // `curator_report_skill_use_issue`) to embed stored memories, and by
    // the distillation timer to embed lessons.
    let inference_port = hkask_inference::resolve_inference_port().await;
    hkask_mcp_server::run_server(
        SERVER_NAME,
        env!("CARGO_PKG_VERSION"),
        move |ctx: hkask_mcp_server::server::ServerContext| {
            let db = Arc::new(CuratorDb::from_context(&ctx));
            // ALWAYS-mode distillation: the background pass shares the
            // server's DB handle, inference port, and webid, so lessons
            // enter through the same evidence + 0.5-floor invariants the
            // memory_insert tool enforces.
            distillation::spawn_distillation_timer(
                Arc::clone(&db),
                inference_port.clone(),
                ctx.webid,
                distillation::DistillationConfig::from_env(),
            );
            Ok(CuratorServer::new(ctx.webid, db, inference_port.clone()))
        },
        vec![hkask_mcp_server::CredentialRequirement::required(
            "HKASK_DB_PASSPHRASE",
            "SQLCipher encryption passphrase (resolved via hkask keystore chain when not set)",
        )],
    )
    .await
}

#[cfg(test)]
mod tests {
    /// expect: "The curator server refuses startup without its DB
    /// passphrase — 14 of 15 tools are DB-backed and there is no
    /// in-memory store to fall back to, so a limping start would surface
    /// permission_denied on every memory tool instead of one startup error
    /// naming HKASK_DB_PASSPHRASE." [P1] Motivating: User Sovereignty.
    /// [P2] Constraining: Transparent Imperfection — kata-kanban precedent
    /// (`kanban_startup_requires_durable_storage`).
    /// pre: the production startup source is compiled
    /// post: HKASK_DB_PASSPHRASE is declared required, not optional
    #[test]
    fn curator_startup_requires_durable_storage() {
        let source = include_str!("hkask_mcp_curator.rs");
        let required_passphrase = [
            "CredentialRequirement::required(",
            "\n            \"HKASK_DB_PASSPHRASE\"",
        ]
        .concat();
        assert!(
            source.contains(&required_passphrase),
            "HKASK_DB_PASSPHRASE must be declared required — the curator has no \
             in-memory fallback (CuratorStores::empty), so an optional declaration \
             starts a server whose every DB-backed tool call fails"
        );
    }
}

// Pins the registered tool-surface count end-to-end against the router the
// `#[tool_router(server_handler)]` block generates. Adding or removing a
// curator tool is an intentional surface change — this pin catches
// accidental drift (a dropped `#[tool]` attribute, a registration change)
// instead of shipping as an undocumented surface change. Mirrors
// the shared `tool_surface_pin!` macro every built-in server carries.
#[cfg(test)]
mod tool_surface_tests {
    use super::CuratorServer;

    include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));
    hkask_mcp_server::tool_surface_pin!(
        CuratorServer::tool_router(),
        "tool_router",
        19,
        tool_surface_is_exactly_19_registered_tools,
    );

    /// The retired escalation/advice tool family must not reappear — the
    /// algedonic-review board replaced it (2026-09-27). Split from the
    /// count pin so a re-introduction fails with its own name.
    #[test]
    fn retired_escalation_tools_stay_unregistered() {
        let tools = CuratorServer::tool_router().list_all();
        for removed in [
            "curator_escalations",
            "curator_escalation_resolve",
            "curator_escalation_dismiss",
            "curator_escalation_dismiss_by_pattern",
            "curator_advice_mark_applied",
            "curator_advice_reviews",
        ] {
            assert!(
                !tools.iter().any(|tool| tool.name == removed),
                "removed tool {removed} must not be registered"
            );
        }
    }
}
