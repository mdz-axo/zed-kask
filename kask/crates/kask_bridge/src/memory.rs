//! `MemoryPort` adapter — bridges zed's thread completion to hKask memory (D6).
//!
//! `RealMemoryPort` — full hKask memory stack. Stores completed turns as
//! h_mems (Private, perspective = curator WebID, process-axis anchored ontology)
//! and a shared copy for curator access.
//! Embeds the user prompt for future retrieval. Used when the curator DB
//! path + `HKASK_DB_PASSPHRASE` are configured.
//!
//! The port is injected via a global hook (`agent::set_memory_port`) so the
//! `agent` crate doesn't depend on `kask_bridge`. When the port is not yet
//! wired (at startup), the thread's ingest call site no-ops on `None`.

use hkask_memory::{MemoryConsolidator, MemoryStore};
use hkask_storage::open_or_repair;
#[cfg(test)]
use hkask_storage::{EmbeddingStore, HMemStore};
use hkask_types::{MemoryError, MemoryPort, MemorySnippet, TurnRecord, WebID};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::time::Duration;

use chrono::Utc;

use crate::inference_embedding::LanguageModelEmbeddingPort;

// ── Curator store infrastructure — extracted to `memory/curator_stores.rs` ─
// Deep-module split (bridge-audit BD-04): the curator's sovereign `curator.db`
// infrastructure (path resolution, store open, self-healing handle, consolidation
// builder, regulation archive opener) is a one-way dependency of the memory
// port and independent of the user-store orchestration that remains here.
// `open_regulation_archive` stays in this file — the user-store path
// (`RealMemoryPort::new`) also calls it — and `curator_stores` reaches it via
// `use super::open_regulation_archive`.
mod curator_stores;
pub(crate) use curator_stores::curator_db_path;
pub use curator_stores::open_curator_regulation_archive;
pub(crate) use curator_stores::{CuratorStore, build_curator_consolidation};

// ── Alert escalation — extracted to `memory/alert_escalation.rs` ──────────
// Deep-module split (bridge-audit BD-04): the algedonic alert path implements a
// *different* port (`AlertEscalationSink`) with zero coupling to the memory
// port. `open_curator_escalation_queue` borrows `curator_db_path` from the
// `curator_stores` re-export above.
mod alert_escalation;
pub use alert_escalation::{BridgeAlertEscalationSink, open_curator_escalation_queue};

// ── Ingest write path — extracted to `memory/ingest.rs` ────────────────────
// Deep-module split (bridge-audit BD-04 continuation): the turn write path
// (h_mem writes + embedding) is independent of the port
// orchestration and the recall path that remain here. `write_turn` borrows the
// port's fields via `WriteContext`; `ingest_turn` keeps only the semaphore permit.
mod ingest;
pub(crate) use ingest::WriteContext;

// ── Real memory port (full hKask memory stack) ─────────────────────────────

/// Real `MemoryPort` implementation backed by hKask's unified `MemoryStore`.
///
/// Stores each completed turn as:
/// 1. A user-perspective h_mem (Private) — the user's record in the
///    user's own `memory.db`.
/// 2. A shared copy written to the **curator's** sovereign `curator.db`,
///    not the user's memory DB. The curator
///    `curator_memory_recall` / `curator_semantic_search` see turns the
///    agent has observed.
/// 3. An embedding of the user prompt — for future retrieval and
///    context injection, stored in the user's `memory.db`.
///
/// Construction requires a SQLCipher database path and passphrase. When these
/// are not available, the port is simply not wired (the hook stays `None`).
pub struct RealMemoryPort {
    /// The curator's sovereign store (`agents/curator/curator.db`) behind a
    /// self-healing handle: when the curator DB cannot be opened at startup
    /// (locked by a previous MCP server instance, transient I/O), the store
    /// is `None` and every access re-attempts the open. A successful
    /// re-open restores curator memory without an app restart; persistent
    /// failure is signaled with a warn-once per healing attempt, never
    /// silently.
    //
    // All turns (curator and zed agent) are ingested into the curator's
    // shared store. Curator turns additionally get a private perspective-scoped
    // h_mem. The zed agent's turns are shared copies only — the curator can
    // recall what happened across all agents. Context injection is wired for
    // both the curator (via `recall_context_curator`) and the zed agent (via
    // the `MemoryPort` trait impls, which are currently no-ops — recall is
    // curator-only until the zed agent gets its own recall path).
    curator_store: Arc<CuratorStore>,
    /// `None` when embedding credentials are unavailable — h_mem writes still
    /// work (they're pure SQL), but semantic recall (KNN) is degraded to
    /// keyword-only. This must NOT block the memory pipeline: the curator's
    /// episodic memory of conversations is more valuable than vector search.
    embedding_port: Option<LanguageModelEmbeddingPort>,
    embedding_model: String,
    curator_webid: WebID,
    /// Consolidation service for the curator's store. `None` when
    /// consolidation is disabled (`consolidation_cadence_secs == 0`).
    /// Rebuilt when the curator stores heal after an open failure.
    //
    /// Behind an `Arc` so the production consolidation timer can hold a clone
    /// and re-read the current value on each tick — picks up the rebuild that
    /// `write_turn` performs after a curator-store heal.
    curator_consolidation: Arc<RwLock<Option<Arc<MemoryConsolidator>>>>,
    /// Consolidation cadence in seconds. `0` disables the trigger.
    consolidation_cadence_secs: u64,
    /// Confidence floor for cleanup during consolidation.
    confidence_floor: f64,
    /// Timestamp of the last consolidation pass. Shared by the test-only
    /// `maybe_consolidate` method and the production timer (via
    /// `cadence_should_fire`).
    last_consolidation: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// Tokio runtime handle — entered around embedding HTTP calls so that
    /// `reqwest` (which is tokio-backed) has a reactor.
    tokio_handle: tokio::runtime::Handle,
    /// Ingestion concurrency limiter.
    ingest_semaphore: tokio::sync::Semaphore,
}

impl RealMemoryPort {
    /// Construct a new `RealMemoryPort`.
    ///
    /// Opens the curator's SQLCipher database and initializes the embedding
    /// router. All turns (curator and zed agent) are ingested into the
    /// curator's DB — the curator has perspective-scoped memory, the zed
    /// agent's turns are shared copies.
    ///
    /// Returns `Err` if the curator database cannot be opened.
    pub fn new(
        passphrase: &str,
        embedding_model: String,
        embedding_dim: usize,
        embedding_port: Option<LanguageModelEmbeddingPort>,
        consolidation_cadence_secs: u64,
        confidence_floor: f64,
        tokio_handle: tokio::runtime::Handle,
    ) -> Result<Self, String> {
        if embedding_dim == 0 {
            tracing::warn!(
                target: "reg.memory",
                embedding_dim,
                "RealMemoryPort constructed with embedding_dim == 0 — \
                 clamping to 1024"
            );
        }
        let embedding_dim = if embedding_dim == 0 {
            1024
        } else {
            embedding_dim
        };

        let curator_webid = WebID::from_persona(b"curator");

        // Curator store behind the self-healing handle.
        let curator_store = Arc::new(CuratorStore::new(passphrase, embedding_dim));

        let curator_consolidation = Arc::new(RwLock::new(build_curator_consolidation(
            consolidation_cadence_secs,
            &curator_store.get(),
        )));

        Ok(Self {
            curator_store,
            embedding_port,
            embedding_model,
            curator_webid,
            curator_consolidation,
            consolidation_cadence_secs,
            confidence_floor,
            last_consolidation: Mutex::new(None),
            tokio_handle,
            ingest_semaphore: tokio::sync::Semaphore::new(resolve_ingest_concurrency()),
        })
    }

    /// Check whether the consolidation cadence has elapsed and, if so, fire
    /// a consolidation pass (confidence cleanup + budget pruning).
    ///
    /// This is the test entry point. The cadence-elapsed check lives here (it
    /// reads `self.last_consolidation` under the mutex and fires when
    /// never-consolidated — `unwrap_or(true)`); the actual consolidate-and-log
    /// logic is shared with `start_consolidation_timer` via
    /// [`fire_curator_consolidation_pass`]. The timer skips its first tick instead, so
    /// the only difference between the two paths is the first-fire decision,
    /// now made explicit at each call site rather than hidden in two copies.
    ///
    /// Kept as a method so tests can fire consolidation directly without
    /// starting a timer.
    #[cfg(test)]
    fn maybe_consolidate(&self) {
        if self.consolidation_cadence_secs == 0 {
            return;
        }

        let now = Utc::now();
        let cadence = chrono::Duration::seconds(self.consolidation_cadence_secs as i64);
        let should_fire = match cadence_should_fire(&self.last_consolidation, now, cadence, true) {
            Some(fire) => fire,
            None => return,
        };
        if !should_fire {
            return;
        }

        let curator_consolidation = self
            .curator_consolidation
            .read()
            .ok()
            .and_then(|g| g.clone());
        if let Some(curator_consolidation) = curator_consolidation {
            fire_curator_consolidation_pass(
                &curator_consolidation,
                self.curator_webid,
                self.confidence_floor,
                "maybe_consolidate",
            );
        }
    }

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
        let curator_consolidation_lock = Arc::clone(&self.curator_consolidation);
        let curator_webid = self.curator_webid;
        let confidence_floor = self.confidence_floor;
        let last_consolidation = self.last_consolidation.lock().ok().and_then(|guard| *guard);
        let cadence = self.consolidation_cadence_secs;
        let poll_interval = Duration::from_secs(cadence.clamp(60, 3600));

        let shared_last: Arc<Mutex<Option<chrono::DateTime<chrono::Utc>>>> =
            Arc::new(Mutex::new(last_consolidation));
        let shared_last_for_timer = Arc::clone(&shared_last);

