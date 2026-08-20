//! `MemoryPort` adapter — bridges zed's thread completion to hKask memory (D6).
//!
//! `RealMemoryPort` — full hKask memory stack. Stores completed turns as
//! episodic h_mems (Private, perspective = user WebID, PKO-anchored ontology)
//! and semantic h_mems (Shared, DC-anchored ontology, for curator access).
//! Embeds the user prompt for future retrieval. Used when `HKASK_DB_PATH` +
//! `HKASK_DB_PASSPHRASE` are configured.
//!
//! The port is injected via a global hook (`agent::set_memory_port`) so the
//! `agent` crate doesn't depend on `kask_bridge`. When the port is not yet
//! wired (pre-login), the thread's ingest call site no-ops on `None`.

use hkask_memory::{MemoryConsolidator, MemoryStore};
use hkask_storage::{Database, EmbeddingStore, HMem, HMemStore};
use hkask_types::{
    HMemOntology, MemoryError, MemoryPort, MemorySnippet, TurnRecord, Visibility, WebID,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::time::Duration;

use chrono::Utc;

use crate::inference::LanguageModelEmbeddingPort;

// ── Curator store infrastructure — extracted to `memory/curator_stores.rs` ─
// Deep-module split (bridge-audit BD-04): the curator's sovereign `curator.db`
// infrastructure (path resolution, store open, self-healing handle, consolidation
// builder, regulation archive opener) is a one-way dependency of the memory
// port and independent of the user-store orchestration that remains here.
// `open_regulation_archive` stays in this file — the user-store path
// (`RealMemoryPort::new`) also calls it — and `curator_stores` reaches it via
// `use super::open_regulation_archive`.
mod curator_stores;
pub(crate) use curator_stores::{CuratorStore, build_curator_consolidation};
pub use curator_stores::{curator_db_path, open_curator_regulation_archive};

// ── Swarm store infrastructure — extracted to `memory/swarm_stores.rs` ───
// Mirrors `curator_stores` for the swarm's sovereign `swarm_memory.db`.
// Opened directly in the bridge process so `recall_context_swarm` can read
// swarm memory without an IPC round-trip to the swarm MCP server.
mod swarm_stores;
pub(crate) use swarm_stores::SwarmStore;

// ── Alert escalation — extracted to `memory/alert_escalation.rs` ──────────
// Deep-module split (bridge-audit BD-04): the algedonic alert path implements a
// *different* port (`AlertEscalationSink`) with zero coupling to the memory
// port. `open_curator_escalation_queue` borrows `curator_db_path` from the
// `curator_stores` re-export above.
mod alert_escalation;
pub use alert_escalation::{BridgeAlertEscalationSink, open_curator_escalation_queue};

// ── Real memory port (full hKask memory stack) ─────────────────────────────

/// Real `MemoryPort` implementation backed by hKask's unified `MemoryStore`.
///
/// Stores each completed turn as:
/// 1. An episodic h_mem (Private, perspective = user WebID) — the user's
///    first-person experience record, in the user's own `memory.db`.
/// 2. A semantic h_mem (Shared) — a curator-accessible copy written to the
///    **curator's** sovereign `curator.db`, not the user's memory DB. The curator
///    MCP server reads from the same `curator.db`, so `curator_memory_recall` and
///    `curator_semantic_search` see turns the agent has observed.
/// 3. An embedding of the user prompt — for future semantic retrieval and
///    context injection, stored in the user's `memory.db`.
///
/// Construction requires a SQLCipher database path and passphrase. When these
/// are not available, the port is simply not wired (the hook stays `None`).
pub struct RealMemoryPort {
    store: Arc<MemoryStore>,
    /// The curator's sovereign store (`agents/curator/curator.db`) behind a
    /// self-healing handle: when the curator DB cannot be opened at startup
    /// (locked by a previous MCP server instance, transient I/O), the store
    /// is `None` and every access re-attempts the open. A successful
    /// re-open restores curator memory without an app restart; persistent
    /// failure is signaled with a warn-once per healing attempt, never
    /// silently.
    curator_store: Arc<CuratorStore>,
    /// The swarm's sovereign store (`swarm_memory.db`) behind a self-healing
    /// handle — mirrors `curator_store`. Opened directly in the bridge process
    /// so `recall_context_swarm` can read swarm memory without an IPC
    /// round-trip. `None`-valued (self-heals to `Some`) when the swarm DB
    /// cannot be opened (not configured, locked, passphrase mismatch).
    /// Swarm recall degrades to empty — the cascade runs without swarm
    /// memory instead of erroring.
    swarm_store: Arc<SwarmStore>,
    embedding_port: LanguageModelEmbeddingPort,
    embedding_model: String,
    user_webid: WebID,
    curator_webid: WebID,
    /// Consolidation service — promotes the user's episodic h_mems to the
    /// user's semantic memory. `None` when consolidation is disabled
    /// (`consolidation_cadence_secs == 0`).
    consolidation: Option<Arc<MemoryConsolidator>>,
    /// Consolidation service for the curator's stores — promotes the
    /// curator's episodic h_mems (curator-perspective first-person turns) to
    /// the curator's semantic memory, mirroring the user's consolidation loop.
    /// Rebuilt when the curator stores heal after an open failure; `None`
    /// when consolidation is disabled OR the curator stores are unavailable.
    curator_consolidation: RwLock<Option<Arc<MemoryConsolidator>>>,
    /// Consolidation cadence in seconds. `0` disables the trigger for both
    /// the user and curator consolidation services.
    consolidation_cadence_secs: u64,
    /// Confidence floor for semantic cleanup during consolidation.
    confidence_floor: f64,
    /// Timestamp of the last consolidation pass. Guarded by a mutex so the
    /// test-only `maybe_consolidate` method can check-and-update atomically.
    ///
    /// In production, the background timer (`start_consolidation_timer`) uses
    /// its own `Arc<Mutex<Option<DateTime>>>` (captured at startup) because the
    /// timer task must be `Send + 'static` and cannot borrow `&self`. The two
    /// mutexes are not shared — in production, only the timer runs, so this
    /// field stays at its initial value (`None`). This is not a bug; it's a
    /// deliberate split between the test entry point and the production timer.
    last_consolidation: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// Tokio runtime handle — entered around embedding HTTP calls so that
    /// `reqwest` (which is tokio-backed) has a reactor. The memory port's
    /// async methods are called from GPUI's background executor, not tokio.
    tokio_handle: tokio::runtime::Handle,
    /// Ingestion concurrency limiter. Each `ingest_turn` acquires a permit
    /// before touching the database, so concurrent ingestion futures (one per
    /// completing thread) serialize instead of contending for the SQLite pool.
    /// Without this, N active threads each completing a turn fire N concurrent
    /// ingestion futures — each doing multiple writes + an embedding HTTP call
    /// — which crowds out the recall path (also SQLite-bound) and starves the
    /// inference thread's `inject_context` recall.
    ///
    /// Default 1 (fully serial). Override via `HKASK_MEMORY_INGEST_CONCURRENCY`.
    /// A value of 1 is correct for SQLite (single writer).
    ingest_semaphore: tokio::sync::Semaphore,
}

impl RealMemoryPort {
    /// Construct a new `RealMemoryPort` from a database path and passphrase.
    ///
    /// Opens a SQLCipher database, creates episodic and semantic memory stores,
    /// and initializes an embedding router for prompt embedding.
    ///
    /// Returns `Err` if the database cannot be opened.
    pub fn new(
        db_path: &str,
        passphrase: &str,
        user_webid: WebID,
        embedding_model: String,
        embedding_dim: usize,
        embedding_port: LanguageModelEmbeddingPort,
        consolidation_cadence_secs: u64,
        confidence_floor: f64,
        tokio_handle: tokio::runtime::Handle,
    ) -> Result<Self, String> {
        let db = Database::open(db_path, passphrase).map_err(|e| e.to_string())?;
        let pool = db.sqlite_pool().map_err(|e| e.to_string())?;
        let driver: Arc<dyn hkask_storage::DatabaseDriver> = Arc::new(
            hkask_storage::database::sqlite::SqliteDriver::new_labeled(pool, db_path),
        );

        // Unified memory store — h_mems + embeddings, same SQLCipher DB.
        // The embedding dimension must match the embedding model's output —
        // a mismatch causes `DimensionMismatch` errors on every store call,
        // silently disabling embedding-based recall. The caller resolves
        // this from `kask_settings.corpus.embedding_dim` (default 1024,
        // matching `ollama/nomic-embed-text`).
        //
        // A dim of 0 is a footgun: `unwrap_or(1024)` only fires for `None`,
        // not for `Some(0)`, and `KaskCorpusSettings` deriving `Default`
        // returned `u32::default()` == 0, not the serde default 1024. The
        // `From<KaskSettingsContent>` impl filters Some(0) → 1024, but the
        // Default path (no kask.corpus section) bypassed that filter and
        // produced dim == 0 here, panicking in EmbeddingStore::from_driver.
        // KaskCorpusSettings now has a manual Default returning 1024; we
        // clamp below too, in case a caller bypasses settings (e.g.
        // HKASK_EMBEDDING_DIM=0) — per the .rules trap "Process-global hooks
        // set at runtime need a startup-failure signal".
        if embedding_dim == 0 {
            tracing::warn!(
                target: "reg.memory",
                embedding_dim,
                "RealMemoryPort constructed with embedding_dim == 0 — \
                 clamping to 1024 to avoid a zero-dimensional store panic. \
                 Set kask_settings.corpus.embedding_dim (or HKASK_EMBEDDING_DIM) \
                 to match the embedding model's output (default 1024 for \
                 ollama/nomic-embed-text)."
            );
        } else if embedding_dim != 1024 {
            tracing::info!(
                target: "reg.memory",
                embedding_dim,
                "RealMemoryPort using non-default embedding dimension \
                 (ensure this matches the configured embedding model)"
            );
        }
        // Clamp 0 → 1024: the warn above signals the misconfiguration; this
        // keeps the system functional (degraded) instead of panicking.
        let embedding_dim = if embedding_dim == 0 {
            1024
        } else {
            embedding_dim
        };
        let h_mem_store = HMemStore::from_driver(Arc::clone(&driver)).map_err(|e| e.to_string())?;
        let embedding_store = EmbeddingStore::from_driver(driver, embedding_dim)
            .map_err(|e| format!("Failed to create EmbeddingStore: {e}"))?;
        // Resolve the per-agent storage budget and memory life from env vars.
        // The parsing is extracted into pure functions (`resolve_storage_budget`,
        // `resolve_memory_life_days`) so proptests can exercise the
        // parse/fallback/warn logic without constructing a full `RealMemoryPort`.
        let storage_budget = resolve_storage_budget();
        let memory_life_days = resolve_memory_life_days();
        // Wire the `reg.memory.encode` span sink: every `store()` persists a
        // span to the user's `curator.db` regulation archive. Without this the
        // span emitter in `MemoryStore::store` is dead code (the `.rules`
        // "Advertised invariants need enforcement points" trap). `None`
        // degrades to no span persistence with a warn — the store still works.
        let regulation_archive = open_regulation_archive(db_path, passphrase, "user");
        let store = Arc::new(match regulation_archive {
            Some(archive) => MemoryStore::new(h_mem_store, embedding_store)
                .with_storage_budget(storage_budget)
                .with_memory_life_days(memory_life_days)
                .with_ledger(archive),
            None => MemoryStore::new(h_mem_store, embedding_store)
                .with_storage_budget(storage_budget)
                .with_memory_life_days(memory_life_days),
        });

        let curator_webid = WebID::from_persona(b"curator");

        // Curator store behind the self-healing handle — see the field docs.
        let curator_store = Arc::new(CuratorStore::new(passphrase, embedding_dim));

        // Swarm store behind a self-healing handle — mirrors `curator_store`.
        // Opened from `HKASK_SWARM_MEMORY_DB` + `HKASK_SWARM_MEMORY_PASSPHRASE`
        // (same env vars the swarm MCP server reads). When the swarm DB is
        // not configured (passphrase empty / DB missing), the store is `None`
        // and swarm recall degrades to empty — the cascade runs without swarm
        // memory instead of erroring. This is the correct default: swarm
        // memory is only relevant when a swarm agent is participating, and
        // not every deployment runs swarms.
        let swarm_passphrase = std::env::var("HKASK_SWARM_MEMORY_PASSPHRASE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                // The swarm server's compiled-in default (config.rs) is
                // "allostery" — use the same default so the bridge opens the
                // same DB the swarm MCP server opens.
                "allostery".to_string()
            });
        let swarm_embedding_dim = std::env::var("HKASK_SWARM_EMBEDDING_DIM")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .filter(|d: &usize| *d > 0)
            .unwrap_or(embedding_dim);
        let swarm_store = Arc::new(SwarmStore::new(&swarm_passphrase, swarm_embedding_dim));

        // Consolidation service — perspective-bound → shared promotion.
        // Only constructed when the cadence is non-zero; a zero cadence disables
        // the trigger entirely (the operator can still fire consolidation
        // manually via the curator MCP server).
        let consolidation = if consolidation_cadence_secs > 0 {
            Some(Arc::new(MemoryConsolidator::new(Arc::clone(&store))))
        } else {
            None
        };

        let curator_consolidation = RwLock::new(build_curator_consolidation(
            consolidation_cadence_secs,
            &curator_store.get(),
        ));

        Ok(Self {
            store,
            curator_store,
            swarm_store,
            embedding_port,
            embedding_model,
            user_webid,
            curator_webid,
            consolidation,
            curator_consolidation,
            consolidation_cadence_secs,
            confidence_floor,
            last_consolidation: Mutex::new(None),
            tokio_handle,
            ingest_semaphore: tokio::sync::Semaphore::new(
                std::env::var("HKASK_MEMORY_INGEST_CONCURRENCY")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .filter(|v: &usize| *v > 0)
                    .unwrap_or(1),
            ),
        })
    }

    /// Check whether the consolidation cadence has elapsed and, if so, fire
    /// a consolidation pass (episodic → semantic promotion + semantic cleanup).
    ///
    /// This is the single source of truth for the consolidation check-and-fire
    /// logic. The background timer (`start_consolidation_timer`) inlines its
    /// own version of this logic because it needs to capture `Send + 'static`
    /// state (the timestamp is shared via `Arc<Mutex<...>>` rather than
    /// `&self.last_consolidation`). Both paths use the same cadence check and
    /// the same `MemoryConsolidator::consolidate` call.
    ///
    /// Kept as a method so tests can fire consolidation directly without
    /// starting a timer.
    /// Start a background timer that fires consolidation on the configured
    /// cadence. This decouples consolidation from the ingestion path —
    /// ingestion writes complete quickly without waiting for consolidation,
    /// and consolidation runs on its own schedule without holding the
    /// ingestion semaphore.
    ///
    /// The timer checks the cadence every `consolidation_cadence_secs` seconds
    /// (or every 60 seconds if the cadence is < 60, to avoid tight polling).
    /// On each tick it calls `maybe_consolidate`, which does the atomic
    /// check-and-fire under the mutex.
    ///
    /// Returns a `JoinHandle` that the caller can detach or store. Dropping
    /// the handle cancels the timer.
    ///
    /// A cadence of 0 disables consolidation entirely (no timer started).
    pub fn start_consolidation_timer(&self) -> Option<tokio::task::JoinHandle<()>> {
        if self.consolidation_cadence_secs == 0 {
            return None;
        }
        let consolidation = self.consolidation.clone()?;
        let curator_consolidation = self
            .curator_consolidation
            .read()
            .ok()
            .and_then(|guard| guard.clone());
        let user_webid = self.user_webid;
        let curator_webid = self.curator_webid;
        let confidence_floor = self.confidence_floor;
        let last_consolidation = self.last_consolidation.lock().ok().and_then(|guard| *guard);
        let cadence = self.consolidation_cadence_secs;
        // Poll interval: check at least once per cadence window, but no more
        // often than every 60s to avoid tight polling.
        let poll_interval = Duration::from_secs(cadence.clamp(60, 3600));

        // We need a self-referential structure for the timer to call
        // maybe_consolidate on each tick. Instead of capturing `self` (which
        // would require Arc<Self>), we capture the consolidation service and
        // a shared mutex for the last-fired timestamp. This is the same
        // pattern as maybe_consolidate but running on a timer.
        let shared_last: Arc<Mutex<Option<chrono::DateTime<chrono::Utc>>>> =
            Arc::new(Mutex::new(last_consolidation));
        let shared_last_for_timer = Arc::clone(&shared_last);

        let handle = self.tokio_handle.spawn(async move {
            let mut interval = tokio::time::interval(poll_interval);
            // The first tick fires immediately — skip it so we don't consolidate
            // on startup before any ingestion has happened.
            interval.tick().await;
            loop {
                interval.tick().await;
                // Check if the cadence has elapsed since the last consolidation.
                let now = Utc::now();
                let cadence_dur = chrono::Duration::seconds(cadence as i64);
                let should_fire = match shared_last_for_timer.lock() {
                    Ok(mut guard) => {
                        let elapsed = guard
                            .map(|last| now.signed_duration_since(last) >= cadence_dur)
                            .unwrap_or(false);
                        if elapsed {
                            *guard = Some(now);
                            true
                        } else {
                            false
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.memory",
                            error = %e,
                            "consolidation timer: last_consolidation mutex poisoned — stopping timer"
                        );
                        return;
                    }
                };
                if !should_fire {
                    continue;
                }
                let request = hkask_types::ConsolidationRequest {
                    confidence_floor: Some(confidence_floor),
                    ..Default::default()
                };
                tracing::info!(
                    target: "reg.memory",
                    cadence_secs = cadence,
                    confidence_floor,
                    "Consolidation timer fired"
                );
                match consolidation.consolidate(&user_webid, request.clone()) {
                    Ok(outcome) => {
                        tracing::info!(
                            target: "reg.memory",
                            consolidated = outcome.consolidated_count,
                            deleted = outcome.deleted_count,
                            failed = outcome.failed_count,
                            "User consolidation timer pass complete"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.memory",
                            error = %e,
                            "User consolidation timer pass failed"
                        );
                    }
                }
                // Fire the curator consolidation pass on the same cadence —
                // promotes the curator's episodic turns to the curator's
                // semantic memory, mirroring the user pass. Skipped when the
                // curator consolidation service is unavailable (curator
                // stores down); `ingest_turn` rebuilds it after a heal.
                if let Some(curator_consolidation) = &curator_consolidation {
                    match curator_consolidation.consolidate(&curator_webid, request) {
                        Ok(outcome) => {
                            tracing::info!(
                                target: "reg.memory",
                                consolidated = outcome.consolidated_count,
                                deleted = outcome.deleted_count,
                                failed = outcome.failed_count,
                                "Curator consolidation timer pass complete"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                target: "reg.memory",
                                error = %e,
                                "Curator consolidation timer pass failed"
                            );
                        }
                    }
                }
            }
        });
        Some(handle)
    }
}

