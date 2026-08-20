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
//! cross-pod semantic search, memory recall, spec drift detection,
//! and algedonic event history.

pub mod governance;
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
    /// The curator's unified memory. One store holds both the curator's
    /// episodic step-execution records and its semantic facts — the
    /// `HMemOntology` blob on each h_mem distinguishes them (P5.4), so no
    /// second store handle is needed. The `curator_memory_recall`
    /// `memory_type` parameter stays as a recall-shape selector (perspective-
    /// scoped vs entity-wide), not a store selector.
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
                     regulation archive, episodic, semantic, token registry) \
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

    /// Construct a handle over pre-built stores — healing disabled. Used by
    /// the qa_contract integration tests (compiled as a separate crate, so
    /// `#[cfg(test)]` doesn't reach them).
    #[doc(hidden)]
    pub fn for_tests(stores: CuratorStores) -> Self {
        Self {
            stores: RwLock::new(stores),
            db_path: None,
            passphrase: None,
            heal_attempt_logged: AtomicBool::new(false),
            heal_enabled: false,
            last_heal_attempt: std::sync::Mutex::new(None),
        }
    }

    /// Replace the stores — simulates an outage or a heal. Test-only.
    #[doc(hidden)]
    pub fn set_for_tests(&self, stores: CuratorStores) {
        if let Ok(mut guard) = self.stores.write() {
            *guard = stores;
        }
    }

    /// True when the DB-open level failed (all four stores `None`) — the
    /// case a re-open can fix. Partial degradation (a per-store `from_driver`
    /// failure leaving some stores `Some`) is NOT healable by re-open and
    /// must not churn re-opens on every tool call.
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
    }
);

#[tool_router(server_handler)]
impl CuratorServer {
    // ── Liveness ───────────────────────────────────────────────────────