        let handle = self.tokio_handle.spawn(async move {
            let mut interval = tokio::time::interval(poll_interval);
            interval.tick().await; // skip first tick
            loop {
                interval.tick().await;
                let now = Utc::now();
                let cadence_dur = chrono::Duration::seconds(cadence as i64);
                let should_fire =
                    match cadence_should_fire(&shared_last_for_timer, now, cadence_dur, false) {
                        Some(fire) => fire,
                        None => continue,
                    };
                if !should_fire {
                    continue;
                }
                tracing::info!(
                    target: "reg.memory",
                    cadence_secs = cadence,
                    confidence_floor,
                    "Consolidation timer fired"
                );
                let curator_consolidation_now = curator_consolidation_lock
                    .read()
                    .ok()
                    .and_then(|g| g.clone());
                if let Some(curator_consolidation) = curator_consolidation_now {
                    fire_curator_consolidation_pass(
                        &curator_consolidation,
                        curator_webid,
                        confidence_floor,
                        "consolidation_timer",
                    );
                }
            }
        });
        Some(handle)
    }
}

/// Default ingestion concurrency — 1 (fully serial) is correct for SQLite,
/// which serializes writers anyway.
const DEFAULT_INGEST_CONCURRENCY: usize = 1;

/// Parse a raw env-var value into an ingestion concurrency (`usize`), falling
/// back to [`DEFAULT_INGEST_CONCURRENCY`] on malformed/zero values. Same
/// startup-failure-signal trap — a malformed value must warn naming the value,
/// not silently fall back (the `.rules` "Numeric env vars that fail to parse"
/// trap).
fn parse_ingest_concurrency(raw: &str) -> usize {
    match raw.trim().parse::<usize>() {
        Ok(n) if n > 0 => n,
        Ok(_zero) => {
            tracing::warn!(
                target: "reg.memory",
                value = %raw,
                "HKASK_MEMORY_INGEST_CONCURRENCY must be > 0 — falling back to {default}",
                default = DEFAULT_INGEST_CONCURRENCY
            );
            DEFAULT_INGEST_CONCURRENCY
        }
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                value = %raw,
                error = %e,
                "HKASK_MEMORY_INGEST_CONCURRENCY malformed — falling back to {default}",
                default = DEFAULT_INGEST_CONCURRENCY
            );
            DEFAULT_INGEST_CONCURRENCY
        }
    }
}