/// Parse a raw env-var value into a storage budget (`usize`), falling back
/// to `MemoryStore::default_storage_budget()` on malformed/zero values.
///
/// A malformed or zero value warns (the `.rules` "Process-global hooks set at
/// runtime need a startup-failure signal" trap). Extracted as a pure function
/// of the raw string so proptests can exercise the parse/fallback logic
/// without setting env vars or constructing a full `RealMemoryPort`.
fn parse_storage_budget(raw: &str) -> usize {
    match raw.trim().parse::<usize>() {
        Ok(budget) if budget > 0 => budget,
        Ok(_zero) => {
            tracing::warn!(
                target: "reg.memory",
                value = %raw,
                "HKASK_MEMORY_STORAGE_BUDGET must be > 0 — falling back to default {}",
                hkask_memory::MemoryStore::default_storage_budget()
            );
            hkask_memory::MemoryStore::default_storage_budget()
        }
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                value = %raw,
                error = %e,
                "HKASK_MEMORY_STORAGE_BUDGET malformed — falling back to default {}",
                hkask_memory::MemoryStore::default_storage_budget()
            );
            hkask_memory::MemoryStore::default_storage_budget()
        }
    }
}

/// Resolve `HKASK_MEMORY_STORAGE_BUDGET` from the environment, falling back
/// to the default when unset.
fn resolve_storage_budget() -> usize {
    match std::env::var("HKASK_MEMORY_STORAGE_BUDGET") {
        Ok(raw) => parse_storage_budget(&raw),
        Err(_) => hkask_memory::MemoryStore::default_storage_budget(),
    }
}