    #[tool(description = "Liveness check")]
    pub async fn curator_ping(&self, Parameters(_req): Parameters<PingRequest>) -> String {
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
    pub async fn curator_escalations(&self, Parameters(_req): Parameters<PingRequest>) -> String {
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
    ) -> String {
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
    ) -> String {
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

    // ── Memory & Learning ──────────────────────────────────────────────

    #[tool(description = "Query the Curator's semantic memory by entity name")]
    pub async fn curator_semantic_search(
        &self,
        Parameters(req): Parameters<SemanticSearchRequest>,
    ) -> String {
        execute_tool(self, "curator_semantic_search", async {
            let stores = self.db.get();
            let memory = stores.memory()?;
            match memory.query_deduped(&req.query) {
                Ok(h_mems) => {
                    let limit = req.limit.unwrap_or(10);
                    let serialized: Vec<serde_json::Value> = h_mems
                        .iter()
                        .take(limit)
                        .map(|t| {
                            json!({
                                "entity": t.entity, "attribute": t.attribute,
                                "value": t.value, "confidence": t.confidence,
                            })
                        })
                        .collect();
                    Ok(json!({"count": serialized.len(), "total": h_mems.len(), "results": serialized}))
                }
                Err(e) => Err(match e {
                    hkask_memory::MemoryStoreError::HMem(
                        hkask_storage::HMemError::Infra(ref infra),
                    )
                    | hkask_memory::MemoryStoreError::Embedding(
                        hkask_storage::EmbeddingError::Infrastructure(ref infra),
                    ) => map_infra_error(infra, "Semantic recall failed"),
                    other => McpToolError::internal(format!("Semantic recall failed: {other}")), // rr0044-ok: fallback arm of per-variant match
                }),
            }
        })
        .await
    }

    #[tool(
        description = "Recall the Curator's episodic and semantic memory about an entity. Set `ontology_axis` (dc_type | dc_subject | pko_procedure | ontology_namespace) plus `ontology_value` to recall along the dual-axis ontology instead of the entity — e.g. every step of a PKO procedure, or every h_mem tagged by a domain ontology namespace."
    )]
    pub async fn curator_memory_recall(
        &self,
        Parameters(req): Parameters<MemoryRecallRequest>,
    ) -> String {
        execute_tool(self, "curator_memory_recall", async {
            let memory_type = req.memory_type.as_deref().unwrap_or("both");
            if !matches!(memory_type, "episodic" | "semantic" | "both") {
                return Err(McpToolError::invalid_argument(format!(
                    "unknown memory_type '{memory_type}' — expected 'episodic', 'semantic', or 'both'"
                )));
            }
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

            if memory_type == "episodic" || memory_type == "both" {
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
                            result["episodic"] = json!({"count": s.len(), "h_mems": s});
                        }
                        Err(e) => {
                            result["episodic"] = json!({"error": format!("{e}")});
                        }
                    },
                    Err(_) => {
                        result["episodic"] = json!({"status": "unavailable"});
                    }
                }
            }
            if memory_type == "semantic" || memory_type == "both" {
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
                            result["semantic"] = json!({"count": s.len(), "h_mems": s});
                        }
                        Err(e) => {
                            result["semantic"] = json!({"error": format!("{e}")});
                        }
                    },
                    Err(_) => {
                        result["semantic"] = json!({"status": "unavailable"});
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
    /// curator's sovereign semantic + episodic memory.
    ///
    /// This is a memory-grounded consultation, not a full curator agent
    /// turn — the curator MCP server has no inference port, so it cannot
    /// synthesize a response. It returns the raw memory fragments (semantic
    /// + episodic) matching the query, structured as a consultation. The
    /// calling agent synthesizes the response from the fragments.
    ///
    /// A full inference-grounded response (where the curator agent itself
    /// synthesizes) requires the in-process `CuratorAgentServer`, which
    /// lives in the zed process, not in this MCP server. That path is a
    /// future enhancement (requires a new IPC method + recursion cap +
    /// gas budget).
    #[tool(
        description = "Consult the curator's memory with a question. Returns semantic + episodic memory fragments matching the query. Memory-grounded consultation, not a full curator agent turn."
    )]
    pub async fn curator_consult(
        &self,
        Parameters(req): Parameters<CuratorConsultRequest>,
    ) -> String {
        execute_tool(self, "curator_consult", async {
            let limit = req.limit.unwrap_or(5);
            let stores = self.db.get();
            let mut result = json!({
                "query": req.query,
                "note": "Memory-grounded consultation — raw fragments, not a synthesized response. The calling agent synthesizes."
            });

            // Semantic search — the curator's consolidated knowledge.
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
                        result["semantic_fragments"] = json!({
                            "count": fragments.len(),
                            "h_mems": fragments,
                        });
                    }
                    Err(e) => {
                        result["semantic_fragments"] = json!({"error": format!("{e}")});
                    }
                },
                Err(_) => {
                    result["semantic_fragments"] = json!({"status": "unavailable"});
                }
            }

            // Episodic search — the curator's turn history.
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
                        result["episodic_fragments"] = json!({
                            "count": fragments.len(),
                            "h_mems": fragments,
                        });
                    }
                    Err(e) => {
                        result["episodic_fragments"] = json!({"error": format!("{e}")});
                    }
                },
                Err(_) => {
                    result["episodic_fragments"] = json!({"status": "unavailable"});
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
    ) -> String {
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
    pub async fn reg_query(&self, Parameters(req): Parameters<RegQueryRequest>) -> String {
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
    /// is stored as an episodic h_mem in the curator's memory store with
    /// entity `skill_use_issue:<skill_name>` so it is queryable via
    /// `curator_memory_recall` and `curator_semantic_search`.
    ///
    /// This is the skill-reported input channel of the co-evolution loop:
    /// skills report MCP tool issues → the Curator analyzes patterns →
    /// CuratorDirectives evolve the MCP tool (add validation, improve error
    /// messages, add fallbacks). Complements the existing runtime telemetry
    /// (reg.* spans, algedonic events).
    #[tool(
        description = "Report a skill-use issue when an MCP tool call fails or produces unexpected output. Stored as an episodic h_mem for Curator pattern analysis. The report includes: skill name, tool name, step ordinal, error description, optional tool input, and optional failure type classification."
    )]
    pub async fn curator_report_skill_use_issue(
        &self,
        Parameters(req): Parameters<ReportSkillUseIssueRequest>,
    ) -> String {
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

    // ── Grounding ledger queries (verification ladder Rung 3) ────────────────
    //
    // These tools query the central verification ledger — the cross-tool,
    // cross-server store of grounding records. Every MCP server that
    // delegates to agents writes to this ledger via
    // `VerificationStore::enforce_for_agent()`. The curator surfaces the
    // trends to the operator, closing the cybernetic feedback loop:
    // enforcement → ledger → curator → user → action → improved contracts.

    /// Query the grounding trend from the central verification ledger.
    /// Answers the paper's §4.1 question: "is this getting better?" The
    /// lead metric is `delegations_with_zero_nulled` (deletion-resistant,
    /// paper Rule 5.4). `delegations_without_contract` is the coverage gap
    /// (paper §6: coverage is itself a metric, not a pass).
    #[tool(
        description = "Query the grounding trend from the central verification ledger. Answers the paper's §4.1 question: is this getting better? The lead metric is delegations_with_zero_nulled (deletion-resistant, paper Rule 5.4). delegations_without_contract is the coverage gap (paper §6). Returns Err when the ledger is unavailable (a DB outage must not collapse to an empty trend)."
    )]
    pub async fn curator_grounding_trend(
        &self,
        Parameters(req): Parameters<GroundingTrendToolRequest>,
    ) -> String {
        execute_tool(self, "curator_grounding_trend", async {
            let scope = parse_grounding_scope(
                req.scope.as_deref(),
                req.agent_name.as_deref(),
                req.source.as_deref(),
            );
            let report = self
                .verification_store
                .grounding_trend(&scope)
                .map_err(|e| {
                    McpToolError::unavailable(format!(
                        "grounding trend query failed (verification ledger unavailable): {e}"
                    ))
                })?;
            Ok(json!({
                // Lead with the deletion-resistant count (paper Rule 5.4):
                // a count of clean delegations cannot be gamed by recording
                // fewer delegations or retiring cards with violations. The
                // derived rates (clean_rate, coverage_rate) are secondary.
                "delegations_with_zero_nulled": report.delegations_with_zero_nulled,
                "trend": report,
                "clean_rate": report.clean_rate(),
                "coverage_rate": report.coverage_rate(),
                "scope": scope_json(&scope),
                "source": "central_verification_ledger",
            }))
        })
        .await
    }

    /// Query recent grounding violations from the central verification
    /// ledger. Returns delegations with nulled fields or narrative leaks,
    /// sorted by timestamp descending. The operator uses this to see what
    /// is failing right now and where to direct remediation (adjust
    /// contracts, add tools, retire agents).
    #[tool(
        description = "Query recent grounding violations from the central verification ledger. Returns delegations with nulled fields or narrative leaks, sorted by timestamp descending. Defaults to the last 24 hours. Returns Err when the ledger is unavailable."
    )]
    pub async fn curator_grounding_violations(
        &self,
        Parameters(req): Parameters<GroundingViolationsToolRequest>,
    ) -> String {
        execute_tool(self, "curator_grounding_violations", async {
            let scope = parse_grounding_scope(
                req.scope.as_deref(),
                req.agent_name.as_deref(),
                req.source.as_deref(),
            );
            let since = match req.since.as_deref() {
                Some(s) => chrono::DateTime::parse_from_rfc3339(s)
                    .map_err(|e| {
                        McpToolError::invalid_argument(format!(
                            "invalid `since` timestamp (expected ISO 8601 / RFC 3339): {e}"
                        ))
                    })?
                    .with_timezone(&chrono::Utc),
                None => chrono::Utc::now() - chrono::Duration::hours(24),
            };
            let violations = self
                .verification_store
                .grounding_violations(since, &scope)
                .map_err(|e| {
                    McpToolError::unavailable(format!(
                        "grounding violations query failed (verification ledger unavailable): {e}"
                    ))
                })?;
            Ok(json!({
                "violations": violations,
                "count": violations.len(),
                "since": since.to_rfc3339(),
                "scope": scope_json(&scope),
                "source": "central_verification_ledger",
            }))
        })
        .await
    }

    /// Query the grounding coverage report from the central verification
    /// ledger. Reports which agent types have grounding contracts vs. which
    /// have delegations but no contract (the coverage gap, paper §6:
    /// coverage is itself a metric, not a pass). The operator uses this to
    /// see which agent types need a contract written.
    #[tool(
        description = "Query the grounding coverage report from the central verification ledger. Reports which agent types have grounding contracts vs. which have delegations but no contract (the coverage gap, paper §6). The operator uses this to see which agent types need a contract written."
    )]
    pub async fn curator_grounding_coverage(
        &self,
        Parameters(_req): Parameters<GroundingCoverageToolRequest>,
    ) -> String {
        execute_tool(self, "curator_grounding_coverage", async {
            let entries = self
                .verification_store
                .grounding_coverage()
                .map_err(|e| McpToolError::unavailable(format!(
                    "grounding coverage query failed (verification ledger unavailable): {e}"
                )))?;
            let total: usize = entries.iter().map(|e| e.total_delegations).sum();
            let with_contract: usize = entries.iter().map(|e| e.delegations_with_contract).sum();
            let without_contract: usize = entries.iter().map(|e| e.delegations_without_contract).sum();
            let coverage_rate = if total == 0 { None } else { Some(with_contract as f64 / total as f64) };
            Ok(json!({
                "agent_types": entries,
                "total_delegations": total,
                "delegations_with_contract": with_contract,
                "delegations_without_contract": without_contract,
                "coverage_rate": coverage_rate,
                "note": "Coverage gap = delegations_without_contract per agent_type. Each is an agent_type with no grounding contract (paper §6: coverage is itself a metric, not a pass). Write a contract for the agent_type to close the gap.",
                "source": "central_verification_ledger",
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

/// Parse the grounding scope from scope/agent_name/source fields. Maps the
/// string-based scope parameter to the `TrendScope` enum. Defaults to
/// `Global` when `scope` is `None` or unrecognized.
fn parse_grounding_scope(
    scope: Option<&str>,
    agent_name: Option<&str>,
    source: Option<&str>,
) -> hkask_verification::TrendScope {
    match scope.unwrap_or("global") {
        "by_agent" => {
            hkask_verification::TrendScope::ByAgent(agent_name.unwrap_or_default().to_string())
        }
        "by_source" => {
            hkask_verification::TrendScope::BySource(source.unwrap_or_default().to_string())
        }
        _ => hkask_verification::TrendScope::Global,
    }
}

/// Serialize a `TrendScope` to a JSON value for the tool response.
fn scope_json(scope: &hkask_verification::TrendScope) -> serde_json::Value {
    match scope {
        hkask_verification::TrendScope::Global => json!({"kind": "global"}),
        hkask_verification::TrendScope::ByAgent(agent) => json!({
            "kind": "by_agent",
            "agent_name": agent,
        }),
        hkask_verification::TrendScope::BySource(source) => json!({
            "kind": "by_source",
            "source": source,
        }),
    }
}

pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        SERVER_NAME,
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::server::ServerContext| {
            let db = Arc::new(CuratorDb::from_context(&ctx));
            let verification_store = Arc::new(hkask_verification::VerificationStore::open());
            Ok(CuratorServer::new(ctx.webid, verification_store, db))
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
    // An unavailable EmbeddingStore must NOT disable curator memory: every
    // curator memory tool (`curator_semantic_search`, `curator_memory_recall`,
    // `curator_consult`) recalls by entity/EAV, never by vector similarity.
    // Before the store unification the h_mem half survived an embedding
    // failure because it was a separate handle; falling back to the
    // embedding-free constructor preserves that degradation boundary instead
    // of coupling all recall to a capability none of these tools use.
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
