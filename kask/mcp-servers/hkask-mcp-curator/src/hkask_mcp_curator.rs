#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
// `tokio` is in [dependencies] for the bin target's `#[tokio::main]`; the lib
// itself does not use it, so the unused_crate_dependencies lint fires on the
// lib target. This is the legitimate bin-needs-dep case.
#![allow(unused_crate_dependencies)]
//! hkask-mcp-curator — Curator MCP server library.
//!
//! Exposes the Curator's regulatory surface as MCP tools:
//! system health, escalation management, Regulation observability,
//! semantic memory search, memory recall, spec drift detection,
//! and algedonic event history.

pub(crate) mod distillation;
pub(crate) mod governance;
pub mod types;

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_mcp_server::server::{
    McpToolError, execute_tool, map_infra_error, map_memory_store_error, resolve_db_passphrase,
};
use hkask_services_core::{ErrorKind, ServiceError};
use hkask_storage::database::sqlite::SqliteDriver;

use hkask_types::event::RegulationSink;
use hkask_types::regulation::RegulationSpan;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use serde_json::json;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use types::*;

const SERVER_NAME: &str = "hkask-mcp-curator";

/// Minimum interval between self-heal re-open attempts, so a DB outage does
/// not trigger a full DB open + store construction on every tool call.
const HEAL_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Cap on fragments one entity may contribute to semantic recall. A thread
/// entity holds one h_mem per turn; without a cap a single chatty thread
/// floods the whole result set and every other entity vanishes from recall.
const MAX_FRAGMENTS_PER_ENTITY: usize = 2;

/// The four stores the curator's tools read, all backed by the curator's
/// sovereign `curator.db`. Grouped so the self-healing handle can swap the whole
/// set atomically after a re-open.
///
/// Named fields (not a positional tuple): tools address stores by name, so
/// adding or reordering a store cannot silently rebind a `..` destructuring
/// to the wrong store.
#[derive(Clone)]
pub struct CuratorStores {
    pub escalation_queue: Option<Arc<hkask_storage::EscalationQueue>>,
    pub regulation_store: Option<Arc<hkask_storage::RegulationArchive>>,
    /// The curator's unified memory. One store holds all of the curator's
    /// h_mems — the `HMemOntology` blob on each h_mem carries dual-axis
    /// anchoring (PKO process + DC state), so no second store handle is
    /// needed. The `curator_memory_recall` `recall_shape` parameter selects
    /// the recall shape (perspective-scoped vs entity-wide), not a store.
    pub memory: Option<Arc<hkask_memory::MemoryStore>>,
}

impl CuratorStores {
    /// All stores `None` — the DB-open level failed and a re-open may help.
    fn all_none(&self) -> bool {
        self.escalation_queue.is_none() && self.regulation_store.is_none() && self.memory.is_none()
    }

    /// Empty store set — used when the DB cannot be opened at all.
    pub fn empty() -> Self {
        Self {
            escalation_queue: None,
            regulation_store: None,
            memory: None,
        }
    }

    /// Guarded accessor — folds the repeated `permission_denied` store check
    /// that every tool used to inline.
    fn escalation_queue(&self) -> Result<&Arc<hkask_storage::EscalationQueue>, McpToolError> {
        self.escalation_queue
            .as_ref()
            .ok_or_else(|| McpToolError::permission_denied("EscalationQueue not available"))
    }

    fn regulation_store(&self) -> Result<&Arc<hkask_storage::RegulationArchive>, McpToolError> {
        self.regulation_store
            .as_ref()
            .ok_or_else(|| McpToolError::permission_denied("RegulationArchive not available"))
    }

    fn memory(&self) -> Result<&Arc<hkask_memory::MemoryStore>, McpToolError> {
        self.memory
            .as_ref()
            .ok_or_else(|| McpToolError::permission_denied("MemoryStore not available"))
    }
}

/// Self-healing handle over the curator's sovereign `curator.db` — the MCP-side
/// mirror of `CuratorStores` in `kask_bridge::memory`.
///
/// When the DB cannot be opened at startup (transient SQLCipher lock from a
/// previous server instance, late-arriving passphrase), every tool call
/// re-attempts the open via `get()`. A successful heal restores the curator's
/// full tool surface mid-process — no server restart. Failure is never
/// silent: construction failure logs `error!`, each failed heal attempt
/// warns once per outage round (re-armed on heal), a successful heal logs
/// `info!`.
pub struct CuratorDb {
    stores: RwLock<CuratorStores>,
    db_path: Option<String>,
    passphrase: Option<String>,
    heal_attempt_logged: AtomicBool,
    /// Tests construct handles with no valid path — healing disabled.
    heal_enabled: bool,
    /// Last heal attempt — gates re-opens so a DB outage doesn't trigger a
    /// full open + store construction on every tool call.
    last_heal_attempt: std::sync::Mutex<Option<std::time::Instant>>,
}