/// Parse a raw env-var value into a memory life in days (`f64`), falling back
/// to `MemoryStore::default_memory_life_days()` on malformed/negative values.
/// Same startup-failure-signal trap as `parse_storage_budget`.
fn parse_memory_life_days(raw: &str) -> f64 {
    match raw.trim().parse::<f64>() {
        Ok(days) if days >= 0.0 => days,
        Ok(_negative) => {
            tracing::warn!(
                target: "reg.memory",
                value = %raw,
                "HKASK_MEMORY_LIFE_DAYS must be >= 0 — falling back to default {}",
                hkask_memory::MemoryStore::default_memory_life_days()
            );
            hkask_memory::MemoryStore::default_memory_life_days()
        }
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                value = %raw,
                error = %e,
                "HKASK_MEMORY_LIFE_DAYS malformed — falling back to default {}",
                hkask_memory::MemoryStore::default_memory_life_days()
            );
            hkask_memory::MemoryStore::default_memory_life_days()
        }
    }
}

/// Resolve `HKASK_MEMORY_LIFE_DAYS` from the environment, falling back to the
/// default when unset.
fn resolve_memory_life_days() -> f64 {
    match std::env::var("HKASK_MEMORY_LIFE_DAYS") {
        Ok(raw) => parse_memory_life_days(&raw),
        Err(_) => hkask_memory::MemoryStore::default_memory_life_days(),
    }
}

