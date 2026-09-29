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

pub(crate) mod distillation;
pub(crate) mod federated;
pub(crate) mod forgetting;

pub(crate) mod thread_turns;
pub mod types;

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_mcp_server::server::{
    McpToolError, execute_tool, map_infra_error, map_memory_store_error, resolve_db_passphrase,
};

use hkask_storage::database::sqlite::SqliteDriver;

use hkask_types::regulation::RegulationSpan;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use serde_json::json;
use std::path::PathBuf;
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
/// plus the number of KNN hits that failed to resolve to any h_mem — the
/// store errored, or the hit's entity has no h_mems (the KNN-orphan case).
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

/// The curator's stores, backed by the curator's
/// sovereign `curator.db`. Grouped so the self-healing handle can swap the whole
/// set atomically after a re-open.
///
/// Named fields (not a positional tuple): tools address stores by name, so
/// adding or reordering a store cannot silently rebind a `..` destructuring
/// to the wrong store.
#[derive(Clone)]
pub struct CuratorStores {
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
        self.regulation_store.is_none() && self.memory.is_none()
    }

    /// Empty store set — used when the DB cannot be opened at all.
    pub fn empty() -> Self {
        Self {
            regulation_store: None,
            memory: None,
        }
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
/// previous server instance), every tool call
/// re-attempts the open via `get()`. A successful heal restores the curator's
/// full tool surface mid-process — no server restart. Failure is never
/// silent: construction failure logs `error!`, each failed heal attempt
/// warns once per outage round (re-armed on heal), a successful heal logs
/// `info!`.
pub struct CuratorDb {
    stores: RwLock<CuratorStores>,
    db_path: Option<String>,
    passphrase: Option<String>,
    federated_manifest_path: PathBuf,
    federated_sources: std::sync::Mutex<Option<Arc<federated::FederatedSourceRegistry>>>,
    /// The decay constant applied to every (re)opened memory store. Resolved
    /// once from env at construction so construction and heals apply the
    /// same operator setting.
    memory_life_days: f64,
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
        // The `required` startup declaration guarantees this resolves for
        // production launches (transport refuses startup naming the var);
        // the Err arm is the defensive tail — there is no in-memory store
        // behind it, so stores stay down until relaunch.
        let passphrase = match resolve_db_passphrase(&ctx.credentials) {
            Ok(passphrase) => Some(passphrase),
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.curator",
                    %error,
                    "Curator stores stay down — DB-backed tools return permission_denied until relaunch with HKASK_DB_PASSPHRASE resolvable"
                );
                None
            }
        };
        let heal_enabled = passphrase.is_some();
        let memory_life_days = memory_life_days_from_env();
        let stores = open_curator_stores(
            Some(db_path.as_str()),
            passphrase.as_deref(),
            memory_life_days,
        );
        let federated_manifest_path = federated::default_manifest_path();
        let this = Self {
            stores: RwLock::new(stores),
            db_path: Some(db_path),
            passphrase,
            federated_manifest_path,
            federated_sources: std::sync::Mutex::new(None),
            memory_life_days,
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
            federated_manifest_path: PathBuf::new(),
            federated_sources: std::sync::Mutex::new(None),
            memory_life_days: hkask_memory::MemoryStore::default_memory_life_days(),
            heal_attempt_logged: AtomicBool::new(false),
            heal_enabled: false,
            last_heal_attempt: std::sync::Mutex::new(None),
        }
    }

    #[doc(hidden)]
    pub fn from_stores_with_federated_manifest(
        stores: CuratorStores,
        manifest_path: PathBuf,
        passphrase: String,
    ) -> Self {
        Self {
            stores: RwLock::new(stores),
            db_path: None,
            passphrase: Some(passphrase),
            federated_manifest_path: manifest_path,
            federated_sources: std::sync::Mutex::new(None),
            memory_life_days: hkask_memory::MemoryStore::default_memory_life_days(),
            heal_attempt_logged: AtomicBool::new(false),
            heal_enabled: false,
            last_heal_attempt: std::sync::Mutex::new(None),
        }
    }

    fn federated_sources(&self) -> Arc<federated::FederatedSourceRegistry> {
        let mut cached = self.federated_sources.lock().unwrap_or_else(|poisoned| {
            tracing::warn!(target: "hkask.mcp.curator", "Federated source cache lock poisoned; revalidating source identities");
            poisoned.into_inner()
        });
        if let Some(registry) = cached.as_ref()
            && registry.unchanged()
        {
            return Arc::clone(registry);
        }
        let fresh = Arc::new(federated::FederatedSourceRegistry::load(
            &self.federated_manifest_path,
            self.passphrase.as_deref(),
        ));
        *cached = Some(Arc::clone(&fresh));
        fresh
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
        let fresh = open_curator_stores(
            self.db_path.as_deref(),
            self.passphrase.as_deref(),
            self.memory_life_days,
        );
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
    /// any h_mem — the store errored, or the hit's entity has no h_mems
    /// (the KNN-orphan case). Each distinct entity contributes at most
    /// `MAX_FRAGMENTS_PER_ENTITY` fragments (its freshest), and no h_mem
    /// appears twice even when the KNN hits it through several embeddings.
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
                    if h_mems.is_empty() {
                        // KNN orphan: the embedding's entity has no h_mems,
                        // so this hit resolved to nothing. Count it so an
                        // all-orphans run reads as degraded recall, not as
                        // an empty store.
                        resolution_failures += 1;
                        tracing::warn!(
                            target: "hkask.mcp.curator",
                            entity_ref = %entity_ref,
                            "KNN hit resolved to zero h_mems — orphaned embedding (non-fatal)"
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
                Ok(recall) if !recall.fragments.is_empty() => {
                    let serialized: Vec<serde_json::Value> = recall
                        .fragments
                        .iter()
                        .take(limit)
                        .map(|(t, distance)| {
                            json!({
                                "id": t.id.to_string(),
                                "entity": t.entity, "attribute": t.attribute,
                                "value": t.value, "confidence": t.confidence,
                                "distance": distance,
                            })
                        })
                        .collect();
                    let yielded = serialized.len();
                    let mut result = json!({
                        "count": yielded,
                        "mode": "semantic",
                        "results": serialized,
                    });
                    let mut notes: Vec<String> = Vec::new();
                    if recall.resolution_failures > 0 {
                        notes.push(resolution_failure_note(recall.resolution_failures, yielded));
                    }
                    if recall.excluded_model_mismatch > 0 {
                        result["model_mismatch_excluded"] =
                            json!(recall.excluded_model_mismatch);
                        notes.push(model_mismatch_note(recall.excluded_model_mismatch));
                    }
                    if !notes.is_empty() {
                        result["note"] = json!(notes.join("; "));
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
                        .map(|t| {
                            json!({
                                "id": t.id.to_string(),
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
                Ok(recall) => {
                    // Every KNN hit failed to resolve, or none matched. When
                    // hits failed, a bare count:0 would read as "genuinely
                    // no similar memories" — say resolution failed instead
                    // (the empty-result-as-success trap).
                    let mut result = json!({
                        "count": 0,
                        "mode": "semantic",
                        "results": [],
                    });
                    let mut notes: Vec<String> = Vec::new();
                    if recall.resolution_failures > 0 {
                        notes.push(resolution_failure_note(recall.resolution_failures, 0));
                    }
                    if recall.excluded_model_mismatch > 0 {
                        result["model_mismatch_excluded"] =
                            json!(recall.excluded_model_mismatch);
                        notes.push(model_mismatch_note(recall.excluded_model_mismatch));
                    }
                    if !notes.is_empty() {
                        result["note"] = json!(notes.join("; "));
                    }
                    Ok(result)
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
                        reason: (recall.resolution_failures > 0
                            || recall.excluded_model_mismatch > 0)
                            .then(|| {
                                let mut notes = Vec::new();
                                if recall.resolution_failures > 0 {
                                    notes.push(resolution_failure_note(
                                        recall.resolution_failures,
                                        hits.len(),
                                    ));
                                }
                                if recall.excluded_model_mismatch > 0 {
                                    notes.push(model_mismatch_note(recall.excluded_model_mismatch));
                                }
                                notes.join("; ")
                            }),
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
                match source.search(&embedding_model, &query_vector, limit) {
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
                        json!({
                            "id": t.id.to_string(),
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
                                        "id": t.id.to_string(),
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
                                        "id": t.id.to_string(),
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
                Ok(recall) if !recall.fragments.is_empty() => {
                    // Entity-wide — the curator's consolidated knowledge:
                    // every KNN-resolved h_mem regardless of who wrote it.
                    let entity_wide: Vec<serde_json::Value> = recall
                        .fragments
                        .iter()
                        .take(limit)
                        .map(|(t, distance)| {
                            json!({
                                "id": t.id.to_string(),
                                "entity": t.entity,
                                "attribute": t.attribute,
                                "value": t.value,
                                "confidence": t.confidence,
                                "distance": distance,
                            })
                        })
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
                        .map(|(t, distance)| {
                            json!({
                                "id": t.id.to_string(),
                                "entity": t.entity,
                                "attribute": t.attribute,
                                "value": t.value,
                                "confidence": t.confidence,
                                "distance": distance,
                            })
                        })
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
                    let mut notes: Vec<String> = Vec::new();
                    if recall.resolution_failures > 0 {
                        notes.push(resolution_failure_note(
                            recall.resolution_failures,
                            entity_wide_count,
                        ));
                    }
                    if recall.excluded_model_mismatch > 0 {
                        notes.push(model_mismatch_note(recall.excluded_model_mismatch));
                    }
                    if !notes.is_empty() {
                        let note = json!(notes.join("; "));
                        entity_wide_json["note"] = note.clone();
                        perspective_json["note"] = note;
                    }
                    result["entity_wide_fragments"] = entity_wide_json;
                    result["perspective_scoped_fragments"] = perspective_json;
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
                                            "id": t.id.to_string(),
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
                                            "id": t.id.to_string(),
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
            let embed_text = hkask_memory::semantic_passage_for_h_mem(&h_mem).ok_or_else(|| {
                McpToolError::internal("skill-use report has no canonical semantic passage")
            })?;

            memory
                .store(h_mem)
                .map_err(|e| map_memory_store_error(e, "Failed to store skill-use issue report"))?;

            RegulationSpan::Curation.emit("skill_use_issue_reported");

            // Semantic recallability — the same canonical passage identity
            // writer, backfill, and production recall use. Non-fatal;
            // degradation is surfaced below.
            let embedded = embed_for_semantic_recall(
                self.inference_port.as_ref(),
                memory,
                &entity,
                &embed_text,
            )
            .await;

            Ok(json!({
                "reported": true,
                "entity": entity,
                "skill_name": req.skill_name,
                "tool_name": req.tool_name,
                "step_ordinal": req.step_ordinal,
                "failure_type": req.failure_type,
                "failure_origin": req.failure_origin.as_str(),
                "semantic_recall": if embedded { "embedded" } else { DEGRADED_EMBEDDING_NOTE },
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

            // Build the h_mem with confidence floor 0.5. The AnyJsonValue
            // tool input converts to an owned Value here — the handler
            // mutates it (note insertion, recall_text backfill) below.
            let mut value = serde_json::Value::from(req.value);
            if let Some(note) = &req.note {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("_note".to_string(), serde_json::Value::String(note.clone()));
                }
            }
            let default_recall_text = format!("{} {}: {}", req.entity, req.attribute, value);
            if let Some(object) = value.as_object_mut() {
                let has_supported_passage = object
                    .get("text")
                    .or_else(|| object.get("recall_text"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|text| !text.is_empty());
                let is_mutable = object
                    .get("mutable_state")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true);
                if !has_supported_passage && !is_mutable {
                    object.insert(
                        "recall_text".to_string(),
                        serde_json::Value::String(default_recall_text),
                    );
                }
            } else if !value.is_string() {
                value = serde_json::json!({
                    "value": value,
                    "recall_text": default_recall_text,
                });
            }
            let h_mem = hkask_storage::HMem::new(
                &req.entity,
                &req.attribute,
                value,
                self.webid,
            )
            .with_confidence(hkask_types::Confidence::new(0.5));
            let embed_text = hkask_memory::semantic_passage_for_h_mem(&h_mem).ok_or_else(|| {
                McpToolError::invalid_argument(
                    "memory value has no canonical semantic passage; mutable state requires recall_text",
                )
            })?;

            memory
                .store(h_mem)
                .map_err(|e| map_memory_store_error(e, "Failed to store curator memory"))?;

            RegulationSpan::Curation.emit("memory_inserted");

            // Semantic recallability — the entity_ref invariant. Without
            // this, every agent-inserted memory (operator rulings, verified
            // code status — the knowledge layer) is invisible to
            // `curator_semantic_search` and the semantic leg of
            // `curator_consult`: recallable only by exact entity name, and
            // a semantic search that found nothing was read as "no memory
            // exists". Non-fatal; the degradation is surfaced below.
            let embedded = embed_for_semantic_recall(
                self.inference_port.as_ref(),
                memory,
                &req.entity,
                &embed_text,
            )
            .await;

            Ok(json!({
                "inserted": true,
                "entity": req.entity,
                "attribute": req.attribute,
                "confidence": 0.5,
                "evidence_h_mem_id": req.evidence_h_mem_id,
                "semantic_recall": if embedded { "embedded" } else { DEGRADED_EMBEDDING_NOTE },
                "guidance": "Memory stored at confidence 0.5 with a semantic embedding. Use memory_update to adjust confidence after outcome observation. Retrieve via curator_memory_recall with this entity, or curator_semantic_search by meaning."
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

            // Fetch the existing h_mem to get its current value and confidence
            // — by ID, not by entity ref. The previous entity-keyed lookup
            // (`query_deduped_untouched` with the bare UUID) could never
            // match — no entity is a bare UUID — so every update attempt
            // returned not_found and the tool never updated anything (the
            // same bug class memory_insert's evidence check and
            // memory_resolve_contradiction were fixed for).
            let existing_h_mem = memory
                .get_by_id(&h_mem_id)
                .map_err(|e| map_memory_store_error(e, "Failed to fetch h_mem for update"))?
                .ok_or_else(|| {
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
            let value = req
                .new_value
                .map(serde_json::Value::from)
                .unwrap_or_else(|| existing_h_mem.value.clone());

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

            let target_id = req
                .target_h_mem_id
                .parse::<hkask_storage::HMemId>()
                .map_err(|e| {
                    McpToolError::invalid_argument(format!(
                        "Invalid target_h_mem_id '{id}': {e}",
                        id = req.target_h_mem_id
                    ))
                })?;

            // Verify the target exists — by ID, not by entity ref. The
            // previous entity-keyed lookup (`query_deduped_untouched` with
            // the bare UUID) could never match — no entity is a bare UUID —
            // so every resolution attempt returned not_found and the tool
            // never resolved anything (the same bug class memory_insert's
            // evidence check was fixed for).
            let target_h_mem = memory
                .get_by_id(&target_id)
                .map_err(|e| map_memory_store_error(e, "Failed to fetch target h_mem"))?
                .ok_or_else(|| {
                    McpToolError::not_found(format!(
                        "Target h_mem '{id}' not found",
                        id = req.target_h_mem_id
                    ))
                })?;

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
        description = "Backfill semantic embeddings across the memory stores. mode=missing (default) embeds h_mems whose exact canonical passage has no embedding row — eligibility is exact (entity + canonical passage), not entity-level; over the curator store it excludes turns, distillation watermarks, and goal rows; over the swarm store (store=swarm) eligibility mirrors the swarm server's own embed path: delegation response chunks only. mode=model_mismatch re-embeds stored rows whose recorded model matches neither the current requested embedding model nor its provider-confirmed actual form — the same two-form predicate the search gate excludes on, so a model migration's gated-out rows are restored rather than silently invisible. store=curator (default) or store=swarm (the shared swarm memory DB). dry_run lists candidates without embedding."
    )]
    pub async fn curator_memory_backfill_embeddings(
        &self,
        Parameters(req): Parameters<BackfillEmbeddingsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_memory_backfill_embeddings", async {
            let mode = match req.mode.as_deref().map(str::trim) {
                None | Some("") | Some("missing") => BackfillMode::Missing,
                Some("model_mismatch") => BackfillMode::ModelMismatch,
                Some(other) => {
                    return Err(McpToolError::invalid_argument(format!(
                        "mode must be \"missing\" or \"model_mismatch\", got \"{other}\""
                    )));
                }
            };
            let store_target = match req.store.as_deref().map(str::trim) {
                None | Some("") | Some("curator") => BackfillStore::Curator,
                Some("swarm") => BackfillStore::Swarm,
                Some(other) => {
                    return Err(McpToolError::invalid_argument(format!(
                        "store must be \"curator\" or \"swarm\", got \"{other}\""
                    )));
                }
            };

            let stores = self.db.get();
            let memory: Arc<hkask_memory::MemoryStore> = match store_target {
                BackfillStore::Curator => stores.memory()?.clone(),
                BackfillStore::Swarm => open_swarm_memory_store()?,
            };

            match mode {
                BackfillMode::Missing => {
                    backfill_missing_passages(
                        self.inference_port.as_ref(),
                        &memory,
                        store_target,
                        req.dry_run.unwrap_or(false),
                    )
                    .await
                }
                BackfillMode::ModelMismatch => {
                    backfill_model_mismatched_rows(
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
}

// ── Server startup ─────────────────────────────────────────────────────

/// Output note surfaced by the insert paths when the semantic embedding
/// could not be stored — the degradation must be visible in the tool
/// result, never a silent success.
pub(crate) const DEGRADED_EMBEDDING_NOTE: &str = "degraded (embedding unavailable — warn logged)";

/// The curator server's embedding model: the env-resolved model when
/// configured, else `None`. `None` is NOT a fallback to a hidden constant
/// (the operator's no-hidden-models spec) — the semantic paths degrade
/// VISIBLY: `curator_semantic_search` returns a typed
/// `EmbeddingNotConfigured` error naming the setting, and the insert
/// paths stamp `DEGRADED_EMBEDDING_NOTE` + warn. The 2026-09-04
/// regression this note previously guarded against was the degradation
/// firing under default settings; the ratified fix is to surface it (the
/// operator sets `kask.models.embedding_model`), not to hide a constant
/// model behind it.
pub(crate) fn curator_embedding_model() -> Option<String> {
    hkask_inference::model_constants::embedding_model()
}

/// Embed `text` under `entity` so semantic recall finds the h_mem just
/// stored — the entity_ref invariant (memory-system-specification.md §3).
/// Non-fatal on failure, matching the ingest path's degradation contract
/// (write-side invariant 3): the h_mem is already durable in SQL; only
/// semantic recall degrades, with a warn naming the cause. Returns whether
/// the embedding landed, so callers can surface the degradation.
///
/// The one insert-path embedding contract, shared by `memory_insert`,
/// `curator_report_skill_use_issue`, and the distillation pass after its
/// atomic lesson-plus-watermark commit. Consolidated 2026-09-04: `memory_insert` and the
/// skill-use path stored h_mems without embeddings, leaving the entire
/// agent-inserted knowledge layer invisible to `curator_semantic_search`.
pub(crate) async fn embed_for_semantic_recall(
    inference_port: &dyn hkask_types::InferencePort,
    memory: &hkask_memory::MemoryStore,
    entity: &str,
    text: &str,
) -> bool {
    let owned_text = text.to_string();
    let Some(embedding_model) = curator_embedding_model() else {
        tracing::warn!(
            target: "hkask.mcp.curator",
            entity,
            reason = "no embedding model configured — set \\
                      kask.models.embedding_model (injected as \\
                      HKASK_EMBEDDING_MODEL); kask never falls back to a \\
                      hidden code constant",
            "embedding skipped — semantic recall degraded for this memory"
        );
        return false;
    };
    match inference_port
        .embed_with_dimensions(
            &embedding_model,
            std::slice::from_ref(&owned_text),
            Some(hkask_storage::embedding_dim() as u32),
        )
        .await
    {
        Ok(batch) if !batch.vectors.is_empty() && !batch.vectors[0].is_empty() => {
            match memory.store_embedding(entity, &batch.vectors[0], &embedding_model, Some(text)) {
                Ok(_) => true,
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.mcp.curator",
                        %error,
                        entity,
                        "Failed to store embedding — semantic recall degraded for this memory"
                    );
                    false
                }
            }
        }
        _ => {
            tracing::warn!(
                target: "hkask.mcp.curator",
                entity,
                "Embedding unavailable — semantic recall degraded for this memory"
            );
            false
        }
    }
}

// ── Memory backfill sweep helpers ─────────────────────────────────────────

/// The sweep's two modes: restore missing passages, or re-embed rows the
/// search model gate excludes.
enum BackfillMode {
    Missing,
    ModelMismatch,
}

/// The sweep's two store targets: the curator's own memory DB, or the
/// shared swarm memory DB.
enum BackfillStore {
    Curator,
    Swarm,
}

/// The swarm server's sole embed path stores delegation response chunks
/// (`hkask-mcp-swarm/src/local_knowledge.rs`); the swarm-store missing-mode
/// backfill mirrors that eligibility exactly rather than guessing a
/// knowledge-layer shape the swarm store does not have.
const SWARM_RESPONSE_CHUNK_ATTRIBUTE: &str = "delegation:response_chunk";

/// Resolve the shared swarm memory DB path. Mirrors
/// `kask_bridge::identity::resolve_swarm_memory_db_path`: an absolute
/// `HKASK_SWARM_MEMORY_DB` wins; anything else resolves under the shared
/// kask data dir (the swarm server's own default, `mcp/swarm/memory.db`).
fn swarm_memory_db_path() -> std::path::PathBuf {
    match std::env::var("HKASK_SWARM_MEMORY_DB")
        .ok()
        .filter(|raw| !raw.trim().is_empty())
    {
        Some(raw) if std::path::Path::new(&raw).is_absolute() => raw.into(),
        Some(raw) => hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(&raw)),
        None => hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
            "mcp/swarm/memory.db",
        )),
    }
}

/// Open the shared swarm memory DB for the sweep. The passphrase resolves
/// through the canonical env chain — a missing or empty key is
/// `permission_denied` naming the env var, never a silent empty-key open.
/// Orphaned vec0 shadow rows (metadata deleted without vec access, e.g.
/// the 2026-09-28 SQL pass) collide on re-insert at the reused rowid and
/// fail every backfill write with a UNIQUE constraint error, so the
/// store is swept at open. A sweep failure fails closed: the backfill
/// writes would fail on the same lock anyway.
fn open_swarm_memory_store() -> Result<Arc<hkask_memory::MemoryStore>, McpToolError> {
    let passphrase =
        hkask_mcp_server::resolve_credential("HKASK_DB_PASSPHRASE").map_err(|error| {
            tracing::warn!(
                target: "hkask.mcp.curator",
                %error,
                "HKASK_DB_PASSPHRASE resolution failed — the swarm store sweep is permission_denied"
            );
            McpToolError::permission_denied(
                "HKASK_DB_PASSPHRASE is not resolvable — the swarm store sweep is unavailable \
                 (relaunch with HKASK_DB_PASSPHRASE in the server env)",
            )
        })?;
    if passphrase.is_empty() {
        return Err(McpToolError::permission_denied(
            "HKASK_DB_PASSPHRASE resolved empty — the swarm store sweep is unavailable",
        ));
    }
    let path = swarm_memory_db_path();
    let store = hkask_memory::MemoryStore::open(
        &path.to_string_lossy(),
        &passphrase,
        hkask_storage::embedding_dim(),
    )
    .map_err(|error| {
        McpToolError::internal(format!(
            "cannot open swarm memory DB {}: {error}",
            path.display()
        ))
    })?;
    let removed = store.delete_orphaned_embeddings().map_err(|error| {
        McpToolError::internal(format!(
            "orphaned-embedding sweep failed on swarm memory DB {}: {error}",
            path.display()
        ))
    })?;
    if removed > 0 {
        tracing::warn!(
            target: "hkask.mcp.curator",
            removed,
            "swarm memory DB carried orphaned embedding rows — swept at open"
        );
    }
    Ok(Arc::new(store))
}

/// The gate's two-form predicate: a stored row is mismatched when its
/// recorded model matches neither the requested form nor the provider-
/// confirmed actual form — the same comparison `EmbeddingStore::search`
/// excludes on. A row with no recorded model can never match either form.
fn model_is_mismatched(stored: Option<&str>, requested: &str, actual: Option<&str>) -> bool {
    match stored {
        None => true,
        Some(stored) => stored != requested && actual != Some(stored),
    }
}

/// Missing-passage backfill: h_mems whose exact canonical passage has no
/// embedding row. Over the curator store this is the knowledge-layer sweep
/// (turns, distillation watermarks, and goal rows excluded); over the swarm
/// store eligibility mirrors the swarm server's own embed path —
/// delegation response chunks only.
async fn backfill_missing_passages(
    inference_port: &dyn hkask_types::InferencePort,
    memory: &hkask_memory::MemoryStore,
    store_target: BackfillStore,
    dry_run: bool,
) -> Result<serde_json::Value, McpToolError> {
    // Every h_mem (empty prefix matches all entities — all rows are
    // current; forgotten rows are deleted, not filtered out).
    let active = memory
        .h_mems_by_entity_prefix("")
        .map_err(|e| map_memory_store_error(e, "Failed to scan active h_mems"))?;

    let is_excluded = |entity: &str| {
        entity.starts_with(thread_turns::SHARED_TURN_PREFIX)
            || entity.starts_with(distillation::WATERMARK_PREFIX)
            || entity.starts_with("curator:goal:")
    };
    let eligible = |h_mem: &hkask_storage::HMem| match store_target {
        BackfillStore::Curator => !is_excluded(&h_mem.entity),
        BackfillStore::Swarm => h_mem.attribute == SWARM_RESPONSE_CHUNK_ATTRIBUTE,
    };

    // Candidates are passage-scoped: one successful vector under an entity
    // never hides a failed sibling h_mem.
    let mut candidates: Vec<(&hkask_storage::HMem, String)> = Vec::new();
    // Rows with no embeddable text are named, not just counted, so an
    // operator can see which memories stay invisible to semantic recall.
    let mut unsupported: Vec<serde_json::Value> = Vec::new();
    for h_mem in active.iter().filter(|h_mem| eligible(h_mem)) {
        let Some(passage) = hkask_memory::semantic_passage_for_h_mem(h_mem) else {
            unsupported.push(json!({
                "h_mem_id": h_mem.id.to_string(),
                "entity": h_mem.entity,
                "attribute": h_mem.attribute,
                "reason": "value has no string, recall_text or text field",
            }));
            continue;
        };
        let already_embedded = memory
            .has_embedding_for_passage(&h_mem.entity, &passage)
            .map_err(|error| {
                map_memory_store_error(error, "Failed to inspect passage-level embedding coverage")
            })?;
        if !already_embedded {
            candidates.push((h_mem, passage));
        }
    }

    if dry_run {
        return Ok(json!({
            "dry_run": true,
            "mode": "missing",
            "store": match store_target { BackfillStore::Curator => "curator", BackfillStore::Swarm => "swarm" },
            "candidate_count": candidates.len(),
            "unsupported_count": unsupported.len(),
            "unsupported": unsupported,
            "candidates": candidates.iter().map(|(h_mem, _passage)| json!({
                "h_mem_id": h_mem.id.to_string(),
                "entity": h_mem.entity,
                "attribute": h_mem.attribute,
            })).collect::<Vec<_>>(),
            "guidance": "Dry run — nothing embedded. Re-run without dry_run to backfill."
        }));
    }

    let candidate_count = candidates.len();
    let mut results = Vec::with_capacity(candidate_count);
    let mut embedded_count = 0usize;
    let mut failed_count = 0usize;
    for (h_mem, embed_text) in candidates {
        let embedded =
            embed_for_semantic_recall(inference_port, memory, &h_mem.entity, &embed_text).await;
        if embedded {
            embedded_count += 1;
        } else {
            failed_count += 1;
        }
        results.push(json!({
            "h_mem_id": h_mem.id.to_string(),
            "entity": h_mem.entity,
            "attribute": h_mem.attribute,
            "embedded": embedded,
        }));
    }

    RegulationSpan::Curation.emit("memory_embeddings_backfilled");

    Ok(json!({
        "mode": "missing",
        "store": match store_target { BackfillStore::Curator => "curator", BackfillStore::Swarm => "swarm" },
        "candidate_count": candidate_count,
        "backfilled": embedded_count,
        "failed": failed_count,
        "unsupported_count": unsupported.len(),
        "unsupported": unsupported,
        "results": results,
        "guidance": "Embeddings are backfilled per exact canonical passage. Goal rows are excluded and never repaired here. Failed candidates remain passage-level candidates on re-run."
    }))
}

/// Model-mismatch sweep — the write side of the search model gate
/// (a4174ed658): stored rows whose recorded model matches neither the
/// current requested embedding model nor its provider-confirmed actual
/// form are re-embedded under the current model, so recall stops excluding
/// them. Each stale row is deleted by exact (entity, passage) before its
/// replacement is stored — siblings under the same entity survive; a
/// failure between delete and store leaves a missing passage the
/// missing-mode backfill recovers by design.
async fn backfill_model_mismatched_rows(
    inference_port: &dyn hkask_types::InferencePort,
    memory: &hkask_memory::MemoryStore,
    dry_run: bool,
) -> Result<serde_json::Value, McpToolError> {
    let Some(requested_model) = curator_embedding_model() else {
        return Err(McpToolError::failed_precondition(
            "no embedding model configured — set kask.models.embedding_model \
             (injected as HKASK_EMBEDDING_MODEL); the model-mismatch sweep \
             cannot run without the model to re-embed under",
        ));
    };

    // Probe once to learn the provider-confirmed actual form — the second
    // identity the search gate accepts. A failed probe degrades to the
    // requested-form-only comparison, surfaced here rather than silent.
    let probe = inference_port
        .embed_with_dimensions(
            &requested_model,
            std::slice::from_ref(&"model-mismatch sweep identity probe".to_string()),
            Some(hkask_storage::embedding_dim() as u32),
        )
        .await;
    let actual_model = match &probe {
        Ok(batch) => batch.actual_model.clone(),
        Err(error) => {
            tracing::warn!(
                target: "hkask.mcp.curator",
                %error,
                "identity probe failed — model-mismatch selection compares \
                 against the requested form only"
            );
            None
        }
    };

    let rows = memory
        .all_embeddings_with_text()
        .map_err(|e| map_memory_store_error(e, "Failed to scan stored embeddings"))?;

    // `all_with_text` yields (entity_ref, vector, passage_text, model) —
    // the passage is nullable, the model is not.
    let mut candidates: Vec<(String, String, String)> = Vec::new();
    let mut unsupported: Vec<serde_json::Value> = Vec::new();
    for (entity, _vector, passage, stored_model) in rows {
        if !model_is_mismatched(
            Some(stored_model.as_str()),
            &requested_model,
            actual_model.as_deref(),
        ) {
            continue;
        }
        let Some(passage) = passage.filter(|p| !p.trim().is_empty()) else {
            unsupported.push(json!({
                "entity": entity,
                "stored_model": stored_model,
                "reason": "no stored passage text — the row cannot be re-embedded \
                           from its source; delete it and re-ingest the source passage",
            }));
            continue;
        };
        candidates.push((entity, passage, stored_model));
    }

    if dry_run {
        return Ok(json!({
            "dry_run": true,
            "mode": "model_mismatch",
            "requested_model": requested_model,
            "actual_model": actual_model,
            "candidate_count": candidates.len(),
            "unsupported_count": unsupported.len(),
            "unsupported": unsupported,
            "candidates": candidates.iter().map(|(entity, _passage, stored_model)| json!({
                "entity": entity,
                "stored_model": stored_model,
            })).collect::<Vec<_>>(),
            "guidance": "Dry run — nothing re-embedded. Re-run without dry_run to sweep."
        }));
    }

    let candidate_count = candidates.len();
    let mut results = Vec::with_capacity(candidate_count);
    let mut re_embedded = 0usize;
    let mut failed = 0usize;
    for (entity, passage, stored_model) in candidates {
        let retired = memory
            .delete_embedding_by_entity_ref_and_passage(&entity, &passage)
            .map_err(|e| map_memory_store_error(e, "Failed to retire the stale-model row"))?;
        let embedded = if retired == 0 {
            // The row vanished between scan and sweep — surfaced as a
            // failure below, never silently skipped.
            false
        } else {
            embed_for_semantic_recall(inference_port, memory, &entity, &passage).await
        };
        if embedded {
            re_embedded += 1;
        } else {
            failed += 1;
        }
        results.push(json!({
            "entity": entity,
            "stored_model": stored_model,
            "re_embedded": embedded,
        }));
    }

    RegulationSpan::Curation.emit("memory_embeddings_backfilled");

    Ok(json!({
        "mode": "model_mismatch",
        "requested_model": requested_model,
        "actual_model": actual_model,
        "candidate_count": candidate_count,
        "re_embedded": re_embedded,
        "failed": failed,
        "unsupported_count": unsupported.len(),
        "unsupported": unsupported,
        "results": results,
        "guidance": "Stale-model rows are re-embedded under the current model \
                     (delete-then-store per exact passage; siblings survive). A row \
                     that fails between delete and store becomes a missing passage \
                     the missing-mode backfill recovers."
    }))
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

/// Resolve the memory decay constant from `HKASK_MEMORY_LIFE_DAYS`. Absent →
/// the store default (180). Malformed values warn naming the value and fall
/// back to the default — never a silent fallback. The env var is emitted by
/// the bridge only when the setting differs from the default, so an absent
/// var and a default setting produce the same store.
fn memory_life_days_from_env() -> f64 {
    match std::env::var("HKASK_MEMORY_LIFE_DAYS") {
        Ok(raw) => parse_memory_life_days_value(&raw),
        Err(_) => hkask_memory::MemoryStore::default_memory_life_days(),
    }
}

/// Pure parse for the decay constant — separated so the malformed path is
/// testable without env mutation (env writes are unsafe in edition 2024).
fn parse_memory_life_days_value(raw: &str) -> f64 {
    match raw.trim().parse::<f64>() {
        Ok(value) if value.is_finite() && value > 0.0 => value,
        _ => {
            tracing::warn!(
                target: "hkask.mcp.curator",
                env = "HKASK_MEMORY_LIFE_DAYS",
                value = %raw,
                "Malformed memory life setting — must be a positive number of days; using default"
            );
            hkask_memory::MemoryStore::default_memory_life_days()
        }
    }
}

/// Open the curator's sovereign `curator.db` and construct all four stores from
/// a single shared driver. Called at construction and on every heal attempt.
/// All-or-nothing on the DB-open steps (a failure before store construction
/// returns all `None`s); per-store `from_driver` failures degrade only that
/// store.
///
/// `memory_life_days` is the decay constant S (R(t) = exp(-t/S), spec §7) the
/// store actually uses at recall — resolved once from
/// `HKASK_MEMORY_LIFE_DAYS` (emitted by the bridge's
/// `emit_curator_distillation_env`) so the operator's
/// `kask.memory.memory_life_days` setting governs real decay, not just the
/// regulation sensor that reports it.
fn open_curator_stores(
    db_path: Option<&str>,
    passphrase: Option<&str>,
    memory_life_days: f64,
) -> CuratorStores {
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

    // Memory degrades independently of the Regulation archive below — a
    // memory failure must not take down Regulation queries.
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
            Some(embeddings) => Some(Arc::new(
                hkask_memory::MemoryStore::new(h_mem_store, embeddings)
                    .with_memory_life_days(memory_life_days),
            )),
            None => match hkask_memory::MemoryStore::try_new_without_embeddings(h_mem_store) {
                Ok(store) => {
                    tracing::warn!(
                        target: "hkask.mcp.curator",
                        "EmbeddingStore unavailable — curator memory opened without \
                         embeddings; entity/EAV recall works, vector similarity does not"
                    );
                    Some(Arc::new(store.with_memory_life_days(memory_life_days)))
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
    let regulation_store = match hkask_storage::RegulationArchive::from_driver(Arc::clone(&driver))
    {
        Ok(store) => Some(Arc::new(store)),
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.curator", error = %e, "Failed to create RegulationArchive");
            None
        }
    };
    CuratorStores {
        regulation_store,
        memory,
    }
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

    use super::*;

    /// expect: "A malformed memory-life setting warns and falls back to the
    /// default — never a silent fallback." [P1]
    #[test]
    fn parse_memory_life_days_value_warns_and_defaults_on_malformed() {
        assert_eq!(parse_memory_life_days_value("30"), 30.0);
        assert_eq!(parse_memory_life_days_value(" 45.5 "), 45.5);
        let default = hkask_memory::MemoryStore::default_memory_life_days();
        assert_eq!(parse_memory_life_days_value("soon"), default);
        assert_eq!(parse_memory_life_days_value(""), default);
        assert_eq!(parse_memory_life_days_value("-5"), default);
        assert_eq!(parse_memory_life_days_value("inf"), default);
    }

    /// expect: "The `kask.memory.memory_life_days` setting governs the store's
    /// actual decay — not just the regulation sensor that reports it." [P1]
    /// pre: an encrypted curator DB exists; the decay constant is passed at
    /// open time exactly as `from_context` resolves it from
    /// `HKASK_MEMORY_LIFE_DAYS`.
    /// post: the opened memory store carries the configured constant, and
    /// the default constant arrives when no override is configured.
    #[test]
    fn open_curator_stores_applies_configured_memory_life_days() {
        let directory = tempfile::tempdir().expect("tempdir");
        let db_path = directory.path().join("curator.db");
        let db_path = db_path.to_str().expect("UTF-8 path");
        hkask_storage::open_or_repair(db_path, "test-passphrase").expect("create db");

        let stores = open_curator_stores(Some(db_path), Some("test-passphrase"), 30.0);
        let memory = stores.memory.expect("memory store");
        assert_eq!(
            memory.memory_life_days(),
            30.0,
            "the configured decay constant must reach the store that actually decays"
        );

        let stores_default = open_curator_stores(
            Some(db_path),
            Some("test-passphrase"),
            hkask_memory::MemoryStore::default_memory_life_days(),
        );
        assert_eq!(
            stores_default
                .memory
                .expect("memory store")
                .memory_life_days(),
            hkask_memory::MemoryStore::default_memory_life_days(),
            "without an override the store uses the default decay constant"
        );
    }
}

// Pins the registered tool-surface count end-to-end against the router the
// `#[tool_router(server_handler)]` block generates. Adding or removing a
// curator tool is an intentional surface change — this pin catches
// accidental drift (a dropped `#[tool]` attribute, a registration change)
// instead of shipping as an undocumented surface change. Mirrors
// `hkask-mcp-training::tool_surface_is_exactly_9_registered_tools`.
#[cfg(test)]
mod tool_surface_tests {
    use super::CuratorServer;

    #[test]
    fn tool_surface_is_exactly_15_registered_tools() {
        let tools = CuratorServer::tool_router().list_all();
        assert_eq!(tools.len(), 15, "curator registered tool surface changed");
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