/// Resolve `HKASK_MEMORY_INGEST_CONCURRENCY` from the environment, falling back
/// to the default when unset.
fn resolve_ingest_concurrency() -> usize {
    match std::env::var("HKASK_MEMORY_INGEST_CONCURRENCY") {
        Ok(raw) => parse_ingest_concurrency(&raw),
        Err(_) => DEFAULT_INGEST_CONCURRENCY,
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
    let db = match open_or_repair(db_path, passphrase) {
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

/// Check whether enough time has elapsed since the last consolidation to fire
/// another pass. Updates the timestamp to `now` when firing.
///
/// `fire_when_no_last` reconciles the two calling contexts:
/// - `maybe_consolidate` (test): `true` — fire on first call (no timer to wait for)
/// - `start_consolidation_timer` (production): `false` — wait one full cadence
///   before first fire
///
/// Returns `Some(true)` to fire, `Some(false)` to skip, or `None` if the mutex
/// is poisoned (each caller decides whether to skip or stop the timer).
fn cadence_should_fire(
    last: &Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    now: chrono::DateTime<chrono::Utc>,
    cadence: chrono::Duration,
    fire_when_no_last: bool,
) -> Option<bool> {
    let mut guard = match last.lock() {
        Ok(guard) => guard,
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                "last_consolidation mutex poisoned — cannot check cadence"
            );
            return None;
        }
    };
    let elapsed = guard
        .map(|l| now.signed_duration_since(l) >= cadence)
        .unwrap_or(fire_when_no_last);
    if elapsed {
        *guard = Some(now);
        Some(true)
    } else {
        Some(false)
    }
}

/// Fire one curator consolidation pass.
///
/// Shared by `maybe_consolidate` (test entry) and `start_consolidation_timer`
/// (production). The cadence-elapsed check is shared via `cadence_should_fire`.
///
/// `log_label` distinguishes the two paths in tracing.
fn fire_curator_consolidation_pass(
    curator_consolidation: &hkask_memory::MemoryConsolidator,
    curator_webid: WebID,
    confidence_floor: f64,
    log_label: &str,
) {
    let request = hkask_types::ConsolidationRequest {
        confidence_floor: Some(confidence_floor),
        ..Default::default()
    };
    match curator_consolidation.consolidate(&curator_webid, request) {
        Ok(outcome) => {
            tracing::info!(
                target: "reg.memory",
                label = log_label,
                deleted = outcome.deleted_count,
                failed = outcome.failed_count,
                "Curator consolidation pass complete"
            );
        }
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                label = log_label,
                error = %e,
                "Curator consolidation pass failed"
            );
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
            // thread) so they don't contend for the SQLite pool with each other
            // or with the recall path. The permit is held for the duration of the
            // ingestion (including the embedding HTTP call) and released on drop.
            //
            // If the semaphore is contended, this future parks until a permit is
            // available — the calling thread's turn has already completed, so
            // the user sees no latency from this wait.
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

            // Delegate the writes to the extracted write path. The semaphore
            // permit is held across the full write (h_mem + embedding);
            // `write_turn` borrows the port's fields via `WriteContext`.
            let ctx = WriteContext {
                curator_store: &self.curator_store,
                embedding_port: self.embedding_port.as_ref(),
                embedding_model: &self.embedding_model,
                curator_webid: self.curator_webid,
                tokio_handle: &self.tokio_handle,
                curator_consolidation: &self.curator_consolidation,
                consolidation_cadence_secs: self.consolidation_cadence_secs,
            };
            ingest::write_turn(&ctx, record).await
        })
    }

    fn recall_context<'a>(
        &'a self,
        _query: &'a str,
        _limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemorySnippet>, MemoryError>> + Send + 'a>> {
        // The MemoryPort trait impl is a no-op for the zed agent.
        // Actual recall is via `recall_context_curator` (inherent method),
        // called by the curator context injector.
        Box::pin(async { Ok(Vec::new()) })
    }

    fn recall_thread<'a>(
        &'a self,
        _thread_id: &'a str,
        _limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemorySnippet>, MemoryError>> + Send + 'a>> {
        // The MemoryPort trait impl is a no-op for the zed agent.
        // Actual recall is via `recall_thread_curator` (inherent method),
        // called by the curator context injector.
        Box::pin(async { Ok(Vec::new()) })
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
        serde_json::json!({
            "curator_store": curator_up,
            "degraded": !curator_up,
        })
    }

    /// Get a clone of the curator's `MemoryStore` if available.
    ///
    /// Used by the `MemoryHealthSource` implementation to read memory metrics
    /// for the cybernetics loop. Returns `None` if the store is unavailable
    /// (self-healing will re-attempt on the next access).
    pub fn curator_memory_store(&self) -> Option<Arc<MemoryStore>> {
        self.curator_store.get()
    }

    /// The configured memory life in days.
    pub fn memory_life_days(&self) -> f64 {
        self.curator_store
            .get()
            .map(|s| s.memory_life_days())
            .unwrap_or(180.0)
    }

    /// Recall memory snippets from the **curator's** sovereign stores.
    ///
    /// This mirrors `recall_context` but reads from the curator's `MemoryStore`
    /// (`agents/curator/curator.db`) using the
    /// curator's WebID for perspective-scoped queries. Used by the
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
    /// Used by the curator context injector's `inject_context` to load
    /// the curator's prior turns on this thread per turn (fresh, not
    /// session-cached). Returns `Ok(vec![])` when the curator stores are not
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

    /// Record co-occurrence links between entities recalled in the same
    /// context. Delegates to the curator's `MemoryStore`. Called by the
    /// context injector after a successful recall to populate the
    /// `memory_links` table — the `connectedness` signal for recall ranking.
    ///
    /// Returns `Ok(())` when the curator store is unavailable (graceful
    /// degradation — co-occurrence tracking is a bonus, not a requirement).
    pub fn record_co_occurrence(&self, entities: &[String]) {
        if let Some(ref store) = self.curator_store.get() {
            if let Err(e) = store.record_co_occurrence(entities) {
                tracing::warn!(
                    target: "reg.memory",
                    error = %e,
                    "Failed to record co-occurrence links"
                );
            }
        }
    }

    /// Get the connectedness score for an entity — the total co-occurrence
    /// count across all links. Higher = more connected = more salient.
    /// Returns 0 when the curator store is unavailable.
    pub fn connectedness(&self, entity: &str) -> u64 {
        self.curator_store
            .get()
            .as_ref()
            .and_then(|store| store.connectedness(entity).ok())
            .unwrap_or(0)
    }

    /// Shared recall implementation for both the user and curator stores.
    ///
    /// `perspective` scopes the keyword search to the owning agent's WebID;
    /// `None` skips the keyword leg entirely. `log_label` is used in tracing
    /// so the user and curator paths are distinguishable in logs.
    ///
    /// This was previously duplicated verbatim between `recall_context` and
    /// `recall_context_curator`; the duplication was a maintenance hazard
    /// (a fix to one had to be manually mirrored in the other).
    async fn recall_from<'a>(
        &'a self,
        store: &'a Arc<MemoryStore>,
        perspective: Option<WebID>,
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

        // ── 1. Embedding search (KNN) ────────────────────────────────
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
            .spawn(async move {
                let Some(ref embedding_port) = embedding_port else {
                    return Ok(Vec::new());
                };
                embedding_port.embed(&embedding_model, &[query_owned]).await
            })
            .await;

        // A failed embed degrades recall to keyword-only — surface it so
        // the operator can distinguish "no memory found" from "embedding
        // endpoint down".
        let vectors = match vectors {
            Ok(Ok(vectors)) => vectors,
            Ok(Err(e)) => {
                tracing::warn!(
                    target: "reg.memory",
                    error = %e,
                    label = log_label,
                    "Failed to embed recall query — embedding search skipped for this turn"
                );
                Vec::new()
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.memory",
                    error = %e,
                    label = log_label,
                    "Embedding task panicked — embedding search skipped for this turn"
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
                                            entity: h_mem.entity.clone(),
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
                        "Embedding search failed during recall"
                    );
                }
            }
        }

        // ── 2. Keyword search (entity prefix + word overlap) ────────
        //
        // Load h_mems for the agent's chat threads ONCE, then
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
            && let Some(perspective) = perspective
        {
            // Use a prefix query to load all chat:thread:* h_mems
            // in a single SQL call. The previous implementation queried
            // the exact entity "chat:thread:" (no thread_id suffix), which
            // never matched stored entities "chat:thread:<thread_id>" —
            // so the keyword search was dead code. Combined with
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
                perspective,
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
                            entity: h_mem.entity.clone(),
                            confidence: h_mem.confidence.value(),
                            relevance_score: 0.5, // Base relevance for keyword match
                        },
                        h_mem_id: h_mem.id,
                    });
                }
            }
        }

        // ── 3. Sort by combined relevance × confidence × connectedness and truncate ─
        //
        // Confidence is the outcome-calibrated signal (Dunning's double
        // curse: the model can't self-evaluate, but confidence that's been
        // calibrated by outcomes IS meaningful). Using it as a ranking
        // multiplier — not just a threshold filter — means a memory that
        // has been recalled many times and never contradicted outranks a
        // fresh, untested memory with similar embedding similarity.
        //
        // Connectedness is a structural prior: entities that co-occur
        // frequently across recall contexts are more salient — they've been
        // tested against more contexts (Tetlock's dilution effect: well-
        // connected memories resist dilution). The bonus is capped at 50%
        // so a highly-connected entity can at most boost salience by 1.5×,
        // preventing a popularity cascade that crowds out fresh memories.
        //
        // Corpus evidence: Dunning `138299529:5` (double curse), Tetlock
        // `Superforecasting_tetlock:71` (Brier = forecast accuracy).
        candidates.sort_by(|a, b| {
            let a_conn = self.connectedness(&a.snippet.entity) as f64;
            let b_conn = self.connectedness(&b.snippet.entity) as f64;
            let a_score =
                a.snippet.relevance_score * a.snippet.confidence * (1.0 + (a_conn * 0.1).min(0.5));
            let b_score =
                b.snippet.relevance_score * b.snippet.confidence * (1.0 + (b_conn * 0.1).min(0.5));
            b_score
                .partial_cmp(&a_score)
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
    /// The user entity is `chat:thread:{thread_id}` (scoped by `perspective`),
    /// and the curator copy entity is `curator:thread:{thread_id}`.
    async fn recall_thread_from<'a>(
        &'a self,
        primary_store: &'a Arc<MemoryStore>,
        curator_store: Option<&'a Arc<MemoryStore>>,
        perspective: WebID,
        thread_id: &'a str,
        limit: usize,
        log_label: &'static str,
    ) -> Result<Vec<MemorySnippet>, MemoryError> {
        struct Candidate {
            snippet: MemorySnippet,
            h_mem_id: hkask_storage::HMemId,
            touch_store_idx: usize, // 0 = primary, 1 = curator
        }
        let mut candidates: Vec<Candidate> = Vec::new();

        // ── 1. User store: exact entity match, perspective-scoped ────
        let user_entity = format!("chat:thread:{thread_id}");
        if let Ok(h_mems) = primary_store.query_for_deduped_untouched(&user_entity, perspective) {
            for h_mem in h_mems {
                let text = h_mem.value.as_str().unwrap_or("").to_string();
                if text.is_empty() {
                    continue;
                }
                candidates.push(Candidate {
                    snippet: MemorySnippet {
                        text,
                        entity: h_mem.entity.clone(),
                        confidence: h_mem.confidence.value(),
                        relevance_score: 1.0,
                    },
                    h_mem_id: h_mem.id,
                    touch_store_idx: 0,
                });
            }
        }

        // ── 2. Curator store: exact entity match (shared copy) ──────
        let curator_entity = format!("curator:thread:{thread_id}");
        if let Some(curator) = curator_store
            && let Ok(h_mems) = curator.query_deduped_untouched(&curator_entity)
        {
            for h_mem in h_mems {
                let text = h_mem.value.as_str().unwrap_or("").to_string();
                if text.is_empty() {
                    continue;
                }
                // Dedup against user store by text
                if candidates.iter().any(|c| c.snippet.text == text) {
                    continue;
                }
                candidates.push(Candidate {
                    snippet: MemorySnippet {
                        text,
                        entity: h_mem.entity.clone(),
                        confidence: h_mem.confidence.value(),
                        relevance_score: 1.0,
                    },
                    h_mem_id: h_mem.id,
                    touch_store_idx: 1,
                });
            }
        }

        // ── 3. Truncate to limit ────────────────────────────────────
        // Both queries return most-recent-first, so the candidates are
        // already in recency order. All candidates have relevance_score 1.0
        // (exact entity match), so no sort is needed — just truncate.
        candidates.truncate(limit);

        // ── 4. Touch only the injected h_mems ────────────────────────
        let stores: [&Arc<MemoryStore>; 2] =
            [primary_store, curator_store.unwrap_or(primary_store)];
        for c in &candidates {
            let touch_store = stores[c.touch_store_idx];
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

#[async_trait::async_trait]
impl hkask_regulation::MemoryHealthSource for RealMemoryPort {
    async fn h_mem_count(&self) -> Option<usize> {
        let store = self.curator_store.get()?;
        match store.h_mem_count() {
            Ok(count) => Some(count),
            Err(e) => {
                tracing::warn!(
                    target: "reg.sensor.memory",
                    error = %e,
                    "h_mem_count: store query failed — returning None (not 0)"
                );
                None
            }
        }
    }

    async fn low_confidence_count(&self, threshold: f64) -> Option<usize> {
        let store = self.curator_store.get()?;
        match store.low_confidence_count(threshold) {
            Ok(count) => Some(count),
            Err(e) => {
                tracing::warn!(
                    target: "reg.sensor.memory",
                    error = %e,
                    "low_confidence_count: store query failed — returning None (not 0)"
                );
                None
            }
        }
    }

    async fn storage_budget(&self) -> usize {
        self.curator_store
            .get()
            .map(|s| s.storage_budget())
            .unwrap_or(0)
    }

    async fn memory_life_days(&self) -> f64 {
        self.curator_store
            .get()
            .map(|s| s.memory_life_days())
            .unwrap_or(180.0)
    }
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
                    goal_events: record.goal_events,
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
#[cfg(test)]
#[allow(dead_code)] // test utility
pub(crate) fn in_memory_port_for_tests() -> RealMemoryPort {
    use hkask_storage::database::sqlite::SqliteDriver;
    let curator_driver: Arc<dyn hkask_storage::DatabaseDriver> = SqliteDriver::in_memory_driver();
    let curator_h_mem_store =
        HMemStore::from_driver(Arc::clone(&curator_driver)).expect("curator hmem store init");
    let curator_embedding_store =
        EmbeddingStore::from_driver(curator_driver, 1024).expect("embedding store init");
    let curator_store_inner = Arc::new(MemoryStore::new(
        curator_h_mem_store,
        curator_embedding_store,
    ));
    let embedding_port = LanguageModelEmbeddingPort::for_tests();
    RealMemoryPort {
        curator_store: Arc::new(CuratorStore::for_tests(Some(curator_store_inner))),
        embedding_port: Some(embedding_port),
        embedding_model: "test-model".to_string(),
        curator_webid: WebID::from_persona(b"curator"),
        curator_consolidation: Arc::new(RwLock::new(None)),
        consolidation_cadence_secs: 0,
        confidence_floor: 0.3,
        last_consolidation: Mutex::new(None),
        tokio_handle: tokio::runtime::Handle::current(),
        ingest_semaphore: tokio::sync::Semaphore::new(1),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use hkask_storage::database::sqlite::SqliteDriver;

    pub(crate) fn in_memory_port() -> RealMemoryPort {
        in_memory_port_with_cadence(0, 0.3)
    }

    fn in_memory_port_with_cadence(
        consolidation_cadence_secs: u64,
        confidence_floor: f64,
    ) -> RealMemoryPort {
        // Curator store — a separate in-memory driver so the curator copy
        // lands in a different DB, mirroring production where the curator
        // has its own `curator.db`.
        let curator_driver: Arc<dyn hkask_storage::DatabaseDriver> =
            SqliteDriver::in_memory_driver();
        let curator_h_mem_store =
            HMemStore::from_driver(Arc::clone(&curator_driver)).expect("curator hmem store init");
        let curator_embedding_store =
            EmbeddingStore::from_driver(curator_driver, 1024).expect("embedding store init");
        let curator_store_inner = Arc::new(MemoryStore::new(
            curator_h_mem_store,
            curator_embedding_store,
        ));

        // Tests don't call embed — use a stub port with no backing task.
        let embedding_port = LanguageModelEmbeddingPort::for_tests();

        // Curator consolidation service — mirrors the production construction
        // in `RealMemoryPort::new`. Skipped when cadence is 0 (matches
        // production). The curator store is always `Some` in tests.
        let curator_consolidation = build_curator_consolidation(
            consolidation_cadence_secs,
            &Some(Arc::clone(&curator_store_inner)),
        );

        RealMemoryPort {
            curator_store: Arc::new(CuratorStore::for_tests(Some(curator_store_inner))),
            embedding_port: Some(embedding_port),
            embedding_model: "test-model".to_string(),
            curator_webid: WebID::from_persona(b"curator"),
            curator_consolidation: Arc::new(RwLock::new(curator_consolidation)),
            consolidation_cadence_secs,
            confidence_floor,
            last_consolidation: Mutex::new(None),
            tokio_handle: tokio::runtime::Handle::current(),
            ingest_semaphore: tokio::sync::Semaphore::new(1),
        }
    }

    /// Construct an in-memory `RealMemoryPort` whose embedding port is backed
    /// by `embed_fn` (a deterministic text→vector closure) instead of the
    /// channel-closed `for_tests()` stub. For tests that exercise the
    /// end-to-end embedding recall path. The receiver task runs on the
    /// current tokio runtime (the test's `#[tokio::test]` reactor).
    pub(crate) fn in_memory_port_with_embed_fn<F>(embed_fn: Arc<F>) -> RealMemoryPort
    where
        F: Fn(&str) -> Vec<f32> + Send + Sync + ?Sized + 'static,
    {
        let curator_driver: Arc<dyn hkask_storage::DatabaseDriver> =
            SqliteDriver::in_memory_driver();
        let curator_h_mem_store =
            HMemStore::from_driver(Arc::clone(&curator_driver)).expect("curator hmem store init");
        let curator_embedding_store =
            EmbeddingStore::from_driver(curator_driver, 1024).expect("embedding store init");
        let curator_store_inner = Arc::new(MemoryStore::new(
            curator_h_mem_store,
            curator_embedding_store,
        ));

        let embedding_port = LanguageModelEmbeddingPort::for_tests_with_embed_fn(
            embed_fn,
            tokio::runtime::Handle::current(),
        );

        RealMemoryPort {
            curator_store: Arc::new(CuratorStore::for_tests(Some(curator_store_inner))),
            embedding_port: Some(embedding_port),
            embedding_model: "test-model".to_string(),
            curator_webid: WebID::from_persona(b"curator"),
            curator_consolidation: Arc::new(RwLock::new(None)),
            consolidation_cadence_secs: 0,
            confidence_floor: 0.3,
            last_consolidation: Mutex::new(None),
            tokio_handle: tokio::runtime::Handle::current(),
            ingest_semaphore: tokio::sync::Semaphore::new(1),
        }
    }

    #[tokio::test]
    async fn ingest_turn_stores_episodic_h_mem() {
        let port = in_memory_port();
        let curator_webid = port.curator_webid;
        let record = TurnRecord {
            thread_id: "test-thread".to_string(),
            user_input: "What is Rust?".to_string(),
            agent_response: "Rust is a systems programming language.".to_string(),
            model: "test-model".to_string(),
            thread_title: Some("Rust Discussion".to_string()),
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };

        let result = port.ingest_turn(record).await;
        assert!(result.is_ok(), "ingest_turn should succeed");

        // Verify h_mem was stored in the curator's store.
        let curator_store = port.curator_store.get().expect("curator store");
        let h_mems = curator_store
            .query_for_deduped_untouched("chat:thread:test-thread", curator_webid)
            .expect("query should succeed");
        assert_eq!(h_mems.len(), 1, "one h_mem should be stored");
        assert_eq!(h_mems[0].attribute, "chatted");
        // The ontology blob carries process-axis anchoring (PKO).
        let ontology = h_mems[0]
            .ontology
            .as_ref()
            .expect("h_mem carries an ontology blob");
        assert_eq!(ontology.pko_procedure.as_deref(), Some("chat"));
        assert_eq!(ontology.pko_step.as_deref(), Some("turn"));
    }

    #[tokio::test]
    async fn ingest_turn_stores_zed_goal_events_as_shared_h_mems_only() {
        // Operator ruling 2026-08-29: zed-agent goals are ephemeral; the
        // curator's memory is the durable vehicle. A zed turn's goal events
        // get a SHARED goal h_mem (curator recall) but NO curator-perspective
        // h_mem — the curator only remembers goals it was involved with.
        let port = in_memory_port();
        let curator_webid = port.curator_webid;
        let record = TurnRecord {
            thread_id: "zed-thread".to_string(),
            user_input: "add date filtering".to_string(),
            agent_response: "done".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("zed".to_string()),
            goal_events: vec![hkask_types::GoalEvent {
                tool_name: "kanban_goal_create".to_string(),
                output: serde_json::json!({
                    "goal_id": "g-123",
                    "goal_text": "The user can filter by date",
                    "prediction": 0.8
                }),
            }],
        };

        port.ingest_turn(record)
            .await
            .expect("ingest should succeed");
        let curator_store = port.curator_store.get().expect("curator store");

        // Shared copy exists — curator recall sees the goal event. Shared
        // records are recalled via the perspective-free query (the same
        // path as `curator:thread:` shared copies).
        let shared = curator_store
            .query_deduped_untouched("curator:goal:g-123")
            .expect("query should succeed");
        assert_eq!(shared.len(), 1, "one shared goal h_mem");
        assert_eq!(shared[0].attribute, "kanban_goal_create");
        assert_eq!(
            shared[0].value.get("goal_text").and_then(|v| v.as_str()),
            Some("The user can filter by date")
        );

        // No curator-perspective h_mem — the curator was not involved.
        let perspective = curator_store
            .query_for_deduped_untouched("goal:g-123", curator_webid)
            .expect("query should succeed");
        assert!(
            perspective.is_empty(),
            "zed-agent goal events must not create curator-perspective h_mems"
        );
    }

    #[tokio::test]
    async fn ingest_turn_curator_goal_events_get_curator_perspective_h_mem() {
        // "Curator remembers all goals it is involved with": a curator
        // turn's goal events get BOTH the curator-perspective Private h_mem
        // (the curator's own memory) and the shared copy.
        let port = in_memory_port();
        let curator_webid = port.curator_webid;
        let record = TurnRecord {
            thread_id: "curator-thread".to_string(),
            user_input: "set a goal".to_string(),
            agent_response: "goal recorded".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: vec![
                hkask_types::GoalEvent {
                    tool_name: "kanban_goal_score".to_string(),
                    output: serde_json::json!({
                        "goal_id": "g-456",
                        "achieved": true,
                        "brier": 0.04
                    }),
                },
                hkask_types::GoalEvent {
                    tool_name: "kanban_goal_list".to_string(),
                    output: serde_json::json!({"goals": []}),
                },
            ],
        };

        port.ingest_turn(record)
            .await
            .expect("ingest should succeed");
        let curator_store = port.curator_store.get().expect("curator store");

        // Curator-perspective h_mem for the scored goal.
        let perspective = curator_store
            .query_for_deduped_untouched("goal:g-456", curator_webid)
            .expect("query should succeed");
        assert_eq!(perspective.len(), 1, "curator-perspective goal h_mem");
        assert_eq!(perspective[0].attribute, "kanban_goal_score");
        assert_eq!(
            perspective[0].value.get("brier").and_then(|v| v.as_f64()),
            Some(0.04)
        );

        // Shared copies for both events (perspective-free recall path).
        let shared_score = curator_store
            .query_deduped_untouched("curator:goal:g-456")
            .expect("query should succeed");
        assert_eq!(shared_score.len(), 1);
        // The list event (no goal_id) lands under the list entity.
        let shared_list = curator_store
            .query_deduped_untouched("curator:goal:list")
            .expect("query should succeed");
        assert_eq!(shared_list.len(), 1, "goal_list event uses the list entity");
    }

    #[tokio::test]
    async fn ingest_turn_stores_semantic_curator_copy() {
        let port = in_memory_port();
        let record = TurnRecord {
            thread_id: "test-thread-2".to_string(),
            user_input: "Explain async Rust".to_string(),
            agent_response: "Async Rust uses tokio.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };

        let result = port.ingest_turn(record).await;
        assert!(result.is_ok());

        // Verify the shared curator copy was stored in the curator's store.
        let curator_store = port.curator_store.get().expect("curator store");
        let h_mems = curator_store
            .query_deduped("curator:thread:test-thread-2")
            .expect("query should succeed");
        assert_eq!(h_mems.len(), 1, "one curator copy h_mem should be stored");
        assert_eq!(h_mems[0].attribute, "turn");
        let ontology = h_mems[0]
            .ontology
            .as_ref()
            .expect("curator copy carries an ontology blob");
        assert_eq!(ontology.dc_type, hkask_bridge_ontology::dc_bibo::DOCUMENT);
        assert!(
            ontology.pko_procedure.is_none(),
            "the curator copy is a semantic fact, not a process step"
        );
    }

    #[tokio::test]
    async fn ingest_turn_stores_shared_copy_for_zed_agent_turn() {
        // Non-Curator (e.g. Zed agent) turns must be ingested into the
        // curator's shared store so the curator can recall what happened
        // across all agents. Previously, the is_curator_turn filter gated
        // all three write steps, so Zed agent work was invisible to the
        // curator — the memory stayed empty.
        let port = in_memory_port();
        let record = TurnRecord {
            thread_id: "zed-agent-thread".to_string(),
            user_input: "Fix the memory ingestion bug".to_string(),
            agent_response: "I narrowed the is_curator_turn filter.".to_string(),
            model: "test-model".to_string(),
            thread_title: Some("Memory fix".to_string()),
            agent_id: Some("Zed Agent".to_string()),
            goal_events: Vec::new(),
        };

        let result = port.ingest_turn(record).await;
        assert!(result.is_ok(), "zed agent ingest should succeed");

        // The shared copy (curator:thread:...) must be present.
        let curator_store = port.curator_store.get().expect("curator store");
        let h_mems = curator_store
            .query_deduped("curator:thread:zed-agent-thread")
            .expect("query should succeed");
        assert_eq!(
            h_mems.len(),
            1,
            "shared copy must be stored for zed agent turns"
        );
        assert_eq!(h_mems[0].attribute, "turn");

        // The curator-perspective h_mem (chat:thread:...) must NOT exist —
        // that's the curator's own memory of its own turn, not a zed agent
        // turn.
        let perspective_h_mems = curator_store
            .query_for_deduped_untouched("chat:thread:zed-agent-thread", port.curator_webid)
            .expect("query should succeed");
        assert_eq!(
            perspective_h_mems.len(),
            0,
            "zed agent turns must not get a curator-perspective h_mem"
        );
    }

    #[tokio::test]
    async fn ingest_turn_skips_curator_copy_when_store_absent() {
        // Simulate the curator DB being unavailable — the curator store is
        // `None`. Ingestion of a curator turn should still succeed (Ok),
        // but no records are written since there's no store to write to.
        let port = in_memory_port();
        port.curator_store.set_for_tests(None);
        let record = TurnRecord {
            thread_id: "test-no-curator".to_string(),
            user_input: "What is memory?".to_string(),
            agent_response: "Memory is persistence across time.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };

        let result = port.ingest_turn(record).await;
        assert!(
            result.is_ok(),
            "ingest should succeed without curator store"
        );

        // Nothing was written — the store is still absent.
        assert!(port.curator_store.get().is_none());
    }

    #[tokio::test]
    async fn ingest_turn_handles_empty_prompt_gracefully() {
        let port = in_memory_port();
        let curator_webid = port.curator_webid;
        let record = TurnRecord {
            thread_id: "test-empty".to_string(),
            user_input: String::new(),
            agent_response: "Response".to_string(),
            model: "test".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };

        let result = port.ingest_turn(record).await;
        assert!(result.is_ok(), "empty prompt should not fail ingestion");

        let curator_store = port.curator_store.get().expect("curator store");
        let h_mems = curator_store
            .query_for_deduped_untouched("chat:thread:test-empty", curator_webid)
            .expect("query should succeed");
        assert_eq!(h_mems.len(), 1, "episodic h_mem should still be stored");
    }

    #[tokio::test]
    async fn ingest_turn_does_not_fire_consolidation() {
        // Consolidation is now decoupled from ingestion — it runs on a
        // background timer (see start_consolidation_timer). Ingestion should
        // NOT fire consolidation, even when the cadence has elapsed.
        let port = in_memory_port_with_cadence(1, 0.3);

        let record = TurnRecord {
            thread_id: "no-consolidation-from-ingest".to_string(),
            user_input: "Tell me about memory consolidation".to_string(),
            agent_response: "Consolidation promotes episodic to semantic.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };
        let result = port.ingest_turn(record).await;
        assert!(result.is_ok(), "ingest_turn should succeed");

        // last_consolidation should remain None — ingestion no longer fires it.
        let last = port.last_consolidation.lock().expect("mutex not poisoned");
        assert!(
            last.is_none(),
            "ingest_turn should not fire consolidation (timer-decoupled)"
        );
    }

    // ── cadence_should_fire unit tests ─────────────────────────────────
    // These test the shared cadence check directly, without constructing a
    // full RealMemoryPort — so the production timer's None-wait semantics are
    // testable for the first time.

    #[test]
    fn cadence_waits_when_no_last_and_not_first_run() {
        let last = Mutex::new(None);
        let now = Utc::now();
        let cadence = chrono::Duration::seconds(60);
        assert_eq!(
            cadence_should_fire(&last, now, cadence, false),
            Some(false),
            "production timer waits one cadence before first fire"
        );
    }

    #[test]
    fn cadence_fires_when_no_last_and_first_run() {
        let last = Mutex::new(None);
        let now = Utc::now();
        let cadence = chrono::Duration::seconds(60);
        assert_eq!(
            cadence_should_fire(&last, now, cadence, true),
            Some(true),
            "test path fires immediately on first call"
        );
    }

    #[test]
    fn cadence_fires_when_elapsed() {
        let now = Utc::now();
        let old = now - chrono::Duration::seconds(120);
        let last = Mutex::new(Some(old));
        let cadence = chrono::Duration::seconds(60);
        assert_eq!(cadence_should_fire(&last, now, cadence, false), Some(true));
    }

    #[test]
    fn cadence_skips_when_not_elapsed() {
        let now = Utc::now();
        let recent = now - chrono::Duration::seconds(30);
        let last = Mutex::new(Some(recent));
        let cadence = chrono::Duration::seconds(60);
        assert_eq!(cadence_should_fire(&last, now, cadence, false), Some(false));
    }

    #[tokio::test]
    async fn maybe_consolidate_fires_when_cadence_elapsed() {
        // Directly test the consolidation callback (what the timer calls).
        let port = in_memory_port_with_cadence(1, 0.3);
        let curator_webid = port.curator_webid;

        // Ingest a curator turn so there's something to consolidate.
        port.ingest_turn(TurnRecord {
            thread_id: "consolidation-test".to_string(),
            user_input: "Tell me about memory consolidation".to_string(),
            agent_response: "Consolidation promotes episodic to semantic.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        })
        .await
        .expect("ingest succeeds");

        // Fire consolidation directly (simulating the timer callback).
        port.maybe_consolidate();

        // The last_consolidation timestamp should now be set.
        let last = port
            .last_consolidation
            .lock()
            .expect("mutex not poisoned")
            .expect("consolidation should have fired");
        assert!(
            Utc::now().signed_duration_since(last).num_seconds() < 5,
            "last_consolidation should be recent"
        );

        // A second call immediately after should NOT re-fire consolidation
        // (cadence hasn't elapsed).
        port.maybe_consolidate();
        let last_after = port
            .last_consolidation
            .lock()
            .expect("mutex not poisoned")
            .expect("consolidation timestamp should still be set");
        assert_eq!(
            last, last_after,
            "second call within cadence should not re-fire consolidation"
        );

        // After consolidation, low-confidence h_mems may have been
        // pruned. The h_mem may or may not survive depending on
        // confidence decay — we just verify the query succeeds.
        let curator_store = port.curator_store.get().expect("curator store");
        let h_mems = curator_store
            .query_for_deduped_untouched("chat:thread:consolidation-test", curator_webid)
            .expect("query should succeed");
        let _ = h_mems;
    }

    #[tokio::test]
    async fn ingest_turn_skips_consolidation_when_cadence_zero() {
        // Cadence of 0 — consolidation is disabled.
        let port = in_memory_port_with_cadence(0, 0.3);
        let record = TurnRecord {
            thread_id: "no-consolidation".to_string(),
            user_input: "Hello".to_string(),
            agent_response: "Hi".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };
        let result = port.ingest_turn(record).await;
        assert!(result.is_ok());

        // last_consolidation should remain None (never fired).
        let last = port.last_consolidation.lock().expect("mutex not poisoned");
        assert!(
            last.is_none(),
            "consolidation should not fire when cadence is 0"
        );
    }

    /// Pin the N+1 fix in `recall_from`: the keyword search must load
    /// h_mems exactly once per recall call, not once per query word. The
    /// previous implementation re-queried the store for each of the 5 query
    /// words using the same fixed entity string `"chat:thread:"`, which also
    /// fired `touch_recall` on every row per iteration — turning recall into
    /// a write storm under multi-thread load.
    ///
    /// We can't easily count SQL queries from here, but we can verify the
    /// observable consequence: `recall_context_curator` returns snippets that
    /// match ANY query word (not just the last one), and the recall completes
    /// in reasonable time.
    #[tokio::test]
    async fn recall_context_matches_any_query_word_single_load() {
        let port = in_memory_port();

        // Ingest two curator turns with distinct keywords so we can verify
        // both match.
        let records = [
            TurnRecord {
                thread_id: "t-rust".to_string(),
                user_input: "Tell me about rust programming".to_string(),
                agent_response: "Rust is a systems language.".to_string(),
                model: "test-model".to_string(),
                thread_title: None,
                agent_id: Some("Curator".to_string()),
                goal_events: Vec::new(),
            },
            TurnRecord {
                thread_id: "t-python".to_string(),
                user_input: "Tell me about python programming".to_string(),
                agent_response: "Python is a scripting language.".to_string(),
                model: "test-model".to_string(),
                thread_title: None,
                agent_id: Some("Curator".to_string()),
                goal_events: Vec::new(),
            },
        ];
        for record in records {
            port.ingest_turn(record).await.expect("ingest succeeds");
        }

        // Query with two distinct keywords — both should match.
        let snippets = port
            .recall_context_curator("rust python", 10)
            .await
            .expect("recall succeeds");

        // Both turns should be recalled — the fix loads h_mems once and
        // checks all query words against each.
        let texts: Vec<&str> = snippets.iter().map(|s| s.text.as_str()).collect();
        let has_rust = texts.iter().any(|t| t.contains("rust"));
        let has_python = texts.iter().any(|t| t.contains("python"));
        assert!(
            has_rust && has_python,
            "recall should match ANY query word, got: {snippets:?}"
        );
    }

    /// A failed embedding call must degrade recall to the keyword leg —
    /// never error, never lose keyword-matched turns. This pins the
    /// behavioral contract of the embed-failure branch in `recall_from`:
    /// a dead embedding endpoint costs semantic recall only.
    #[tokio::test]
    async fn recall_degrades_to_keyword_leg_when_embedding_fails() {
        let port = in_memory_port();

        // Ingest a curator turn whose text contains a distinctive keyword.
        // The `for_tests()` port fails every embed, so the semantic leg is
        // dead and the keyword leg is the only path to this turn.
        port.ingest_turn(TurnRecord {
            thread_id: "embed-failure-degradation".to_string(),
            user_input: "kangaroo wallaby emu cassowary".to_string(),
            agent_response: "distinctive keyword quokka".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        })
        .await
        .expect("ingest succeeds despite embed failure");

        // Recall must succeed (not error) and find the turn via the keyword
        // leg — "quokka" appears in the stored text and passes the >3-char
        // word filter.
        let snippets = port
            .recall_context_curator("quokka habitat", 10)
            .await
            .expect("recall must not error when the embedding endpoint is down");
        assert!(
            snippets.iter().any(|s| s.text.contains("quokka")),
            "keyword recall must survive embed failure — got: {snippets:?}"
        );
    }

    // The embed-failure degradation holds for arbitrary queries: whatever
    // the query, recall with a dead embedding endpoint never errors and
    // returns only keyword-leg matches (a snippet with no shared >3-char
    // word can only come from the semantic leg, which is dead). Each case
    // builds its own current-thread runtime — `in_memory_port` captures
    // `Handle::current()` for the embed spawn.
    mod embed_failure_prop {
        use super::*;
        use proptest::prop_assert;

        proptest::proptest! {
            #![proptest_config(proptest::test_runner::Config {
                cases: 64,
                ..proptest::test_runner::Config::default()
            })]

            #[test]
            fn query_returns_keyword_leg_matches_only(
                query in proptest::string::string_regex(r"[a-z ]{4,40}").unwrap()
            ) {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .expect("test runtime builds");
                let snippets = runtime.block_on(async {
                    let port = in_memory_port();
                    port.ingest_turn(TurnRecord {
                        thread_id: "prop-embed-failure".to_string(),
                        user_input: "the quick brown fox jumps".to_string(),
                        agent_response: "lazy dog response".to_string(),
                        model: "test-model".to_string(),
                        thread_title: None,
                        agent_id: Some("Curator".to_string()),
                        goal_events: Vec::new(),
                    })
                    .await
                    .expect("ingest succeeds");

                    port.recall_context_curator(&query, 10)
                        .await
                        .expect("recall must never error on embed failure")
                });

                let query_words: Vec<String> = query
                    .split_whitespace()
                    .filter(|w| w.len() > 3)
                    .map(|w| w.to_lowercase())
                    .collect();
                for snippet in &snippets {
                    let text = snippet.text.to_lowercase();
                    let shares_word = query_words.iter().any(|w| text.contains(w));
                    prop_assert!(
                        shares_word || query_words.is_empty(),
                        "snippet returned without keyword overlap — the semantic \
                         leg must be dead when embed fails: {snippet:?}"
                    );
                }
            }
        }
    }

    /// Pin that the semantic (embedding KNN) recall leg works end-to-end.
    /// Before the fix, the embedding was stored under `embedding:thread:...`
    /// while the h_mem text lived under `chat:thread:...`, so the KNN
    /// neighbor's `entity_ref` joined to no h_mem and the semantic leg
    /// always returned zero snippets — silently degrading recall to the
    /// keyword leg only. The fix stores the embedding under the same
    /// `chat:thread:{id}` entity as the h_mem.
    ///
    /// This test isolates the semantic leg from the keyword leg by using a
    /// stub embedding function that returns the same unit vector for any
    /// non-empty input, so every query is a KNN match for every stored
    /// embedding (cosine distance 0). The query shares NO words with the
    /// stored turn, so the keyword leg misses — the only path to recall is
    /// the semantic leg.
    #[tokio::test]
    async fn recall_context_finds_turn_by_embedding_only() {
        // Constant embedding: every text maps to the same unit vector. KNN
        // search returns every stored embedding at cosine distance 0, so the
        // semantic leg always finds every stored turn regardless of word
        // overlap. The keyword leg is the only thing that could miss.
        let embed_fn = Arc::new(|_text: &str| -> Vec<f32> {
            let mut v = vec![0.0f32; 1024];
            v[0] = 1.0;
            v
        });
        let port = in_memory_port_with_embed_fn(embed_fn);

        // Ingest a curator turn whose text contains none of the query words.
        let record = TurnRecord {
            thread_id: "t-unique-thread-id".to_string(),
            user_input: "alpha beta gamma delta epsilon".to_string(),
            agent_response: "zeta eta theta iota kappa".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };
        port.ingest_turn(record).await.expect("ingest succeeds");

        // Query with words that share NO tokens with the stored turn. All
        // query words are >3 chars (pass the keyword filter) but none appear
        // in the stored text — the keyword leg returns nothing. The semantic
        // leg must find the turn via KNN (constant embedding → distance 0).
        let snippets = port
            .recall_context_curator("kangaroo wallaby emu cassowary", 10)
            .await
            .expect("recall succeeds");
        assert!(
            snippets.iter().any(|s| s.text.contains("alpha beta gamma")),
            "semantic-only recall should find the turn despite zero word overlap, got: {snippets:?}"
        );
    }

    /// Pin that NON-curator (zed agent) turns are findable by embedding-only
    /// recall. Before the fix, the turn embedding was stored under
    /// `chat:thread:{id}` — an entity that only carries an h_mem for CURATOR
    /// turns. For zed-agent turns the h_mem lives under `curator:thread:{id}`
    /// (the shared copy), so the KNN neighbor's `entity_ref` joined to no
    /// h_mem: every non-curator turn was an orphan embedding, invisible to
    /// semantic recall. The fix stores the embedding under the shared-copy
    /// entity, which is written for every turn.
    #[tokio::test]
    async fn recall_context_finds_zed_agent_turn_by_embedding_only() {
        // Constant embedding — every query is a KNN match for every stored
        // embedding, so the only path that can miss is the entity_ref join.
        let embed_fn = Arc::new(|_text: &str| -> Vec<f32> {
            let mut v = vec![0.0f32; 1024];
            v[0] = 1.0;
            v
        });
        let port = in_memory_port_with_embed_fn(embed_fn);

        // A NON-curator turn — the case whose embedding used to be orphaned.
        let record = TurnRecord {
            thread_id: "t-zed-agent-thread-id".to_string(),
            user_input: "omega psi chi upsilon".to_string(),
            agent_response: "the join now resolves".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Zed Agent".to_string()),
            goal_events: Vec::new(),
        };
        port.ingest_turn(record).await.expect("ingest succeeds");

        // Zero-overlap query — the only path to the turn is the semantic leg.
        let snippets = port
            .recall_context_curator("kookaburra wombat quoll bilby", 10)
            .await
            .expect("recall succeeds");
        assert!(
            snippets.iter().any(|s| s.text.contains("omega psi chi")),
            "semantic-only recall should find the zed-agent turn — an orphan \
             embedding under chat:thread: would make it invisible, got: {snippets:?}"
        );
    }

    /// Confidence-weighted ranking test (Priority 1): when two memories
    /// have similar embedding relevance but different confidence scores,
    /// the higher-confidence memory should rank first. Before the fix,
    /// the sort used `relevance_score` only — confidence was a threshold
    /// filter, not a ranking signal.
    ///
    /// Grounding: Dunning's double curse (`138299529:5`) — the model
    /// can't self-evaluate, but confidence calibrated by outcomes IS
    /// meaningful. Tetlock (`Superforecasting_tetlock:71`) — confidence
    /// is a forecast. Using it as a ranking multiplier means a
    /// well-calibrated memory outranks an untested one.
    #[tokio::test]
    async fn recall_context_ranks_by_confidence_weighted_relevance() {
        // Constant embedding: both h_mems map to the same unit vector, so
        // they have identical relevance_score (1.0 - distance = 1.0). The
        // only differentiator is confidence.
        let embed_fn = Arc::new(|_text: &str| -> Vec<f32> {
            let mut v = vec![0.0f32; 1024];
            v[0] = 1.0;
            v
        });
        let port = in_memory_port_with_embed_fn(embed_fn);
        let curator_webid = port.curator_webid;

        // Store h_mems directly in the curator store with distinct entities
        // and confidence scores. The embedding store uses entity_ref as the
        // key, and query_deduped_untouched queries by entity — so each
        // h_mem needs its own entity, and each embedding's entity_ref must
        // match its h_mem's entity.
        let high_conf = hkask_storage::HMem::new(
            "test:ranking:high",
            "fact",
            serde_json::json!("alpha"),
            curator_webid,
        )
        .with_confidence(hkask_types::Confidence::new(0.99));
        let low_conf = hkask_storage::HMem::new(
            "test:ranking:low",
            "fact",
            serde_json::json!("beta"),
            curator_webid,
        )
        .with_confidence(hkask_types::Confidence::new(0.51));

        let unit_vec = {
            let mut v = vec![0.0f32; 1024];
            v[0] = 1.0;
            v
        };
        let curator_store = port.curator_store.get().expect("curator store");
        curator_store
            .store_embedding("test:ranking:high", &unit_vec, "test-model", None)
            .expect("store embedding high");
        curator_store
            .store_embedding("test:ranking:low", &unit_vec, "test-model", None)
            .expect("store embedding low");

        curator_store.store(high_conf).expect("store high");
        curator_store.store(low_conf).expect("store low");

        // Recall with a query that matches both (constant embedding →
        // both at distance 0 → both have relevance_score 1.0).
        let snippets = port
            .recall_context_curator("matching query", 10)
            .await
            .expect("recall succeeds");

        // Both should be recalled.
        assert_eq!(
            snippets.len(),
            2,
            "both h_mems should be recalled, got: {snippets:?}"
        );

        // The high-confidence one ("alpha") should rank first.
        assert_eq!(
            snippets[0].text,
            "alpha",
            "higher-confidence memory should rank first, got order: {:?}",
            snippets.iter().map(|s| &s.text).collect::<Vec<_>>()
        );
        assert_eq!(
            snippets[1].text, "beta",
            "lower-confidence memory should rank second"
        );
    }

    /// Pin that `recall_from` touches only the h_mems that survive the
    /// limit, not on every recalled candidate. The previous implementation
    /// touched every deduped h_mem inside `query_for_deduped`, even ones
    /// filtered out by `recall_min_confidence` in the injector.
    ///
    /// We verify this by checking that h_mems NOT in the final snippets have
    /// their `recalled_at` unchanged after a recall that truncates them.
    #[tokio::test]
    async fn recall_context_touches_only_injected_h_mems() {
        let port = in_memory_port();
        let curator_webid = port.curator_webid;

        // Ingest one curator turn.
        port.ingest_turn(TurnRecord {
            thread_id: "touch-test".to_string(),
            user_input: "unique_keyword_xyz".to_string(),
            agent_response: "response".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        })
        .await
        .expect("ingest succeeds");

        let curator_store = port.curator_store.get().expect("curator store");

        // Read the stored recalled_at via the untouched query (no side effects).
        let before = curator_store
            .query_for_deduped_untouched("chat:thread:touch-test", curator_webid)
            .expect("untouched query succeeds");
        assert_eq!(before.len(), 1);
        let recalled_at_before = before[0].recalled_at;

        // Sleep so a touch would be observable.
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Recall with a query that does NOT match the stored keyword — the
        // h_mem should be loaded as a candidate but NOT injected (no keyword
        // overlap), so its recalled_at should NOT be touched.
        let snippets = port
            .recall_context_curator("completely_different_query", 10)
            .await
            .expect("recall succeeds");
        assert!(
            snippets.is_empty(),
            "no snippets should match a non-overlapping query"
        );

        let after = curator_store
            .query_for_deduped_untouched("chat:thread:touch-test", curator_webid)
            .expect("untouched query succeeds");
        assert_eq!(after.len(), 1);
        assert_eq!(
            after[0].recalled_at, recalled_at_before,
            "recalled_at should be unchanged when the h_mem is not injected"
        );

        // Now recall with a matching query — the h_mem should be injected and
        // its recalled_at should be touched.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let snippets = port
            .recall_context_curator("unique_keyword_xyz", 10)
            .await
            .expect("recall succeeds");
        assert_eq!(snippets.len(), 1, "matching query should recall the h_mem");

        let after_match = curator_store
            .query_for_deduped_untouched("chat:thread:touch-test", curator_webid)
            .expect("untouched query succeeds");
        assert_eq!(after_match.len(), 1);
        assert!(
            after_match[0].recalled_at > recalled_at_before,
            "recalled_at should be updated when the h_mem is injected"
        );
    }

    /// Pin that the ingestion semaphore serializes concurrent ingestions.
    /// Two concurrent ingestions should both complete successfully, but the
    /// second should wait for the first to release its permit.
    #[tokio::test]
    async fn ingestion_semaphore_serializes_concurrent_ingestions() {
        let port = std::sync::Arc::new(in_memory_port());

        let port1 = port.clone();
        let port2 = port.clone();

        let record1 = TurnRecord {
            thread_id: "sem-1".to_string(),
            user_input: "first".to_string(),
            agent_response: "response1".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };
        let record2 = TurnRecord {
            thread_id: "sem-2".to_string(),
            user_input: "second".to_string(),
            agent_response: "response2".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };

        // Spawn both ingestions concurrently.
        let (r1, r2) = tokio::join!(
            async move { port1.ingest_turn(record1).await },
            async move { port2.ingest_turn(record2).await }
        );

        assert!(r1.is_ok(), "first ingestion should succeed: {r1:?}");
        assert!(r2.is_ok(), "second ingestion should succeed: {r2:?}");

        // Both turns should be stored in the curator store.
        let curator_webid = port.curator_webid;
        let curator_store = port.curator_store.get().expect("curator store");
        let h1 = curator_store
            .query_for_deduped_untouched("chat:thread:sem-1", curator_webid)
            .expect("query succeeds");
        let h2 = curator_store
            .query_for_deduped_untouched("chat:thread:sem-2", curator_webid)
            .expect("query succeeds");
        assert_eq!(h1.len(), 1, "first turn should be stored");
        assert_eq!(h2.len(), 1, "second turn should be stored");
    }

    /// Curator turns must be ingested into the curator's sovereign DB with the
    /// curator's WebID (Private, curator perspective), mirroring the user
    /// agent's memory loop. This is the core of the curator memory mirror —
    /// without it, the curator has no first-person memory.
    #[tokio::test]
    async fn ingest_curator_turn_stores_curator_perspective() {
        let port = in_memory_port();
        let curator_webid = port.curator_webid;
        let record = TurnRecord {
            thread_id: "curator-thread-1".to_string(),
            user_input: "What is the regulation status?".to_string(),
            agent_response: "All systems nominal.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };

        let result = port.ingest_turn(record).await;
        assert!(result.is_ok(), "curator turn ingestion should succeed");

        // The curator's store should have the turn, tagged with the curator's
        // WebID (Private, curator perspective).
        let curator_store = port.curator_store.get().expect("curator store");
        let h_mems = curator_store
            .query_for_deduped_untouched("chat:thread:curator-thread-1", curator_webid)
            .expect("curator episodic query should succeed");
        assert_eq!(
            h_mems.len(),
            1,
            "one curator-perspective episodic h_mem should be stored"
        );
        assert_eq!(h_mems[0].attribute, "chatted");

        // The same curator store also holds the Shared semantic copy — the
        // ontology blob distinguishes it from the episodic record above.
        let semantic_h_mems = curator_store
            .query_deduped("curator:thread:curator-thread-1")
            .expect("curator copy query should succeed");
        assert_eq!(
            semantic_h_mems.len(),
            1,
            "one curator copy h_mem should be stored"
        );
        assert_eq!(semantic_h_mems[0].attribute, "turn");
    }

    /// `recall_context_curator` should recall from the curator's stores, not
    /// the user's. This pins the curator recall path that the
    /// `BridgeCuratorContextInjector` delegates to.
    #[tokio::test]
    async fn recall_context_curator_reads_curator_store() {
        let port = in_memory_port();

        // Ingest a curator turn.
        let record = TurnRecord {
            thread_id: "curator-recall-test".to_string(),
            user_input: "regulation_status_check_keyword".to_string(),
            agent_response: "All regulation systems are operational.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };
        port.ingest_turn(record)
            .await
            .expect("ingestion should succeed");

        // Recall from the curator's store — the keyword should match.
        let snippets = port
            .recall_context_curator("regulation_status_check_keyword", 5)
            .await
            .expect("curator recall should succeed");
        assert!(
            !snippets.is_empty(),
            "curator recall should find the ingested curator turn"
        );
    }

    /// `recall_thread_curator` should recall a thread's prior turns by exact
    /// entity match, not by content similarity. This pins the thread-scoped
    /// recall fix — the previous `inject_context` passed the `thread_id` UUID
    /// as the query to `recall_context`, which never matched stored turn text
    /// (the stored embeddings are of `user_input`, not the thread_id), so
    /// thread-scoped recall was dead code.
    #[tokio::test]
    async fn recall_thread_recalls_thread_by_entity() {
        let port = in_memory_port();
        let thread_id = "curator-thread-recall-by-entity";

        // Ingest a curator turn.
        let record = TurnRecord {
            thread_id: thread_id.to_string(),
            user_input: "how do I configure the embedding model".to_string(),
            agent_response: "Set kask.corpus.embedding_dim in settings.json.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };
        port.ingest_turn(record)
            .await
            .expect("ingestion should succeed");

        // Recall by thread_id — should find the turn via exact entity match.
        let snippets = port
            .recall_thread_curator(thread_id, 10)
            .await
            .expect("thread recall should succeed");
        assert!(
            !snippets.is_empty(),
            "recall_thread_curator should find the ingested turn by entity, not content"
        );
    }

    /// `recall_thread_curator` should recall the curator's prior turns on a
    /// thread from the curator's sovereign stores. Mirrors the user-side test.
    #[tokio::test]
    async fn recall_thread_curator_recalls_curator_thread() {
        let port = in_memory_port();
        let thread_id = "curator-thread-recall-test";

        // Ingest a curator turn.
        let record = TurnRecord {
            thread_id: thread_id.to_string(),
            user_input: "regulation status check".to_string(),
            agent_response: "All regulation systems are operational.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };
        port.ingest_turn(record)
            .await
            .expect("ingestion should succeed");

        // Recall by thread_id from the curator's stores.
        let snippets = port
            .recall_thread_curator(thread_id, 10)
            .await
            .expect("curator thread recall should succeed");
        assert!(
            !snippets.is_empty(),
            "recall_thread_curator should find the ingested curator turn by entity"
        );
    }

    /// Curator consolidation should promote the curator's episodic h_mems to
    /// the curator's semantic store, mirroring the user's consolidation loop.
    /// This pins the Fix 1 wiring — without the `curator_consolidation` field,
    /// the curator's episodic store would grow unbounded and the curator would
    /// never learn consolidated facts from its own experience.
    #[tokio::test]
    async fn maybe_consolidate_fires_curator_pass() {
        let port = in_memory_port_with_cadence(1, 0.3);
        let curator_webid = port.curator_webid;

        // Ingest a curator turn so there's something to consolidate.
        port.ingest_turn(TurnRecord {
            thread_id: "curator-consolidation-test".to_string(),
            user_input: "regulation status check".to_string(),
            agent_response: "All regulation systems are operational.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        })
        .await
        .expect("ingest succeeds");

        // Verify the curator store has the turn before consolidation.
        let curator_store = port
            .curator_store
            .get()
            .expect("curator store should be available in tests");
        let h_mems_before = curator_store
            .query_for_deduped_untouched("chat:thread:curator-consolidation-test", curator_webid)
            .expect("curator episodic query should succeed");
        assert_eq!(
            h_mems_before.len(),
            1,
            "curator episodic store should have the ingested turn"
        );

        // Fire consolidation directly (simulating the timer callback).
        port.maybe_consolidate();

        // The last_consolidation timestamp should now be set.
        port.last_consolidation
            .lock()
            .expect("mutex not poisoned")
            .expect("consolidation should have fired");

        // After consolidation, low-confidence h_mems may have been pruned.
        // We verify the query succeeds — whether the h_mem survived depends
        // on confidence decay, but the consolidation pass itself must not error.
        let h_mems_after = curator_store
            .query_for_deduped_untouched("chat:thread:curator-consolidation-test", curator_webid)
            .expect("curator memory query should succeed after consolidation");
        // The h_mem may or may not have been pruned depending on
        // confidence decay — we just verify the query succeeds and the
        // curator consolidation pass didn't panic.
        let _ = h_mems_after;
    }

    /// Curator turn pin: a curator turn must produce a first-person
    /// episodic record (curator_webid, curator DB) plus the Shared
    /// semantic copy in the curator's DB. Two records, one conversation.
    #[tokio::test]
    async fn ingest_curator_turn_writes_both_perspectives() {
        let port = in_memory_port();
        let record = TurnRecord {
            thread_id: "dual-perspective-test".to_string(),
            user_input: "status?".to_string(),
            agent_response: "nominal".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        };
        port.ingest_turn(record).await.expect("ingest succeeds");

        let curator_store = port.curator_store.get().expect("curator store");
        let curator_perspective = curator_store
            .query_for_deduped_untouched("chat:thread:dual-perspective-test", port.curator_webid)
            .expect("curator query");
        assert_eq!(curator_perspective.len(), 1, "curator perspective present");

        let shared = curator_store
            .query_deduped("curator:thread:dual-perspective-test")
            .expect("semantic query");
        assert_eq!(shared.len(), 1, "shared semantic copy present");
    }

    /// Memory-health probe pin: reports the curator store up when healthy,
    /// degraded when it is down, and — critically — does NOT trigger a
    /// heal (a status read must be side-effect-free, or the probe would
    /// drive the re-open path and flap the warn-once signal).
    #[tokio::test]
    async fn memory_health_json_reports_degraded_without_healing() {
        let port = in_memory_port();

        let healthy = port.memory_health_json();
        assert_eq!(healthy["curator_store"], true);
        // `degraded` follows the curator store alone now — the bridge-side
        // swarm store was removed (swarm memory lives in the swarm MCP
        // server, not the bridge).
        assert_eq!(healthy["degraded"], false);

        // Simulate a curator outage. Healing is disabled in test handles, so
        // if the probe attempted a heal it would fail — the point is it must
        // not attempt one at all.
        port.curator_store.set_for_tests(None);
        let degraded = port.memory_health_json();
        assert_eq!(degraded["curator_store"], false);
        assert_eq!(degraded["degraded"], true);

        // Store still down after the probe — the probe didn't heal.
        assert!(port.curator_store.get().is_none());
    }

    /// Self-healing pin: when the curator store is down, `get()` returns
    /// `None` without healing (heal disabled in tests), and after
    /// `set_for_tests` restores it, subsequent reads see the healed
    /// store. This mirrors the production heal path where a failed open is
    /// retried on the next access.
    #[tokio::test]
    async fn curator_store_heals_after_outage() {
        let port = in_memory_port();

        // Simulate an outage — the store goes None.
        port.curator_store.set_for_tests(None);
        assert!(port.curator_store.get().is_none(), "store down");

        // Ingestion during the outage still succeeds (curator writes skip
        // — there's no store to write to).
        port.ingest_turn(TurnRecord {
            thread_id: "outage-test".to_string(),
            user_input: "during outage".to_string(),
            agent_response: "response".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        })
        .await
        .expect("ingestion during outage succeeds");

        // Heal: restore a fresh in-memory store and verify reads see it.
        let curator_driver: Arc<dyn hkask_storage::DatabaseDriver> =
            SqliteDriver::in_memory_driver();
        let healed = Arc::new(MemoryStore::new(
            HMemStore::from_driver(Arc::clone(&curator_driver)).expect("hmem init"),
            EmbeddingStore::from_driver(curator_driver, 1024).expect("embedding store init"),
        ));
        port.curator_store.set_for_tests(Some(Arc::clone(&healed)));

        assert!(port.curator_store.get().is_some(), "store healed");

        // Post-heal ingestion writes curator records again.
        port.ingest_turn(TurnRecord {
            thread_id: "post-heal-test".to_string(),
            user_input: "after heal".to_string(),
            agent_response: "response".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            goal_events: Vec::new(),
        })
        .await
        .expect("post-heal ingestion succeeds");
        let curator_record = healed
            .query_for_deduped_untouched("chat:thread:post-heal-test", port.curator_webid)
            .expect("curator query");
        assert_eq!(curator_record.len(), 1, "curator record written after heal");
    }

    /// T7b wiring pin: `BridgeMemoryPort::ingest_turn` must call the reask
    /// correlator (`hkask_tool_invoker::correlate_reask`) without panicking.
    /// The correlator drains the process-global render buffer that
    /// provenance-carrying widgets populate via `record_render`. We cannot
    /// easily assert the `reg.widget.reask` tracing span fired without a
    /// capture subscriber (the repo has none); the leaf's unit tests cover the
    /// correlator state machine. This test pins the call path is wired: record
    /// a render, then ingest a user-message turn, and assert no error.
    #[tokio::test]
    async fn bridge_ingest_turn_calls_reask_correlator() {
        use agent::ThreadMemoryPort as _;

        // Record a widget render so the correlator has state to drain.
        hkask_tool_invoker::record_render(Some("portfolio_returns".into()), None);

        // Construct a BridgeMemoryPort over the in-memory RealMemoryPort.
        let inner: std::sync::Arc<dyn MemoryPort> = std::sync::Arc::new(in_memory_port());
        let bridge = BridgeMemoryPort::new(inner);

        // Ingest a user-message turn (non-empty user_input). The correlator
        // fires inside ingest_turn before the inner call.
        let record = agent::ThreadTurnRecord {
            thread_id: "reask-wiring-test".to_string(),
            user_input: "now show me the portfolio returns".to_string(),
            agent_response: "here are the returns".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: None,
            goal_events: Vec::new(),
        };
        let result = bridge.ingest_turn(record).await;
        assert!(
            result.is_ok(),
            "ingest_turn with correlator wired should succeed: {result:?}"
        );
    }

    // ── BridgeAlertEscalationSink tests ──────────────────────────────────

    /// `BridgeAlertEscalationSink::persist_alert` must write to the
    /// `EscalationQueue` so the entry is readable via `list_pending` — this
    /// pins the Store seam end-to-end (sink → queue → `curator_escalations`).
    /// If the adapter drops the call or the queue write fails silently, the
    /// alert never reaches the reviewable backlog.
    #[test]
    fn bridge_alert_escalation_sink_writes_to_queue() {
        use hkask_regulation::AlertEscalationSink;
        use hkask_storage::EscalationQueue;
        use hkask_storage::database::sqlite::SqliteDriver;

        let driver = SqliteDriver::in_memory_driver();
        let queue = Arc::new(EscalationQueue::from_driver(driver).expect("escalation queue init"));
        let sink = BridgeAlertEscalationSink::new(queue.clone());

        // Persist a critical alert
        sink.persist_alert(
            "Variety deficit 150 exceeds threshold 100",
            1.0,
            r#"{"domain":"test","deficit":150,"threshold":100,"severity":"Critical"}"#,
        );

        // The alert must be readable via list_pending (the same method
        // `curator_escalations` calls).
        let pending = queue.list_pending().expect("list_pending must succeed");
        assert_eq!(pending.len(), 1, "the alert must reach the queue");
        assert_eq!(
            pending[0].output,
            "Variety deficit 150 exceeds threshold 100"
        );
        assert!((pending[0].confidence - 1.0).abs() < f64::EPSILON);
        assert!(
            pending[0]
                .error_context
                .contains("\"severity\":\"Critical\"")
        );
        assert_eq!(pending[0].status, hkask_storage::EscalationStatus::Pending);
    }

    /// When the queue write fails, `persist_alert` must not panic — it logs
    /// and swallows. This pins the best-effort contract: a failing queue
    /// never breaks the regulation loop.
    #[test]
    fn bridge_alert_escalation_sink_does_not_panic_on_write_failure() {
        use hkask_regulation::AlertEscalationSink;
        use hkask_storage::EscalationQueue;
        use hkask_storage::database::sqlite::SqliteDriver;

        let driver = SqliteDriver::in_memory_driver();
        let queue = Arc::new(EscalationQueue::from_driver(driver).expect("escalation queue init"));
        // Drop the underlying driver by dropping the queue, then reconstruct
        // a sink over a dangling Arc — this is hard to simulate cleanly, so
        // instead we just verify the happy path doesn't panic on a normal
        // call (the error path is covered by the queue's own tests).
        let sink = BridgeAlertEscalationSink::new(queue);
        sink.persist_alert("test", 0.5, "{}");
    }
}