/// Open a `RegulationArchive` on an arbitrary SQLCipher DB. Used by both the
/// user store (`RealMemoryPort::new`, on the user's `curator.db`) and the curator
/// store (`open_curator_store`, on the curator's `curator.db`) to wire
/// `reg.memory.encode` span persistence via `MemoryStore::with_ledger`.
///
/// Returns `None` on any failure with a `tracing::warn!` naming the DB and the
/// role, so an operator can distinguish "not configured" from "configured
/// but broken" (the `.rules` "Process-global hooks set at runtime need a
/// startup-failure signal" trap). The caller degrades to no span persistence.
fn open_regulation_archive(
    db_path: &str,
    passphrase: &str,
    role: &str,
) -> Option<Arc<hkask_storage::RegulationArchive>> {
    let db = match Database::open(db_path, passphrase) {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(
                target: "reg.storage",
                error = %e,
                db_path = %db_path,
                role,
                "Failed to open DB for regulation archive"
            );
            return None;
        }
    };
    let pool = match db.sqlite_pool() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(target: "reg.storage", error = %e, role, "Failed to get SQLite pool for regulation archive");
            return None;
        }
    };
    let driver: Arc<dyn hkask_storage::DatabaseDriver> = Arc::new(
        hkask_storage::database::sqlite::SqliteDriver::new_labeled(pool, db_path),
    );
    match hkask_storage::RegulationArchive::from_driver(driver) {
        Ok(archive) => Some(Arc::new(archive)),
        Err(e) => {
            tracing::warn!(target: "reg.storage", error = %e, role, "Failed to init RegulationArchive schema");
            None
        }
    }
}