impl CuratorDb {
    fn from_context(ctx: &hkask_mcp_server::server::ServerContext) -> Self {
        let db_path = std::env::var("HKASK_CURATOR_DB").unwrap_or_else(|_| {
            let p = hkask_types::agent_paths::agent_db("curator");
            let resolved = hkask_types::agent_paths::resolve_under_data_dir(&p);
            if let Some(parent) = resolved.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                tracing::warn!(
                    target: "hkask.mcp.curator",
                    error = %e,
                    path = ?parent,
                    "Failed to create curator data directory — DB open will likely fail"
                );
            }
            resolved.to_string_lossy().to_string()
        });
        // Resolve passphrase via the canonical 2-tier chain
        // (ctx.credentials → resolve_credential which does env → keychain).
        // Falls back to None (in-memory / no-heal mode) on miss; the helper
        // already emits a `warn!` on miss.
        let passphrase = match resolve_db_passphrase(&ctx.credentials) {
            Ok(passphrase) => Some(passphrase),
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.curator",
                    %error,
                    "Falling back to in-memory / no-heal mode. Curator data will not persist across restarts."
                );
                None
            }
        };
        let heal_enabled = passphrase.is_some();
        let stores = open_curator_stores(Some(db_path.as_str()), passphrase.as_deref());
        let this = Self {
            stores: RwLock::new(stores),
            db_path: Some(db_path),
            passphrase,
            heal_attempt_logged: AtomicBool::new(false),
            heal_enabled,
            last_heal_attempt: std::sync::Mutex::new(None),
        };
        if Self::db_level_down_from(&this.stores) {
            if this.heal_enabled {
                tracing::error!(
                    target: "hkask.mcp.curator",
                    db_path = ?this.db_path,
                    "Curator DB unavailable — ALL curator stores (escalations, \
                     regulation archive, memory, token registry) \
                     are down. Every tool call re-attempts the open \
                     (self-healing); check that no other process holds the \
                     SQLCipher lock and that HKASK_DB_PASSPHRASE matches the \
                     keychain."
                );
            } else {
                // No passphrase — healing can't succeed, so say so rather
                // than promising re-attempts that will never happen.
                tracing::error!(
                    target: "hkask.mcp.curator",
                    db_path = ?this.db_path,
                    "Curator DB unavailable and HKASK_DB_PASSPHRASE is not \
                     set — curator stores will stay down until the server is \
                     restarted with the passphrase configured. Set \
                     HKASK_DB_PASSPHRASE (keychain-provisioned) and relaunch."
                );
            }
        }
        this
    }

    /// True when the DB-open level failed (all four stores `None`) — the
    /// case a re-open can fix. Partial degradation (a per-store `from_driver`
    /// failure leaving some stores `Some`) is NOT healable by re-open and
    /// must not churn re-opens on every tool call.
    /// Construct a `CuratorDb` directly from pre-built stores — the test
    /// seam. Healing is disabled (no path, no passphrase), mirroring the
    /// "tests construct handles with no valid path" contract noted on
    /// `heal_enabled`. `#[doc(hidden)]` because this exists for the
    /// `tests/tool_behavior.rs` integration suite, not for downstream
    /// consumers — the production path is `from_context`.
    #[doc(hidden)]
    pub fn from_stores(stores: CuratorStores) -> Self {
        Self {
            stores: RwLock::new(stores),
            db_path: None,
            passphrase: None,
            heal_attempt_logged: AtomicBool::new(false),
            heal_enabled: false,
            last_heal_attempt: std::sync::Mutex::new(None),
        }
    }

    fn db_level_down(stores: &CuratorStores) -> bool {
        stores.all_none()
    }

    fn db_level_down_from(stores: &RwLock<CuratorStores>) -> bool {
        match stores.read() {
            Ok(guard) => Self::db_level_down(&guard),
            Err(_) => true,
        }
    }

    /// Read the current store set, attempting a re-open when the DB-level
    /// open has failed.
    fn get(&self) -> CuratorStores {
        if self.heal_enabled && Self::db_level_down_from(&self.stores) && self.heal_due() {
            self.try_heal();
        }
        match self.stores.read() {
            Ok(guard) => guard.clone(),
            Err(_) => CuratorStores::empty(),
        }
    }

    /// Gate heal re-open attempts to at most one per `HEAL_RETRY_INTERVAL`.
    fn heal_due(&self) -> bool {
        let now = std::time::Instant::now();
        let mut last = self
            .last_heal_attempt
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(prev) = *last
            && now.duration_since(prev) < HEAL_RETRY_INTERVAL
        {
            return false;
        }
        *last = Some(now);
        true
    }

    fn try_heal(&self) {
        let fresh = open_curator_stores(self.db_path.as_deref(), self.passphrase.as_deref());
        let fresh_ok = !Self::db_level_down(&fresh);
        match self.stores.write() {
            Ok(mut guard) => {
                let was_down = Self::db_level_down(&guard);
                if fresh_ok && was_down {
                    *guard = fresh;
                    tracing::info!(
                        target: "hkask.mcp.curator",
                        db_path = ?self.db_path,
                        "Curator DB healed — curator stores restored"
                    );
                    self.heal_attempt_logged.store(false, Ordering::Relaxed);
                } else if !fresh_ok && !self.heal_attempt_logged.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        target: "hkask.mcp.curator",
                        db_path = ?self.db_path,
                        "Curator DB still unavailable after re-open attempt — \
                         curator tools will keep returning permission_denied"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: "hkask.mcp.curator",
                    error = %e,
                    "Curator DB stores lock poisoned — cannot attempt heal"
                );
            }
        }
    }
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
    }
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
                    "escalation_queue": stores.escalation_queue.is_some(),
                    "regulation_store": stores.regulation_store.is_some(),
                    "memory": stores.memory.is_some(),
                }
            }))
        })
        .await
    }

    // ── Escalation Management ──────────────────────────────────────────

    #[tool(description = "List all pending escalations requiring review")]
    pub async fn curator_escalations(
        &self,
        Parameters(_req): Parameters<PingRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_escalations", async {
            let stores = self.db.get();
            let queue = stores.escalation_queue()?;
            match governance::list_escalations_direct(queue) {
                Ok(entries) => {
                    let serialized: Vec<serde_json::Value> = entries
                        .iter()
                        .map(|e| {
                            json!({
                                "id": e.id,
                                "template_id": e.template_id,
                                "bot_id": e.bot_id,
                                "output": e.output,
                                "confidence": e.confidence,
                                "retry_count": e.retry_count,
                                "error_context": e.error_context,
                                "created_at": e.created_at,
                                "status": e.status,
                                "resolved_at": e.resolved_at,
                                "resolved_by": e.resolved_by,
                            })
                        })
                        .collect();
                    Ok(json!({"count": serialized.len(), "escalations": serialized}))
                }
                Err(e) => Err(to_tool_error(e)),
            }
        })
        .await
    }

    #[tool(description = "Resolve an escalation by ID")]
    pub async fn curator_escalation_resolve(
        &self,
        Parameters(req): Parameters<EscalationResolveRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_escalation_resolve", async {
            let stores = self.db.get();
            let queue = stores.escalation_queue()?;
            let events_store = stores.regulation_store()?;
            let events: Arc<dyn RegulationSink> =
                Arc::clone(events_store) as Arc<dyn RegulationSink>;
            // Attribution is server-side: the MCP request carries no caller
            // identity. The resolution note is recorded in the Regulation
            // event so the audit trail keeps it.
            match governance::resolve_direct(
                queue,
                &events,
                &req.id,
                "curator",
                Some(&req.resolution),
            ) {
                Ok(()) => Ok(json!({"resolved": true, "id": req.id})),
                Err(e) => Err(to_tool_error(e)),
            }
        })
        .await
    }

    #[tool(description = "Dismiss an escalation as not actionable")]
    pub async fn curator_escalation_dismiss(
        &self,
        Parameters(req): Parameters<EscalationDismissRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_escalation_dismiss", async {
            let stores = self.db.get();
            let queue = stores.escalation_queue()?;
            let events_store = stores.regulation_store()?;
            let events: Arc<dyn RegulationSink> =
                Arc::clone(events_store) as Arc<dyn RegulationSink>;
            match governance::dismiss_direct(queue, &events, &req.id, "curator", Some(&req.reason))
            {
                Ok(()) => Ok(json!({"dismissed": true, "id": req.id})),
                Err(e) => Err(to_tool_error(e)),
            }
        })
        .await
    }

    #[tool(
        description = "Dismiss all pending escalations matching an exact output string. Used to clear runaway escalation floods from a single broken feedback loop in one operation. Returns the count of dismissed escalations."
    )]
    pub async fn curator_escalation_dismiss_by_pattern(
        &self,
        Parameters(req): Parameters<EscalationDismissByPatternRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_escalation_dismiss_by_pattern", async {
            let stores = self.db.get();
            let queue = stores.escalation_queue()?;
            let events_store = stores.regulation_store()?;
            let events: Arc<dyn RegulationSink> =
                Arc::clone(events_store) as Arc<dyn RegulationSink>;
            match governance::dismiss_by_pattern_direct(
                queue,
                &events,
                &req.output,
                "curator",
                Some(&req.reason),
            ) {
                Ok(count) => Ok(json!({"dismissed": true, "count": count, "output": req.output})),
                Err(e) => Err(to_tool_error(e)),
            }
        })
        .await
    }

    // ── Memory & Learning ──────────────────────────────────────────────

    /// Embed a recall query and resolve the nearest stored h_mems by cosine
    /// similarity. Returns `(h_mem, distance)` pairs, most similar first.
    /// Each distinct entity contributes at most `MAX_FRAGMENTS_PER_ENTITY`
    /// fragments (its freshest), and no h_mem appears twice even when the
    /// KNN hits it through several embeddings.
    /// `Err(reason)` when the query cannot be embedded (no IPC bridge, no
    /// embedding provider) or the store has no embedding index — callers fall
    /// back to exact-entity lookup and surface the reason.
    async fn semantic_recall_fragments(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(hkask_storage::HMem, f64)>, String> {
        let stores = self.db.get();
        let memory = stores
            .memory()
            .map_err(|e| format!("curator memory unavailable: {e}"))?;
        let embedding_model = hkask_inference::model_constants::embedding_model();
        let vectors = self
            .inference_port
            .embed(&embedding_model, &[query.to_string()])
            .await
            .map_err(|e| format!("embedding the recall query failed: {e}"))?;
        let query_vector = vectors
            .into_iter()
            .next()
            .ok_or_else(|| "embedding model returned no vector for the recall query".to_string())?;
        // Fetch more KNN neighbors than the fragment limit: each distinct
        // entity contributes at most MAX_FRAGMENTS_PER_ENTITY fragments, and
        // the same entity holds one embedding per turn, so a 1:1 KNN limit
        // under-fills the result set once capping bites.
        let knn_limit = limit.saturating_mul(MAX_FRAGMENTS_PER_ENTITY).max(limit);
        let results = memory
            .search_similar(&query_vector, knn_limit)
            .map_err(|e| format!("semantic search over curator memory failed: {e}"))?;
        let mut fragments = Vec::with_capacity(results.len());
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
                    // Freshest first: the newest turn under the entity is the
                    // closest thing it has to current state.
                    h_mems.sort_by_key(|h_mem| std::cmp::Reverse(h_mem.observed_at));
                    for h_mem in h_mems {
                        if per_entity_counts.get(&entity_ref).copied().unwrap_or(0)
                            >= MAX_FRAGMENTS_PER_ENTITY
                        {
                            break;
                        }
                        // Several KNN hits can resolve to the same h_mems
                        // (multiple embeddings under one entity) — no
                        // duplicate fragments.
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
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.mcp.curator",
                        error = %e,
                        entity_ref = %entity_ref,
                        "failed to resolve KNN hit to its h_mem — skipping (non-fatal)"
                    );
                }
            }
        }
        fragments.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(fragments)
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
                Ok(fragments) if !fragments.is_empty() => {
                    let serialized: Vec<serde_json::Value> = fragments
                        .iter()
                        .take(limit)
                        .map(|(t, distance)| {
                            json!({
                                "entity": t.entity, "attribute": t.attribute,
                                "value": t.value, "confidence": t.confidence,
                                "distance": distance,
                            })
                        })
                        .collect();
                    Ok(json!({
                        "count": serialized.len(),
                        "mode": "semantic",
                        "results": serialized,
                    }))
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
                        other => McpToolError::internal(format!("Semantic recall failed: {other}")), // rr0044-ok: fallback arm of per-variant match
                    })?;
                    let serialized: Vec<serde_json::Value> = exact
                        .iter()
                        .take(limit)
                        .map(|t| {
                            json!({
                                "entity": t.entity, "attribute": t.attribute,
                                "value": t.value, "confidence": t.confidence,
                            })
                        })
                        .collect();
                    Ok(json!({
                        "count": serialized.len(),
                        "mode": "entity_exact",
                        "note": format!("semantic recall unavailable — fell back to exact-entity lookup: {reason}"),
                        "results": serialized,
                    }))
                }
                Ok(_) => Ok(json!({
                    "count": 0,
                    "mode": "semantic",
                    "results": [],
                })),
            }
        })
        .await
    }

    #[tool(
        description = "Recall the Curator's memory about an entity. Set `recall_shape` to `perspective_scoped` (curator's own turns) or `entity_wide` (all h_mems for the entity) or `both`. Set `ontology_axis` (dc_type | dc_subject | pko_procedure | ontology_namespace) plus `ontology_value` to recall along the dual-axis ontology instead of the entity — e.g. every step of a PKO procedure, or every h_mem tagged by a domain ontology namespace."
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
                        json!({
                            "entity": t.entity, "attribute": t.attribute,
                            "value": t.value, "confidence": t.confidence,
                            "ontology": t.ontology,
                        })
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
                match stores.memory() {
                    Ok(ep) => match ep.query_for_deduped(&req.entity, self.webid) {
                        Ok(h_mems) => {
                            let s: Vec<serde_json::Value> = h_mems
                                .iter()
                                .map(|t| {
                                    json!({
                                        "entity": t.entity, "attribute": t.attribute,
                                        "value": t.value, "confidence": t.confidence,
                                        "valid_from": t.observed_at.to_rfc3339(),
                                    })
                                })
                                .collect();
                            result["perspective_scoped"] = json!({"count": s.len(), "h_mems": s});
                        }
                        Err(e) => {
                            result["perspective_scoped"] = json!({"error": format!("{e}")});
                        }
                    },
                    Err(_) => {
                        result["perspective_scoped"] = json!({"status": "unavailable"});
                    }
                }
            }
            if recall_shape == MemoryRecallType::EntityWide
                || recall_shape == MemoryRecallType::Both
            {
                match stores.memory() {
                    Ok(sem) => match sem.query_deduped(&req.entity) {
                        Ok(h_mems) => {
                            let s: Vec<serde_json::Value> = h_mems
                                .iter()
                                .map(|t| {
                                    json!({
                                        "entity": t.entity, "attribute": t.attribute,
                                        "value": t.value, "confidence": t.confidence,
                                    })
                                })
                                .collect();
                            result["entity_wide"] = json!({"count": s.len(), "h_mems": s});
                        }
                        Err(e) => {
                            result["entity_wide"] = json!({"error": format!("{e}")});
                        }
                    },
                    Err(_) => {
                        result["entity_wide"] = json!({"status": "unavailable"});
                    }
                }
            }
            Ok(result)
        })
        .await
    }

    // ── Consultation (Slice 8 — curator-as-callable-tool) ────────────────

    /// Consult the curator's memory with a question. A swarm agent calls
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
                Ok(fragments) if !fragments.is_empty() => {
                    // Entity-wide — the curator's consolidated knowledge:
                    // every KNN-resolved h_mem regardless of who wrote it.
                    let entity_wide: Vec<serde_json::Value> = fragments
                        .iter()
                        .take(limit)
                        .map(|(t, distance)| {
                            json!({
                                "entity": t.entity,
                                "attribute": t.attribute,
                                "value": t.value,
                                "confidence": t.confidence,
                                "distance": distance,
                            })
                        })
                        .collect();
                    result["entity_wide_fragments"] = json!({
                        "count": entity_wide.len(),
                        "h_mems": entity_wide,
                    });

                    // Perspective-scoped — the curator's own turns: the same
                    // semantic hits filtered to h_mems the curator wrote.
                    let perspective_scoped: Vec<serde_json::Value> = fragments
                        .iter()
                        .filter(|(t, _)| t.access.perspective == Some(self.webid))
                        .take(limit)
                        .map(|(t, distance)| {
                            json!({
                                "entity": t.entity,
                                "attribute": t.attribute,
                                "value": t.value,
                                "confidence": t.confidence,
                                "distance": distance,
                            })
                        })
                        .collect();
                    result["perspective_scoped_fragments"] = json!({
                        "count": perspective_scoped.len(),
                        "h_mems": perspective_scoped,
                    });
                }
                // Degradation, not a silent fallback — surface why semantic
                // recall is unavailable, then fall back to the exact-entity
                // lookup (which only matches when the query IS an entity).
                Err(reason) => {
                    // Degradation, not a silent fallback — surface why semantic
                    // recall is unavailable, then fall back to the exact-entity
                    // lookup (which only matches when the query IS an entity).
                    // The note is preserved alongside the fallback results so the
                    // operator can distinguish "semantic recall broken" from
                    // "no matching memories" — same pattern as
                    // `curator_semantic_search` (L540-545).
                    match stores.memory() {
                        Ok(sem) => match sem.query_deduped(&req.query) {
                            Ok(h_mems) => {
                                let fragments: Vec<serde_json::Value> = h_mems
                                    .iter()
                                    .take(limit)
                                    .map(|t| {
                                        json!({
                                            "entity": t.entity,
                                            "attribute": t.attribute,
                                            "value": t.value,
                                            "confidence": t.confidence,
                                        })
                                    })
                                    .collect();
                                result["entity_wide_fragments"] = json!({
                                    "count": fragments.len(),
                                    "mode": "entity_exact",
                                    "note": format!("semantic recall unavailable — fell back to exact-entity lookup: {reason}"),
                                    "h_mems": fragments,
                                });
                            }
                            Err(e) => {
                                result["entity_wide_fragments"] =
                                    json!({"error": format!("{e}")});
                            }
                        },
                        Err(_) => {
                            result["entity_wide_fragments"] =
                                json!({"status": "unavailable"});
                        }
                    }
                    match stores.memory() {
                        Ok(ep) => match ep.query_for_deduped(&req.query, self.webid) {
                            Ok(h_mems) => {
                                let fragments: Vec<serde_json::Value> = h_mems
                                    .iter()
                                    .take(limit)
                                    .map(|t| {
                                        json!({
                                            "entity": t.entity,
                                            "attribute": t.attribute,
                                            "value": t.value,
                                            "confidence": t.confidence,
                                            "valid_from": t.observed_at.to_rfc3339(),
                                        })
                                    })
                                    .collect();
                                result["perspective_scoped_fragments"] = json!({
                                    "count": fragments.len(),
                                    "mode": "entity_exact",
                                    "note": format!("semantic recall unavailable — fell back to exact-entity lookup: {reason}"),
                                    "h_mems": fragments,
                                });
                            }
                            Err(e) => {
                                result["perspective_scoped_fragments"] =
                                    json!({"error": format!("{e}")});
                            }
                        },
                        Err(_) => {
                            result["perspective_scoped_fragments"] =
                                json!({"status": "unavailable"});
                        }
                    }
                }
                Ok(_) => {
                    result["entity_wide_fragments"] = json!({
                        "count": 0,
                        "h_mems": [],
                    });
                    result["perspective_scoped_fragments"] = json!({
                        "count": 0,
                        "h_mems": [],
                    });
                }
            }

            Ok(result)
        })
        .await
    }

    // ── Algedonic History ──────────────────────────────────────────────

    #[tool(description = "Read algedonic event log for a time window")]
    pub async fn curator_algedonic_log(
        &self,
        Parameters(req): Parameters<AlgedonicLogRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_algedonic_log", async {
            let stores = self.db.get();
            let store = stores.regulation_store()?;
            let hours = req.hours.unwrap_or(24);
            let since = chrono::Utc::now() - chrono::Duration::hours(hours as i64);
            match store.query_algedonic(since, 500) {
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
                    Ok(json!({"window_hours": hours, "count": s.len(), "events": s}))
                }
                Err(e) => Err(map_infra_error(&e, "Algedonic query failed")),
            }
        })
        .await
    }

    // ── Regulation Query (for platform governance transparency) ────────────────

    #[tool(
        description = "Query Regulation regulation records by namespace prefix within a time window. Returns structured event data for governance transparency reporting and consent auditing."
    )]
    pub async fn reg_query(
        &self,
        Parameters(req): Parameters<RegQueryRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reg_query", async {
            let stores = self.db.get();
            let store = stores.regulation_store()?;
            let window_secs = req.window_seconds.unwrap_or(3600);
            let limit = req.limit.unwrap_or(100) as u64;
            let since = chrono::Utc::now() - chrono::Duration::seconds(window_secs as i64);
            let config = hkask_storage::DecayConfig::default();

            let weighted = store
                .replay_weighted(since, limit, &config)
                .map_err(|e| map_infra_error(&e, "Regulation query failed"))?;
            let replayed_count = weighted.len();
            let filtered: Vec<serde_json::Value> = weighted
                .into_iter()
                .filter(|we| {
                    if let Some(ref ns) = req.namespace {
                        we.event.span.namespace.as_str().starts_with(ns)
                    } else {
                        true
                    }
                })
                // No `.take(limit)` here — `replay_weighted` already applied
                // the SQL `limit` before the namespace filter, so an in-memory
                // `take` can only reduce the already-capped set (dead code)
                // and misleads readers into thinking the limit is enforced
                // post-filter. If post-filter limiting is needed, move the
                // limit into the SQL query (after the namespace filter).
                .map(|we| {
                    json!({
                        "timestamp": we.event.timestamp.to_rfc3339(),
                        "namespace": we.event.span.namespace.as_str(),
                        "path": we.event.span.path,
                        "phase": format!("{:?}", we.event.phase),
                        "weight": we.weight,
                        "observation": we.event.observation,
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
                // The replay applies the SQL limit before the namespace
                // filter, so `replayed_count` is the post-limit, post-weight
                // count — NOT the total number of events in the window for
                // `namespace`.
                "replayed_count": replayed_count,
                "filtered_count": filtered.len(),
                "events": filtered
            }))
        })
        .await
    }

    // ── Skill-use issue reporting (Co-evolution Phase 2) ────────────────

    /// Report a skill-use issue — submitted by a skill's `on_failure` config
    /// when an MCP tool call fails or produces unexpected output. The report
    /// is stored as an h_mem in the curator's memory store with
    /// entity `skill_use_issue:<skill_name>` so it is queryable via
    /// `curator_memory_recall` and `curator_semantic_search`.
    ///
    /// This is the skill-reported input channel of the co-evolution loop:
    /// skills report MCP tool issues → the Curator analyzes patterns →
    /// CuratorDirectives evolve the MCP tool (add validation, improve error
    /// messages, add fallbacks). Complements the existing runtime telemetry
    /// (reg.* spans, algedonic events).
    #[tool(
        description = "Report a skill-use issue when an MCP tool call fails or produces unexpected output. Stored as an h_mem for Curator pattern analysis. The report includes: skill name, tool name, step ordinal, error description, optional tool input, and optional failure type classification."
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

            let report_value = json!({
                "skill_name": req.skill_name,
                "tool_name": req.tool_name,
                "step_ordinal": req.step_ordinal,
                "error": req.error,
                "tool_input": req.tool_input,
                "failure_type": req.failure_type,
                "reported_at": now.to_rfc3339(),
            });

            let h_mem = hkask_storage::HMem::new(
                &entity,
                &format!("tool_failure:{}", req.tool_name),
                report_value,
                self.webid,
            );

            memory
                .store(h_mem)
                .map_err(|e| map_memory_store_error(e, "Failed to store skill-use issue report"))?;

            RegulationSpan::Curation.emit("skill_use_issue_reported");

            Ok(json!({
                "reported": true,
                "entity": entity,
                "skill_name": req.skill_name,
                "tool_name": req.tool_name,
                "step_ordinal": req.step_ordinal,
                "failure_type": req.failure_type,
                "guidance": "The issue has been recorded in the curator's memory store. Use curator_memory_recall with entity 'skill_use_issue:<skill_name>' to retrieve accumulated reports."
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

            // Parse the evidence h_mem ID.
            let evidence_id = req
                .evidence_h_mem_id
                .parse::<hkask_storage::HMemId>()
                .map_err(|e| {
                    McpToolError::invalid_argument(format!(
                        "Invalid evidence_h_mem_id '{id}': {e}",
                        id = req.evidence_h_mem_id
                    ))
                })?;

            // Verify the evidence h_mem exists — by ID, not by entity ref:
            // `query_deduped_untouched` is entity-keyed and no entity is a
            // bare UUID, so the previous entity-keyed lookup rejected every
            // citation and the tool could never insert.
            let evidence = memory
                .get_by_id(&evidence_id)
                .map_err(|e| {
                    map_memory_store_error(e, "Failed to verify evidence h_mem")
                })?;
            if evidence.is_none() {
                return Err(McpToolError::invalid_argument(format!(
                    "Evidence h_mem '{id}' not found — memory_insert requires an existing citation",
                    id = req.evidence_h_mem_id
                )));
            }

            // Build the h_mem with confidence floor 0.5.
            let mut value = req.value;
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
            .with_confidence(hkask_types::Confidence::new(0.5));

            memory
                .store(h_mem)
                .map_err(|e| {
                    map_memory_store_error(e, "Failed to store curator memory")
                })?;

            RegulationSpan::Curation.emit("memory_inserted");

            Ok(json!({
                "inserted": true,
                "entity": req.entity,
                "attribute": req.attribute,
                "confidence": 0.5,
                "evidence_h_mem_id": req.evidence_h_mem_id,
                "guidance": "Memory stored at confidence 0.5. Use memory_update to adjust confidence after outcome observation. Use curator_memory_recall with this entity to retrieve."
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

            let h_mem_id = req
                .h_mem_id
                .parse::<hkask_storage::HMemId>()
                .map_err(|e| {
                    McpToolError::invalid_argument(format!(
                        "Invalid h_mem_id '{id}': {e}",
                        id = req.h_mem_id
                    ))
                })?;

            // Fetch the existing h_mem to get its current value and confidence.
            let existing = memory
                .query_deduped_untouched(&h_mem_id.to_string())
                .map_err(|e| {
                    map_memory_store_error(e, "Failed to fetch h_mem for update")
                })?;
            let existing_h_mem = existing.into_iter().next().ok_or_else(|| {
                McpToolError::not_found(format!(
                    "h_mem '{id}' not found",
                    id = req.h_mem_id
                ))
            })?;

            // Bayesian-combine the new confidence with the existing one.
            let new_confidence_raw = hkask_types::Confidence::new(req.new_confidence);
            let combined = hkask_memory::combine_confidences(
                existing_h_mem.confidence,
                new_confidence_raw,
            );

            // Use the new value if provided, otherwise keep the existing.
            let value = req.new_value.unwrap_or_else(|| existing_h_mem.value.clone());

            memory
                .update_confidence(&h_mem_id, value, combined)
                .map_err(|e| {
                    map_memory_store_error(e, "Failed to update h_mem confidence")
                })?;

            RegulationSpan::Curation.emit("memory_updated");

            Ok(json!({
                "updated": true,
                "h_mem_id": req.h_mem_id,
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
    /// in the memory store by expiring, updating, or deleting contradictory
    /// h_mems.
    #[tool(
        description = "Resolve a contradiction between memories. Strategies: 'expire' (soft-delete), 'update_confidence' (lower confidence), 'delete' (hard-delete). Requires a reason citing the contradiction."
    )]
    pub async fn memory_resolve_contradiction(
        &self,
        Parameters(req): Parameters<MemoryResolveContradictionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "memory_resolve_contradiction", async {
            let stores = self.db.get();
            let memory = stores.memory()?;

            let target_id = req
                .target_h_mem_id
                .parse::<hkask_storage::HMemId>()
                .map_err(|e| {
                    McpToolError::invalid_argument(format!(
                        "Invalid target_h_mem_id '{id}': {e}",
                        id = req.target_h_mem_id
                    ))
                })?;

            // Verify the target exists.
            let target = memory
                .query_deduped_untouched(&target_id.to_string())
                .map_err(|e| map_memory_store_error(e, "Failed to fetch target h_mem"))?;
            if target.is_empty() {
                return Err(McpToolError::not_found(format!(
                    "Target h_mem '{id}' not found",
                    id = req.target_h_mem_id
                )));
            }

            match req.strategy.as_str() {
                "expire" => {
                    memory
                        .expire_h_mem(&target_id)
                        .map_err(|e| map_memory_store_error(e, "Failed to expire h_mem"))?;
                    RegulationSpan::Curation.emit("contradiction_expired");
                    Ok(json!({
                        "resolved": true,
                        "strategy": "expire",
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
                    let target_h_mem = target.into_iter().next().ok_or_else(|| {
                        McpToolError::not_found("Target h_mem disappeared between fetch and update")
                    })?;
                    let confidence = hkask_types::Confidence::new(new_confidence);
                    memory
                        .update_confidence(&target_id, target_h_mem.value, confidence)
                        .map_err(|e| {
                            map_memory_store_error(e, "Failed to update h_mem confidence")
                        })?;
                    RegulationSpan::Curation.emit("contradiction_confidence_lowered");
                    Ok(json!({
                        "resolved": true,
                        "strategy": "update_confidence",
                        "target_h_mem_id": req.target_h_mem_id,
                        "new_confidence": new_confidence,
                        "contradicting_h_mem_ids": req.h_mem_ids,
                        "reason": req.reason
                    }))
                }
                "delete" => {
                    memory
                        .delete_h_mem(&target_id)
                        .map_err(|e| map_memory_store_error(e, "Failed to delete h_mem"))?;
                    RegulationSpan::Curation.emit("contradiction_deleted");
                    Ok(json!({
                        "resolved": true,
                        "strategy": "delete",
                        "target_h_mem_id": req.target_h_mem_id,
                        "contradicting_h_mem_ids": req.h_mem_ids,
                        "reason": req.reason
                    }))
                }
                other => Err(McpToolError::invalid_argument(format!(
                    "Unknown strategy '{other}' — must be one of: expire, update_confidence, delete"
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
        description = "Prune curator h_mems older than max_age_days. Hard-deletes aged h_mems, optionally sparing those recalled within spare_recalled_within_days. Deterministic, non-LLM. Distinct from confidence-based consolidation."
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

            let outcome = memory
                .prune_by_age(req.max_age_days, req.spare_recalled_within_days)
                .map_err(|e| map_memory_store_error(e, "Age-based prune failed"))?;

            RegulationSpan::Curation.emit("memory_pruned");

            Ok(json!({
                "pruned": true,
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
    /// expires the rest. Non-string values skipped.
    #[tool(
        description = "Deduplicate curator h_mems by normalized string value. Groups by (entity, attribute, normalized_value), keeps highest-confidence, expires the rest. Deterministic, non-LLM. Non-string values skipped."
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
                "expired_count": outcome.expired_count,
                "failed_count": outcome.failed_count,
                "skipped_non_string": outcome.skipped_non_string,
            }))
        })
        .await
    }

    /// Extract candidate semantic memories from a thread's turn history.
    /// Queries the curator's memory for all h_mems with entity
    /// `chat:thread:<thread_id>`, returns their IDs and content as
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

            // Query both entity prefixes: curator-perspective turns are stored
            // under `chat:thread:<id>` (curator turns only), and shared copies
            // of all turns (including non-curator) are under `curator:thread:<id>`.
            // Without both, non-curator turns are invisible to extraction.
            let chat_prefix = format!("chat:thread:{}", req.thread_id);
            let curator_prefix = format!("curator:thread:{}", req.thread_id);
            let mut h_mems = memory
                .h_mems_by_entity_prefix(&chat_prefix)
                .map_err(|e| map_memory_store_error(e, "Failed to query curator-perspective thread turns"))?;
            h_mems.extend(
                memory
                    .h_mems_by_entity_prefix(&curator_prefix)
                    .map_err(|e| map_memory_store_error(e, "Failed to query shared thread turns"))?,
            );

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
}

// ── Server startup ─────────────────────────────────────────────────────

/// Map a governance `ServiceError` to the structured MCP wire error,
/// not_found) instead of flattening everything to `internal`.
fn to_tool_error(e: ServiceError) -> McpToolError {
    match e.kind() {
        ErrorKind::NotFound => McpToolError::not_found(e.to_string()),
        _ => McpToolError::internal(e.to_string()), // rr0044-ok: mapper-fallback
    }
}

pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    // Construct the inference port before entering the sync server-
    // construction closure. `resolve_inference_port` is async (it constructs
    // a `LazyInferencePort` — the bridge connection itself is deferred to
    // each `embed()` call, which re-tries `InferenceIpcClient::from_env()`);
    // the closure passed to `run_server` is sync, so the await must happen
    // here. Used by `curator_semantic_search` and `curator_consult` to embed
    // recall queries.
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
            );
            Ok(CuratorServer::new(ctx.webid, db, inference_port.clone()))
        },
        vec![hkask_mcp_server::CredentialRequirement::optional(
            "HKASK_DB_PASSPHRASE",
            "SQLCipher encryption passphrase",
        )],
    )
    .await
}

/// Open the curator's sovereign `curator.db` and construct all four stores from
/// a single shared driver. Called at construction and on every heal attempt.
/// All-or-nothing on the DB-open steps (a failure before store construction
/// returns all `None`s); per-store `from_driver` failures degrade only that
/// store.
fn open_curator_stores(db_path: Option<&str>, passphrase: Option<&str>) -> CuratorStores {
    let Some(db_path) = db_path else {
        tracing::warn!(target: "hkask.mcp.curator", "Curator DB path not resolved");
        return CuratorStores::empty();
    };
    let Some(passphrase) = passphrase else {
        tracing::warn!(target: "hkask.mcp.curator", "HKASK_DB_PASSPHRASE not set");
        return CuratorStores::empty();
    };

    let db = match hkask_storage::open_or_repair(db_path, passphrase) {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.curator", error = %e, "Failed to open curator DB");
            return CuratorStores::empty();
        }
    };
    let pool = match db.sqlite_pool() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.curator", error = %e, "Failed to get SQLite pool");
            return CuratorStores::empty();
        }
    };
    let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
        Arc::new(SqliteDriver::new(pool));
    let embedding_dim = hkask_storage::embedding_dim();
    let embedding_store = match hkask_storage::EmbeddingStore::from_driver(
        Arc::clone(&driver),
        embedding_dim,
    ) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.curator", error = %e, "Failed to create EmbeddingStore — semantic recall degraded");
            None
        }
    };

    // Memory degrades independently of the escalation/regulation/token stores
    // below — a memory failure must not take down the escalation queue and
    // regulation archive with it.
    //
    // An unavailable EmbeddingStore must NOT disable curator memory: the
    // semantic tools (`curator_semantic_search`, `curator_consult`) degrade
    // to exact-entity lookup (surfaced in the tool output), and
    // `curator_memory_recall` recalls by entity/EAV regardless. Before the
    // store unification the h_mem half survived an embedding failure because
    // it was a separate handle; falling back to the embedding-free
    // constructor preserves that degradation boundary instead of coupling
    // all recall to the embedding index.
    let memory = match hkask_storage::HMemStore::from_driver(Arc::clone(&driver)) {
        Ok(h_mem_store) => match embedding_store {
            Some(embeddings) => Some(Arc::new(hkask_memory::MemoryStore::new(
                h_mem_store,
                embeddings,
            ))),
            None => match hkask_memory::MemoryStore::try_new_without_embeddings(h_mem_store) {
                Ok(store) => {
                    tracing::warn!(
                        target: "hkask.mcp.curator",
                        "EmbeddingStore unavailable — curator memory opened without \
                         embeddings; entity/EAV recall works, vector similarity does not"
                    );
                    Some(Arc::new(store))
                }
                Err(e) => {
                    tracing::warn!(target: "hkask.mcp.curator", error = %e, "Failed to open curator memory without embeddings — curator recall degraded");
                    None
                }
            },
        },
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.curator", error = %e, "Failed to create HMemStore — curator recall degraded");
            None
        }
    };
    let escalation_queue = match hkask_storage::EscalationQueue::from_driver(Arc::clone(&driver)) {
        Ok(q) => Some(Arc::new(q)),
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.curator", error = %e, "Failed to create EscalationQueue");
            None
        }
    };
    let regulation_store = match hkask_storage::RegulationArchive::from_driver(Arc::clone(&driver))
    {
        Ok(store) => Some(Arc::new(store)),
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.curator", error = %e, "Failed to create RegulationArchive");
            None
        }
    };
    CuratorStores {
        escalation_queue,
        regulation_store,
        memory,
    }
}

#[cfg(test)]
mod tests {
    // NOTE: The startup requirement is pinned by a single in-file `//`
    // comment, not `///` — the module contains no items that would justify
    // a doc comment.
    //
    // `CuratorDb::from_context` resolves `HKASK_DB_PASSPHRASE` via the
    // canonical 2-tier chain (ctx.credentials → resolve_credential → env →
    // keychain) and only falls back to None (in-memory / no-heal) on miss.
    // The `CredentialRequirement::optional` declaration calls out the same
    // var so server bootstrap warns loudly rather than silently degrade.
    // The pin is the shared helper call in `from_context` (`resolve_db_passphrase`)
    // — same helper used by the other DB-backed MCP servers. This is a
    // comment-only test module: if the comment drifts from the code, it
    // compiles stale.
}