impl MemoryPort for RealMemoryPort {
    fn ingest_turn<'a>(
        &'a self,
        record: TurnRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            // Acquire an ingestion permit before touching the database. This
            // serializes concurrent ingestion futures (one per completing
            // thread) so they don't contend for the SQLite pool with each
            // other or with the recall path. The permit is held for the
            // duration of the ingestion (including the embedding HTTP call
            // and consolidation trigger) and released on drop.
            //
            // If the semaphore is contended, this future parks until a permit
            // is available — the calling thread's turn has already completed,
            // so the user sees no latency from this wait.
            let _ingest_permit = match self.ingest_semaphore.acquire().await {
                Ok(permit) => permit,
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        error = %e,
                        "ingest_semaphore closed — skipping ingestion"
                    );
                    return Err(MemoryError::Ingestion(format!(
                        "ingest_semaphore closed: {e}"
                    )));
                }
            };

            let thread_id = record.thread_id.clone();
            let user_input = record.user_input.clone();
            let agent_response = record.agent_response.clone();
            let model = record.model.clone();
            let title = record.thread_title.clone();
            let agent_id = record.agent_id.clone();
            let is_curator_turn = agent_id.as_deref() == Some("Curator");

            let turn_value = serde_json::json!({
                "user_input": user_input,
                "agent_response": agent_response,
                "model": model,
                "title": title,
            });

            // Resolve the curator stores once per ingestion — `get()`
            // re-attempts the open when they're down (self-healing) and
            // signals persistent failure with a warn-once, so the writes
            // below can treat `None` as "already signaled, skip".
            let curator_store = self.curator_store.get();
            // Rebuild the curator consolidation service after a heal so the
            // timer promotes freshly-ingested curator h_mems.
            if curator_store.is_some() {
                let needs_rebuild = match self.curator_consolidation.read() {
                    Ok(guard) => guard.is_none(),
                    Err(_) => true,
                };
                if needs_rebuild && self.consolidation_cadence_secs > 0 {
                    let rebuilt = build_curator_consolidation(
                        self.consolidation_cadence_secs,
                        &curator_store,
                    );
                    if let Ok(mut guard) = self.curator_consolidation.write()
                        && guard.is_none()
                    {
                        *guard = rebuilt;
                    }
                }
            }

            // ── 1. User-perspective episodic h_mem (Private) — EVERY turn ──
            //
            // Both user turns and curator turns are conversations the USER
            // participated in, so both land in the user's `memory.db` as the
            // user's first-person record. Pre-dual-write, curator turns were
            // written only to the curator's sovereign DB — the user had no
            // episodic record of their own curator conversations.
            let entity = format!("chat:thread:{thread_id}");
            // Process-axis anchoring (P5.4): a chat turn is a PKO step
            // execution of the `chat` procedure. `dc_source` carries the
            // thread as the session provenance so recall can distinguish
            // turns by conversation without re-parsing the entity string.
            let episodic_ontology =
                HMemOntology::episodic("chat", "turn", format!("session:{thread_id}"));
            let episodic_h_mem = HMem::new(
                &entity,
                "chatted",
                serde_json::Value::String(turn_value.to_string()),
                self.user_webid,
            )
            .with_perspective(self.user_webid)
            .with_visibility(Visibility::Private)
            .with_ontology(episodic_ontology);

            if let Err(e) = self.store.store(episodic_h_mem) {
                tracing::warn!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    error = %e,
                    "Failed to store episodic h_mem"
                );
                return Err(MemoryError::Ingestion(format!(
                    "Episodic store failed: {e}"
                )));
            }

            // ── 2. Curator-side writes — branch on whose turn it is ──────
            if is_curator_turn {
                // Curator-perspective episodic h_mem (Private,
                // `curator_webid`) in `agents/curator/curator.db` — the curator's
                // own first-person record of the same conversation, mirroring
                // the user's record above. Together they give each party a
                // first-person memory of the shared conversation from their
                // own perspective.
                let episodic_h_mem = HMem::new(
                    &entity,
                    "chatted",
                    serde_json::Value::String(turn_value.to_string()),
                    self.curator_webid,
                )
                .with_perspective(self.curator_webid)
                .with_visibility(Visibility::Private)
                .with_ontology(HMemOntology::episodic(
                    "chat",
                    "turn",
                    format!("session:{thread_id}"),
                ));

                if let Some(ref curator_store) = curator_store {
                    if let Err(e) = curator_store.store(episodic_h_mem) {
                        tracing::warn!(
                            target: "reg.memory",
                            thread_id = %thread_id,
                            error = %e,
                            "Failed to store curator episodic h_mem — \
                             curator will not recall this turn as experience"
                        );
                        // Non-fatal — fall through to semantic copy.
                    }
                } else {
                    // Store unavailability is already signaled (error at
                    // construction, warn-once per heal attempt) — no
                    // additional per-turn log here.
                    tracing::trace!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        "Curator store unavailable — skipping curator episodic write"
                    );
                }
            }

            // Shared semantic copy in the curator's DB — written for BOTH
            // turn kinds so `curator_memory_recall` / `curator_semantic_search`
            // see every turn the agent has observed, regardless of speaker.
            let curator_entity = format!("curator:thread:{thread_id}");
            // State-axis anchoring (P5.4): the curator copy is a document the
            // curator holds about the conversation, not a step it executed.
            // `bibo:Document` is the BIBO type for a standalone record.
            let curator_ontology =
                HMemOntology::semantic("bibo:Document", vec!["chat_turn".to_string()], "curator");
            let curator_h_mem = HMem::new(
                &curator_entity,
                "turn",
                serde_json::Value::String(turn_value.to_string()),
                self.curator_webid,
            )
            .with_visibility(Visibility::Shared)
            .with_ontology(curator_ontology);

            if let Some(ref curator_store) = curator_store {
                if let Err(e) = curator_store.store(curator_h_mem) {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        error = %e,
                        "Failed to store curator semantic h_mem — \
                         curator memory will not include this turn"
                    );
                    // Non-fatal — the episodic record is the primary store.
                }
            } else {
                tracing::trace!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    "Curator store unavailable — skipping curator copy"
                );
            }

            // ── 3. Embed the user prompt for future retrieval ─────────────
            //
            // The embedding enables semantic search (KNN) for context
            // injection. Written to the user's semantic store always; for
            // curator turns, also written to the curator's semantic store so
            // the curator can recall its own turns by similarity.
            //
            // The embedding's `entity_ref` MUST match the episodic h_mem's
            // `entity` (`chat:thread:{thread_id}`) so the recall path's
            // `query_deduped_untouched(entity_ref)` can join the KNN neighbor
            // back to the h_mem holding the full turn text. A separate
            // `embedding:thread:...` namespace was dead code — no h_mem was
            // ever stored under it, so the semantic recall leg always
            // returned zero snippets. See the `recall_context_finds_turn_by_embedding_only`
            // test for the end-to-end pin.
            let embedding_entity = entity.clone();
            // Spawn the embedding HTTP call on the tokio runtime so the
            // GPUI-side channel task (which holds the AsyncApp) can resolve
            // credentials and make the HTTP call. The rest of ingest_turn
            // doesn't need tokio.
            let embedding_model = self.embedding_model.clone();
            let embedding_port = self.embedding_port.clone();
            let user_input_owned = user_input.clone();
            let vectors = self
                .tokio_handle
                .spawn(async move {
                    embedding_port
                        .embed(&embedding_model, &[user_input_owned])
                        .await
                })
                .await;

            match vectors {
                Ok(Ok(vectors)) => {
                    if let Some(vector) = vectors.into_iter().next() {
                        if let Err(e) = self.store.store_embedding(
                            &embedding_entity,
                            &vector,
                            &self.embedding_model,
                        ) {
                            tracing::warn!(
                                target: "reg.memory",
                                thread_id = %thread_id,
                                error = %e,
                                "Failed to store prompt embedding"
                            );
                        }
                        if is_curator_turn
                            && let Some(ref curator_store) = curator_store
                            && let Err(e) = curator_store.store_embedding(
                                &embedding_entity,
                                &vector,
                                &self.embedding_model,
                            )
                        {
                            tracing::warn!(
                                target: "reg.memory",
                                thread_id = %thread_id,
                                error = %e,
                                "Failed to store curator prompt embedding"
                            );
                        }
                    }
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        error = %e,
                        "Failed to embed user prompt — embedding-based recall will not work for this turn"
                    );
                    // Non-fatal — entity-based recall still works.
                }
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        error = %e,
                        "Embedding task panicked — embedding-based recall will not work for this turn"
                    );
                }
            }

            tracing::info!(
                target: "reg.memory",
                thread_id = %thread_id,
                model = %model,
                is_curator_turn,
                "Turn ingested into episodic + semantic memory"
            );

            // Consolidation is no longer fired from the ingestion path. It runs
            // on a dedicated background timer (see `start_consolidation_timer`)
            // so ingestion completes quickly and consolidation doesn't contend
            // with the recall path or hold the ingestion semaphore.

            Ok(())
        })
    }

    fn recall_context<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemorySnippet>, MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            self.recall_from(
                &self.store,
                Some(self.user_webid),
                query,
                limit,
                "recall_context",
            )
            .await
        })
    }

    fn recall_thread<'a>(
        &'a self,
        thread_id: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemorySnippet>, MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            // The semantic copy of a user turn is written to the curator's
            // sovereign DB under entity `curator:thread:{id}` — not to the
            // user's own store, which holds consolidated facts rather than
            // per-turn records. So the semantic leg queries the curator
            // store, not `self.store`.
            let curator_store = self.curator_store.get();
            self.recall_thread_from(
                &self.store,
                curator_store.as_ref(),
                self.user_webid,
                thread_id,
                limit,
                "recall_thread",
            )
            .await
        })
    }
}

impl RealMemoryPort {
    /// Memory-store health for the curator's status surface — the
    /// self-awareness half of the self-healing work. The curator's
    /// regulation loop reads this (via `BridgeMetacognitionProvider`) so it
    /// can detect and escalate its own memory outage instead of waiting for
    /// an operator to notice degraded recall.
    ///
    /// Side-effect-free: reads availability without triggering a heal, so
    /// polling doesn't drive the re-open path.
    pub fn memory_health_json(&self) -> serde_json::Value {
        let curator_up = self.curator_store.availability();
        let swarm_up = self.swarm_store.availability();
        serde_json::json!({
            "curator_store": curator_up,
            "swarm_store": swarm_up,
            "degraded": !curator_up || !swarm_up,
        })
    }

    /// Recall memory snippets from the **curator's** sovereign stores.
    ///
    /// This mirrors `recall_context` but reads from the curator's `MemoryStore`
    /// (`agents/curator/curator.db`) using the
    /// curator's WebID for perspective-scoped episodic queries. Used by the
    /// curator context injector (`BridgeContextInjector::new_curator`) so the
    /// Curator recalls its own memory — a parallel of the user agent's
    /// `BridgeContextInjector`.
    ///
    /// Returns `Ok(vec![])` when the curator stores are not available
    /// (graceful degradation — the curator runs without recall instead of
    /// erroring).
    pub fn recall_context_curator<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemorySnippet>, MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            let Some(ref curator_store) = self.curator_store.get() else {
                return Ok(Vec::new());
            };
            self.recall_from(
                curator_store,
                Some(self.curator_webid),
                query,
                limit,
                "recall_context_curator",
            )
            .await
        })
    }

    /// Recall all memory snippets from the **curator's** sovereign stores for
    /// a specific thread — the entity-scoped parallel of `recall_thread`.
    ///
    /// Used by the curator context injector's `inject_static_context` to load
    /// the curator's prior turns on this thread into the system prompt once
    /// per session. Returns `Ok(vec![])` when the curator stores are not
    /// available (graceful degradation).
    pub fn recall_thread_curator<'a>(
        &'a self,
        thread_id: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemorySnippet>, MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            let Some(ref curator_store) = self.curator_store.get() else {
                return Ok(Vec::new());
            };
            self.recall_thread_from(
                curator_store,
                Some(curator_store),
                self.curator_webid,
                thread_id,
                limit,
                "recall_thread_curator",
            )
            .await
        })
    }

    /// Recall memory snippets from the **swarm's** sovereign store
    /// (`swarm_memory.db`). Mirrors `recall_context_curator` but reads from
    /// the swarm's `MemoryStore`, opened directly in the bridge process.
    ///
    /// Used by the cascade context provider when a swarm agent is
    /// participating in the thread. Returns `Ok(vec![])` when the swarm
    /// store is not available (graceful degradation — the cascade runs
    /// without swarm memory instead of erroring).
    pub fn recall_context_swarm<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemorySnippet>, MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            let Some(ref swarm_store) = self.swarm_store.get() else {
                return Ok(Vec::new());
            };
            // The swarm store uses a single shared WebID
            // (`swarm_delegate_local`) for all swarm agents — there is no
            // per-agent perspective scoping in the swarm store. `None`
            // skips the episodic perspective filter and recalls across all
            // swarm agents' episodic records.
            self.recall_from(swarm_store, None, query, limit, "recall_context_swarm")
                .await
        })
    }

    /// Shared recall implementation for both the user and curator stores.
    ///
    /// `episodic_perspective` scopes the episodic keyword search to the owning
    /// agent's WebID; `None` skips the episodic leg entirely. `log_label` is
    /// used in tracing so the user and curator paths are distinguishable in
    /// logs.
    ///
    /// This was previously duplicated verbatim between `recall_context` and
    /// `recall_context_curator`; the duplication was a maintenance hazard
    /// (a fix to one had to be manually mirrored in the other).
    async fn recall_from<'a>(
        &'a self,
        store: &'a Arc<MemoryStore>,
        episodic_perspective: Option<WebID>,
        query: &'a str,
        limit: usize,
        log_label: &'static str,
    ) -> Result<Vec<MemorySnippet>, MemoryError> {
        // Collect (snippet, h_mem_id) pairs so we can sort, truncate, and
        // touch only the survivors. Both the embedding-KNN and keyword paths
        // read from the same `store`, so no per-candidate source tag is
        // needed — the touch target is always `store`.
        struct Candidate {
            snippet: MemorySnippet,
            h_mem_id: hkask_storage::HMemId,
        }
        let mut candidates: Vec<Candidate> = Vec::new();

        // ── 1. Semantic search (embedding KNN) ───────────────────────
        //
        // Embed the query and search for similar stored embeddings.
        // This finds turns where the user asked similar questions.
        // Spawn the embedding HTTP call on the tokio runtime so the
        // GPUI-side channel task can resolve credentials and make the
        // HTTP call.
        let embedding_model = self.embedding_model.clone();
        let embedding_port = self.embedding_port.clone();
        let query_owned = query.to_string();
        let vectors = self
            .tokio_handle
            .spawn(async move { embedding_port.embed(&embedding_model, &[query_owned]).await })
            .await;

        // A failed embed degrades recall to keyword-only — surface it so
        // the operator can distinguish "no semantic memory" from "embedding
        // endpoint down". Mirrors the ingest_turn failure branches above.
        let vectors = match vectors {
            Ok(Ok(vectors)) => vectors,
            Ok(Err(e)) => {
                tracing::warn!(
                    target: "reg.memory",
                    error = %e,
                    label = log_label,
                    "Failed to embed recall query — semantic recall skipped for this turn"
                );
                Vec::new()
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.memory",
                    error = %e,
                    label = log_label,
                    "Embedding task panicked — semantic recall skipped for this turn"
                );
                Vec::new()
            }
        };
        if let Some(query_vector) = vectors.into_iter().next() {
            match store.search_similar(&query_vector, limit) {
                Ok(results) => {
                    for result in results {
                        // Retrieve the h_mem associated with this embedding
                        // to get the full text content. Use the untouched
                        // variant — we touch only the injected ones below.
                        let entity_ref = &result.embedding.entity_ref;
                        if let Ok(h_mems) = store.query_deduped_untouched(entity_ref) {
                            for h_mem in h_mems {
                                let text = h_mem.value.as_str().unwrap_or("").to_string();
                                if !text.is_empty() {
                                    candidates.push(Candidate {
                                        snippet: MemorySnippet {
                                            text,
                                            source: "semantic".to_string(),
                                            confidence: h_mem.confidence.value(),
                                            relevance_score: 1.0 - result.distance,
                                        },
                                        h_mem_id: h_mem.id,
                                    });
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        error = %e,
                        label = log_label,
                        "Semantic search failed during recall"
                    );
                }
            }
        }

        // ── 2. Episodic search (keyword overlap) ─────────────────────
        //
        // Load episodic h_mems for the agent's chat threads ONCE, then
        // filter by keyword overlap in memory. The previous implementation
        // re-queried the store for each query word (5x) using the same
        // fixed entity string "chat:thread:" — a redundant N+1 scan that
        // also fired touch_recall on every row per iteration, turning
        // recall into a write storm under multi-thread load.
        //
        // We use the untouched variant and touch only the injected h_mems.
        let query_words: Vec<String> = query
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .take(5)
            .map(|w| w.to_lowercase())
            .collect();

        if !query_words.is_empty()
            && let Some(episodic_perspective) = episodic_perspective
        {
            // Use a prefix query to load all chat:thread:* episodic h_mems
            // in a single SQL call. The previous implementation queried
            // the exact entity "chat:thread:" (no thread_id suffix), which
            // never matched stored entities "chat:thread:<thread_id>" —
            // so the episodic keyword search was dead code. Combined with
            // the N+1 loop (one query per query word), this was both broken
            // and a write storm.
            //
            // The recall budget caps the number of rows loaded — without
            // it, a session with thousands of past turns would load all of
            // them into memory on every recall call. We load 10x the
            // recall limit (most recent first) to give the keyword filter a
            // reasonable pool to filter from without unbounded loading.
            let entity_prefix = "chat:thread:".to_string();
            let recall_budget = limit.saturating_mul(10).max(50);
            if let Ok(h_mems) = store.query_for_deduped_untouched_by_prefix(
                &entity_prefix,
                episodic_perspective,
                recall_budget,
            ) {
                for h_mem in h_mems {
                    let text = h_mem.value.as_str().unwrap_or("").to_string();
                    if text.is_empty() {
                        continue;
                    }
                    let text_lower = text.to_lowercase();
                    // Check if ANY query word appears in the text
                    if !query_words.iter().any(|w| text_lower.contains(w)) {
                        continue;
                    }
                    // Skip if already in candidates (dedup by text)
                    if candidates.iter().any(|c| c.snippet.text == text) {
                        continue;
                    }
                    candidates.push(Candidate {
                        snippet: MemorySnippet {
                            text,
                            source: "episodic".to_string(),
                            confidence: h_mem.confidence.value(),
                            relevance_score: 0.5, // Base relevance for keyword match
                        },
                        h_mem_id: h_mem.id,
                    });
                }
            }
        }

        // ── 3. Sort by relevance and truncate ─────────────────────────
        candidates.sort_by(|a, b| {
            b.snippet
                .relevance_score
                .partial_cmp(&a.snippet.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(limit);

        // ── 4. Touch only the injected h_mems ────────────────────────
        //
        // Resets the decay clock on h_mems that actually got used. This
        // is the “memory that gets used stays fresh” semantics, applied
        // post-filter instead of pre-filter — avoids the write storm.
        for c in &candidates {
            if let Err(e) = store.touch_recall(&c.h_mem_id) {
                tracing::warn!(
                    target: "reg.memory.decay",
                    triple_id = %c.h_mem_id.as_uuid(),
                    error = %e,
                    label = log_label,
                    "Failed to touch_recall h_mem during recall"
                );
            }
        }

        let touched = candidates.len();
        let snippets: Vec<MemorySnippet> = candidates.into_iter().map(|c| c.snippet).collect();

        tracing::debug!(
            target: "reg.memory",
            query_len = query.len(),
            recalled = snippets.len(),
            touched,
            label = log_label,
            "Recalled memory snippets for context injection"
        );

        Ok(snippets)
    }

    /// Shared thread-scoped recall implementation for both the user and curator
    /// stores. Mirrors `recall_from` but uses exact-entity queries instead of
    /// content-similarity / keyword overlap.
    ///
    /// The episodic entity is `chat:thread:{thread_id}` (scoped by `perspective`),
    /// and the semantic entity is `curator:thread:{thread_id}`. This is the
    /// correct recall path for `inject_static_context` — the previous
    /// implementation passed the `thread_id` UUID as the query to
    /// `recall_context`, which never matched stored turn text (the stored
    /// embeddings are of `user_input`, not the thread_id), so static context
    /// injection was dead code for both the user and curator injectors.
    async fn recall_thread_from<'a>(
        &'a self,
        episodic_store: &'a Arc<MemoryStore>,
        semantic_store: Option<&'a Arc<MemoryStore>>,
        perspective: WebID,
        thread_id: &'a str,
        limit: usize,
        log_label: &'static str,
    ) -> Result<Vec<MemorySnippet>, MemoryError> {
        enum RecallSource {
            Episodic,
            Semantic,
        }
        struct Candidate {
            snippet: MemorySnippet,
            h_mem_id: hkask_storage::HMemId,
            source: RecallSource,
        }
        let mut candidates: Vec<Candidate> = Vec::new();

        // ── 1. Episodic: exact entity match, perspective-scoped ─────
        let episodic_entity = format!("chat:thread:{thread_id}");
        if let Ok(h_mems) =
            episodic_store.query_for_deduped_untouched(&episodic_entity, perspective)
        {
            for h_mem in h_mems {
                let text = h_mem.value.as_str().unwrap_or("").to_string();
                if text.is_empty() {
                    continue;
                }
                candidates.push(Candidate {
                    snippet: MemorySnippet {
                        text,
                        source: "episodic".to_string(),
                        confidence: h_mem.confidence.value(),
                        relevance_score: 1.0,
                    },
                    h_mem_id: h_mem.id,
                    source: RecallSource::Episodic,
                });
            }
        }

        // ── 2. Semantic: exact entity match (shared copy in curator DB) ─
        // The `curator:thread:{thread_id}` entity is written to the
        // curator's sovereign DB by both user and curator ingestion paths.
        // `semantic_store` here is the curator store for both callers — see
        // the `recall_thread` and `recall_thread_curator` wrappers.
        let semantic_entity = format!("curator:thread:{thread_id}");
        if let Some(semantic) = semantic_store
            && let Ok(h_mems) = semantic.query_deduped_untouched(&semantic_entity)
        {
            for h_mem in h_mems {
                let text = h_mem.value.as_str().unwrap_or("").to_string();
                if text.is_empty() {
                    continue;
                }
                // Dedup against episodic by text
                if candidates.iter().any(|c| c.snippet.text == text) {
                    continue;
                }
                candidates.push(Candidate {
                    snippet: MemorySnippet {
                        text,
                        source: "semantic".to_string(),
                        confidence: h_mem.confidence.value(),
                        relevance_score: 1.0,
                    },
                    h_mem_id: h_mem.id,
                    source: RecallSource::Semantic,
                });
            }
        }

        // ── 3. Truncate to limit ────────────────────────────────────
        // `query_for_deduped_untouched` and `query_deduped_untouched`
        // both return most-recent-first, so the candidates are already in
        // recency order. All candidates have relevance_score 1.0 (exact
        // entity match), so no sort is needed — just truncate.
        candidates.truncate(limit);

        // ── 4. Touch only the injected h_mems ────────────────────────
        for c in &candidates {
            let touch_store: &Arc<MemoryStore> = match c.source {
                RecallSource::Episodic => episodic_store,
                RecallSource::Semantic => semantic_store.unwrap_or(episodic_store),
            };
            let result: Result<(), Box<dyn std::error::Error>> =
                touch_store.touch_recall(&c.h_mem_id).map_err(Into::into);
            if let Err(e) = result {
                tracing::warn!(
                    target: "reg.memory.decay",
                    triple_id = %c.h_mem_id.as_uuid(),
                    error = %e,
                    label = log_label,
                    "Failed to touch_recall h_mem during thread recall"
                );
            }
        }

        let touched = candidates.len();
        let snippets: Vec<MemorySnippet> = candidates.into_iter().map(|c| c.snippet).collect();

        tracing::debug!(
            target: "reg.memory",
            thread_id_len = thread_id.len(),
            recalled = snippets.len(),
            touched,
            label = log_label,
            "Recalled thread memory snippets for static context injection"
        );

        Ok(snippets)
    }
}

// ── BridgeMemoryPort (agent::ThreadMemoryPort adapter) ─────────────────────

/// Adapter that implements the `agent` crate's `ThreadMemoryPort` trait
/// by delegating to an `hkask_types::MemoryPort`.
///
/// This is the bridge between the `agent` crate's local trait (which can't
/// depend on `hkask-types`) and the hKask `MemoryPort` trait.
pub struct BridgeMemoryPort {
    inner: std::sync::Arc<dyn MemoryPort>,
}

impl BridgeMemoryPort {
    pub fn new(inner: std::sync::Arc<dyn MemoryPort>) -> Self {
        Self { inner }
    }
}

impl agent::ThreadMemoryPort for BridgeMemoryPort {
    fn ingest_turn(
        &self,
        record: agent::ThreadTurnRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let inner = self.inner.clone();
        // Capture user-message non-emptiness before `record` is moved into the
        // TurnRecord below. The reask correlator (T7b) emits a
        // `reg.widget.reask` Regulation span when a user-message turn follows a
        // turn that rendered a provenance-carrying widget. The return is pure
        // telemetry (a test seam, not a production consumer) — discarded here;
        // the leaf unit tests assert the bool.
        let user_message = !record.user_input.trim().is_empty();
        hkask_tool_invoker::correlate_reask(user_message);
        Box::pin(async move {
            inner
                .ingest_turn(TurnRecord {
                    thread_id: record.thread_id,
                    user_input: record.user_input,
                    agent_response: record.agent_response,
                    model: record.model,
                    thread_title: record.thread_title,
                    agent_id: record.agent_id.map(|id| id.to_string()),
                })
                .await
                .map_err(|e| e.to_string())
        })
    }
}

/// Test-only constructor for an in-memory `RealMemoryPort` with no
/// consolidation. Shared across test modules in this crate so
/// `context_injector.rs` tests can construct a `BridgeContextInjector`
/// without duplicating the heavy `RealMemoryPort` setup. Mirrors the
/// private `in_memory_port` helper in `tests` below.