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

use hkask_memory::{ExternalPassageBatch, MemoryConsolidator, MemoryStore};
use hkask_storage::open_or_repair;
#[cfg(test)]
use hkask_storage::{EmbeddingStore, HMemStore};
use hkask_types::{MemoryError, MemoryPort, MemorySnippet, TurnRecord, WebID};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

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
pub(crate) use curator_stores::curator_private_db_path;
pub use curator_stores::{
    CuratorRegulationArchive, hydrate_operator_feedback, open_curator_regulation_archive,
    persist_operator_feedback, persist_skill_outcome, persist_skill_tool_failure,
};
pub(crate) use curator_stores::{CuratorStore, build_curator_consolidation};

// ── Ingest write path — extracted to `memory/ingest.rs` ────────────────────
// Deep-module split (bridge-audit BD-04 continuation): the turn write path
// (h_mem writes + embedding) is independent of the port
// orchestration and the recall path that remain here. `write_turn` borrows the
// port's fields via `WriteContext`; `ingest_turn` keeps only the semaphore permit.
mod ingest;
pub(crate) use ingest::WriteContext;

// ── Real memory port (full hKask memory stack) ─────────────────────────────

/// zed-kask: D100 — the turn-storage entity prefix, the ONE bridge-side
/// spelling (the ingest path's chunk entities, the recall budget's prefix
/// query, and the private forgetting leg's prune scope all key on it; the
/// curator MCP child's `SHARED_TURN_PREFIX` is its crate-private sibling).
pub(crate) const TURN_ENTITY_PREFIX: &str = "curator:thread:";

/// Real `MemoryPort` implementation backed by hKask's unified `MemoryStore`.
///
/// Stores each completed turn as cleaned, word-bounded chunks under the
/// thread entity `curator:thread:{thread_id}` in the curator's sovereign
/// `curator.db` — one shared copy per turn (the former curator-perspective
/// duplicate was removed by the operator's 2026-09-04 single-copy ruling),
/// each chunk embedded (with its `passage_text`) and ontologically tagged
/// (structural dimensions deterministically, content dimensions via the
/// classifier model). The curator's `curator_memory_recall` /
/// `curator_semantic_search` see every turn the agent observed.
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
    // shared store as chunked h_mems — one copy per turn, no perspective
    // duplicate. The curator can recall what happened across all agents.
    // Context injection is wired for the curator (via
    // `recall_context_curator`); the zed agent's `MemoryPort` trait impls
    // are no-ops — recall is curator-only until the zed agent gets its own
    // recall path.
    pub(crate) curator_store: Arc<CuratorStore>,
    /// zed-kask: D100/FR-4 (operator ruling 2026-11-05) — the curator's
    /// PRIVATE memory store (`agents/curator/curator-private.db`) behind the
    /// same self-healing handle. Turns from privacy-marked threads are
    /// written HERE (write routing by the record's `InferenceProvenance`);
    /// a private-marked context's recall federates BOTH stores (the private
    /// rows recall only there); a public context never touches it. A
    /// separate SQLCipher file, so the public DB is clean of UNMARKED
    /// private content by construction: private-origin rows written by
    /// the curator MCP server (the private-context skill-use report,
    /// CU-19 a, 2026-11-14) carry the private mark, embed through the
    /// private leg's LOCAL model, and never recall into a public
    /// context. Stated
    /// limits: the private store keeps NO child-side surface (no
    /// distillation, no child forgetting pass, no MCP valve — the
    /// bridge-side legs below are its only forgetting surfaces; the
    /// inference-carrying passes stay unmirrored until a local model leg
    /// exists, the forward local-routing rule), and its chunks get no
    /// CLASSIFIER tags (the configured classifier is a cloud model;
    /// structural tags only, surfaced). The embedding half of the former
    /// degrade LIFTED (D100 item 3, 2026-11-05): private chunks and
    /// private-context queries embed through the LOCAL ollama port
    /// (`private_embedding_port` below) — never the configured cloud
    /// embedder. The forgetting mirror (operator ruling 2026-11-06, the
    /// review's item 4): the inference-free deletion surfaces now run over
    /// the private store too — the consolidation twin
    /// (`private_curator_consolidation` below) and the age-gated turn-storage
    /// forgetting leg (both bridge-side, both timer-driven on the one
    /// consolidation cadence).
    pub(crate) private_curator_store: Arc<CuratorStore>,
    /// `None` when embedding credentials are unavailable — h_mem writes still
    /// work (they're pure SQL), but semantic recall (KNN) is degraded to
    /// keyword-only. This must NOT block the memory pipeline: the curator's
    /// episodic memory of conversations is more valuable than vector search.
    embedding_port: Option<LanguageModelEmbeddingPort>,
    embedding_model: String,
    /// zed-kask: D100 item 3 (operator 2026-11-05) — the private leg's
    /// LOCAL embedding port, bound to the local ollama descriptor (no
    /// credentials; `build_private_embedding_port`). Private-store chunks
    /// at ingest and private-context recall queries embed through THIS
    /// instance only — the configured cloud port above never sees private
    /// content. `None` (or an empty `private_embedding_model`) keeps the
    /// keyword-only degrade, surfaced.
    private_embedding_port: Option<LanguageModelEmbeddingPort>,
    /// The private leg's local model string
    /// (`kask.private_inference.embedding_model`, default
    /// `DEFAULT_PRIVATE_EMBEDDING_MODEL` — provider-qualified for the
    /// local ollama provider).
    private_embedding_model: String,
    pub(crate) external_passphrase: String,
    external_sources: Arc<hkask_memory::FederatedSourceCache>,
    /// The classifier model used for write-time chunk tagging
    /// (`kask.models.classifier_model`, env `HKASK_CLASSIFIER_MODEL`).
    /// `None` = not configured — chunks get structural tags only. The
    /// inference port itself is read lazily per turn from the app-wide
    /// global (the memory port wires before the inference stack does).
    classifier_model: Option<String>,
    /// zed-kask: D100 classifier lift (operator ruling 2026-11-06) — the
    /// private leg's LOCAL classifier model
    /// (`kask.private_inference.classifier_model`, default
    /// `DEFAULT_PRIVATE_CLASSIFIER_MODEL` = `ollama/qwen3.5:4b`; validated
    /// at wiring via `resolve_private_classifier_model`). `None` = the
    /// explicit opt-out or a refused cloud-prefixed value — private turns
    /// keep structural tags only, surfaced; the configured cloud classifier
    /// never sees private content.
    private_classifier_model: Option<String>,
    /// Host-deployed template registry root for write-time chunk tagging
    /// (`kask.corpus.template_root` override → `{data_dir}/skills/registry`).
    /// Threaded from settings at wiring time — `HKASK_TEMPLATE_ROOT` is only
    /// injected into MCP server child processes, never set in the editor
    /// process this port runs in.
    template_root: std::path::PathBuf,
    curator_webid: WebID,
    /// Consolidation service for the curator's store. `None` when
    /// consolidation is disabled (`consolidation_cadence_secs == 0`).
    /// Rebuilt when the curator stores heal after an open failure.
    //
    // Behind an `Arc` so the production consolidation timer can hold a clone
    // and re-read the current value on each tick — picks up the rebuild that
    // `write_turn` performs after a curator-store heal.
    curator_consolidation: Arc<RwLock<Option<Arc<MemoryConsolidator>>>>,
    /// zed-kask: D100 (operator ruling 2026-11-06, the review's item 4) —
    /// the private store's consolidation TWIN: the same inference-free
    /// confidence-floor sweep over `private_curator_store`, built by the
    /// same `build_curator_consolidation` home (one builder, two
    /// instances), fired on the same timer tick, rebuilt after the private
    /// store's heal. The pass is deletion-only (`MemoryConsolidator::
    /// consolidate`), so the mirror adds no cloud-exposure surface; any
    /// future inference-carrying consolidation over private content must
    /// route its model leg through the local leg (the item-3 pattern) or
    /// not ship — the forward local-routing rule, recorded in D100.
    private_curator_consolidation: Arc<RwLock<Option<Arc<MemoryConsolidator>>>>,
    /// Consolidation cadence in seconds. `0` disables the trigger — for
    /// BOTH stores' consolidation passes and the private forgetting leg
    /// (they ride the one timer; "no automated passes" is one switch).
    consolidation_cadence_secs: u64,
    /// Confidence floor for cleanup during consolidation.
    confidence_floor: f64,
    /// zed-kask: D100 (operator ruling 2026-11-06, the review's item 4) —
    /// the private forgetting leg's age knob
    /// (`kask.memory.forgetting_days`, the same setting the curator MCP
    /// child reads for its public-store forgetting). The leg prunes the
    /// private store's AGED TURN STORAGE (`curator:thread:` rows — the
    /// `curator_memory_prune` valve's fail-closed default scope) past the
    /// age, sparing rows recalled within the same window (the decay-clock
    /// discipline: a recently-recalled row stays). `0` disables the leg
    /// (mirroring the child's "0 disables the pass"). The public store gets
    /// NO bridge-side prune — its forgetting is the child's
    /// distillation-gated pass plus the operator's MCP valve; two pruners
    /// of the same rows would interfere.
    forgetting_days: u64,
    /// Tokio runtime handle — entered around embedding HTTP calls so that
    /// `reqwest` (which is tokio-backed) has a reactor.
    tokio_handle: tokio::runtime::Handle,
    /// Ingestion concurrency limiter.
    ingest_semaphore: tokio::sync::Semaphore,
}

impl RealMemoryPort {
    pub fn goal_projection_store(&self) -> Result<Arc<MemoryStore>, String> {
        self.curator_store
            .get()
            .ok_or_else(|| "curator projection store unavailable".to_string())
    }

    /// Construct a new `RealMemoryPort`.
    ///
    /// Opens the curator's SQLCipher database and initializes the embedding
    /// router. All turns (curator and zed agent) are ingested into the
    /// curator's DB as tagged chunks — one shared copy per turn.
    ///
    /// Returns `Err` if the curator database cannot be opened.
    pub fn new(
        passphrase: &str,
        embedding_model: String,
        embedding_dim: usize,
        embedding_port: Option<LanguageModelEmbeddingPort>,
        private_embedding_port: Option<LanguageModelEmbeddingPort>,
        private_embedding_model: String,
        classifier_model: Option<String>,
        private_classifier_model: Option<String>,
        template_root: std::path::PathBuf,
        consolidation_cadence_secs: u64,
        confidence_floor: f64,
        forgetting_days: u64,
        memory_life_days: f64,
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

        // Curator store behind the self-healing handle. The decay constant
        // (`kask.memory.memory_life_days`) threads through so the store the
        // regulation sensor reports IS the store that actually decays.
        let curator_store = Arc::new(CuratorStore::new(
            curator_db_path(),
            passphrase,
            embedding_dim,
            memory_life_days,
        ));
        // zed-kask: D100/FR-4 — the private sibling (same handle, same
        // passphrase chain, its own SQLCipher file).
        let private_curator_store = Arc::new(CuratorStore::new(
            curator_private_db_path(),
            passphrase,
            embedding_dim,
            memory_life_days,
        ));

        let curator_consolidation = Arc::new(RwLock::new(build_curator_consolidation(
            consolidation_cadence_secs,
            &curator_store.get(),
        )));
        // zed-kask: D100 (operator ruling 2026-11-06, the review's item 4) —
        // the private twin: the same builder home over the private store.
        // Inference-free (deletion-only), so the mirror carries no
        // cloud-exposure surface.
        let private_curator_consolidation = Arc::new(RwLock::new(build_curator_consolidation(
            consolidation_cadence_secs,
            &private_curator_store.get(),
        )));

        Ok(Self {
            curator_store,
            private_curator_store,
            embedding_port,
            embedding_model,
            private_embedding_port,
            private_embedding_model,
            external_passphrase: passphrase.to_string(),
            external_sources: Arc::new(hkask_memory::FederatedSourceCache::new()),
            classifier_model,
            private_classifier_model,
            template_root,
            curator_webid,
            curator_consolidation,
            private_curator_consolidation,
            consolidation_cadence_secs,
            confidence_floor,
            forgetting_days,
            tokio_handle,
            ingest_semaphore: tokio::sync::Semaphore::new(resolve_ingest_concurrency()),
        })
    }

    /// Start a background timer that fires a consolidation pass on the
    /// configured cadence.
    ///
    /// `tokio::time::interval` provides the pass-scheduling invariants by
    /// construction: passes follow the interval without hand-rolled
    /// last-fired bookkeeping. The timer consumes the interval's immediate
    /// first tick as the "wait one full cadence before the first pass"
    /// grace; the poll interval is clamped to at least 60 seconds so short
    /// cadences do not turn into tight polling.
    ///
    /// Each tick re-reads the consolidator handle so a pass picks up the
    /// rebuild that `write_turn` performs after a curator-store heal.
    ///
    /// A cadence of 0 disables consolidation entirely (no timer started).
    pub fn start_consolidation_timer(&self) -> Option<tokio::task::JoinHandle<()>> {
        if self.consolidation_cadence_secs == 0 {
            return None;
        }
        let curator_consolidation = Arc::clone(&self.curator_consolidation);
        let private_curator_consolidation = Arc::clone(&self.private_curator_consolidation);
        let private_curator_store = Arc::clone(&self.private_curator_store);
        let curator_webid = self.curator_webid;
        let confidence_floor = self.confidence_floor;
        let forgetting_days = self.forgetting_days;
        let cadence_secs = self.consolidation_cadence_secs;
        let poll_interval = Duration::from_secs(cadence_secs.max(60));

        let handle = self.tokio_handle.spawn(async move {
            let mut interval = tokio::time::interval(poll_interval);
            interval.tick().await; // the immediate first tick: wait one cadence
            loop {
                interval.tick().await; // one full poll_interval later
                tracing::info!(
                    target: "reg.memory",
                    cadence_secs,
                    confidence_floor,
                    "Consolidation timer fired"
                );
                let consolidator = curator_consolidation
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone());
                if let Some(consolidator) = consolidator {
                    fire_curator_consolidation_pass(
                        &consolidator,
                        curator_webid,
                        confidence_floor,
                        "consolidation_timer",
                    );
                }
                // zed-kask: D100 (operator ruling 2026-11-06, the review's
                // item 4) — the private twin fires on the same tick, then
                // the private forgetting leg. Both inference-free.
                let private_consolidator = private_curator_consolidation
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone());
                if let Some(consolidator) = private_consolidator {
                    fire_curator_consolidation_pass(
                        &consolidator,
                        curator_webid,
                        confidence_floor,
                        "consolidation_timer_private",
                    );
                }
                fire_private_forgetting_pass(&private_curator_store, forgetting_days);
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

/// Open a `RegulationArchive` on an arbitrary SQLCipher DB. Used by
/// `open_curator_regulation_archive` (the deferred wiring in `main.rs`) for
/// the durable regulation event sinks; the store-side ledger archive no
/// longer opens its own driver — `open_curator_store` shares its driver via
/// `RegulationArchive::from_driver` (the hang-audit open dedup, 2026-11-11).
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
    let opened_at = std::time::Instant::now();
    hkask_memory::ensure_db_parent_dir(db_path);
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
        Ok(archive) => {
            // Hang-audit Pr5: the archive self-reports its open cost under
            // reg.storage.open.<role> — best-effort, never fails the open.
            let duration_ms = opened_at.elapsed().as_millis() as u64;
            curator_stores::journal_storage_open(&archive, role, duration_ms);
            Some(Arc::new(archive))
        }
        Err(e) => {
            tracing::warn!(target: "reg.storage", error = %e, role, "Failed to init RegulationArchive schema");
            None
        }
    }
}

/// Fire one curator consolidation pass.
///
/// Called by the consolidation timer on each cadence tick (and directly by
/// tests simulating a tick). `log_label` names the calling context in tracing.
fn fire_curator_consolidation_pass(
    curator_consolidation: &hkask_memory::MemoryConsolidator,
    curator_webid: WebID,
    confidence_floor: f64,
    log_label: &str,
) {
    match curator_consolidation.consolidate(&curator_webid, confidence_floor) {
        Ok(outcome) => {
            tracing::info!(
                target: "reg.memory",
                label = log_label,
                deleted = outcome.deleted_count,
                failed = outcome.failed_count,
                skipped = outcome.skipped,
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

/// zed-kask: D100 (operator ruling 2026-11-06, the review's item 4) — fire
/// one private-store forgetting pass: an age-gated sweep over the private
/// store's TURN STORAGE (`curator:thread:` rows — the `curator_memory_prune`
/// valve's fail-closed default scope), through the valve's own store-level
/// call. `forgetting_days == 0` disables the leg (mirroring the child's "0
/// disables the pass"); rows recalled within the same window are spared (the
/// decay-clock discipline — a recently-recalled row's clock was reset, so
/// age alone must not delete it). Bridge-side, not child-side: the private
/// store has NO child surface (the public DB's clean-by-construction posture
/// stands), so this leg and the consolidation twin are the private store's
/// only forgetting surfaces — and the public store gets NO bridge-side
/// prune, since its forgetting is the child's distillation-gated pass plus
/// the operator's MCP valve (two pruners of the same rows would interfere).
/// Called by the consolidation timer on each cadence tick (and directly by
/// tests simulating a tick).
fn fire_private_forgetting_pass(private_store: &CuratorStore, forgetting_days: u64) {
    if forgetting_days == 0 {
        return;
    }
    let Some(store) = private_store.get() else {
        tracing::warn!(
            target: "reg.memory",
            "private forgetting pass skipped — the private store is unavailable"
        );
        return;
    };
    let max_age = forgetting_days as i64;
    match store.prune_by_age_in_prefixes(&[TURN_ENTITY_PREFIX], max_age, Some(max_age)) {
        Ok(outcome) => {
            tracing::info!(
                target: "reg.memory",
                candidates = outcome.candidates,
                deleted = outcome.deleted_count,
                spared = outcome.spared_count,
                failed = outcome.failed_count,
                forgetting_days,
                "Private forgetting pass complete"
            );
        }
        Err(e) => {
            tracing::warn!(target: "reg.memory", error = %e, "Private forgetting pass failed");
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
                private_curator_store: &self.private_curator_store,
                embedding_port: self.embedding_port.as_ref(),
                embedding_model: &self.embedding_model,
                private_embedding_port: self.private_embedding_port.as_ref(),
                private_embedding_model: &self.private_embedding_model,
                classifier_model: self.classifier_model.as_deref(),
                private_classifier_model: self.private_classifier_model.as_deref(),
                template_root: self.template_root.as_path(),
                curator_webid: self.curator_webid,
                tokio_handle: &self.tokio_handle,
                curator_consolidation: &self.curator_consolidation,
                private_curator_consolidation: &self.private_curator_consolidation,
                consolidation_cadence_secs: self.consolidation_cadence_secs,
            };
            ingest::write_turn(&ctx, record).await?;
            Ok(())
        })
    }

    fn store_private_memory<'a>(
        &'a self,
        thread_id: &'a str,
        content: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                return Err(MemoryError::Ingestion(
                    "cannot store an empty private memory note".to_string(),
                ));
            }
            let _ingest_permit = self
                .ingest_semaphore
                .acquire()
                .await
                .map_err(|e| MemoryError::Ingestion(format!("ingest_semaphore closed: {e}")))?;

            let Some(private_store) = self.private_curator_store.get() else {
                let reason = self
                    .private_curator_store
                    .outage_summary()
                    .unwrap_or_else(|| "curator-private.db is not open".to_string());
                return Err(MemoryError::Ingestion(format!(
                    "private curator database (curator-private.db) unavailable: {reason}"
                )));
            };

            let turn_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let entity = ingest::PRIVATE_MEMORY_ENTITY.to_string();
            let attribute = format!("preference:{thread_id}:{turn_ms}");
            let h_mem = hkask_storage::HMem::new(
                &entity,
                &attribute,
                serde_json::Value::String(trimmed.to_string()),
                self.curator_webid,
            )
            .with_visibility(hkask_types::Visibility::Shared)
            .with_inference_provenance(hkask_types::InferenceProvenance::PrivateInference)
            .with_confidence(hkask_types::Confidence::new(0.85));

            let store_for_write = Arc::clone(&private_store);
            let text_for_embed = trimmed.to_string();
            self.store_work(move || {
                store_for_write.store(h_mem).map_err(|e| {
                    MemoryError::Ingestion(format!("curator-private.db write failed: {e}"))
                })
            })
            .await?;

            // Embed locally when the private embedding leg is wired; a down
            // local embedder degrades semantic KNN without undoing the durable
            // SQLCipher write, because standing private memories are also
            // recalled directly by entity.
            if let (Some(port), false) = (
                self.private_embedding_port.as_ref(),
                self.private_embedding_model.is_empty(),
            ) {
                let port = port.clone();
                let model = self.private_embedding_model.clone();
                let dim = self.private_curator_store.embedding_dim() as u32;
                let embed_input = vec![text_for_embed.clone()];
                if let Ok(Ok(batch)) = self
                    .tokio_handle
                    .spawn(async move {
                        port.embed_with_dimensions(&model, &embed_input, Some(dim))
                            .await
                    })
                    .await
                    && let Some(vector) = batch.vectors.into_iter().next()
                {
                    let store_for_embed = Arc::clone(&private_store);
                    let model_label = self.private_embedding_model.clone();
                    let _ = self
                        .store_work(move || {
                            store_for_embed
                                .store_embedding(
                                    &entity,
                                    &vector,
                                    &model_label,
                                    Some(&text_for_embed),
                                )
                                .map(|_| ())
                                .map_err(|e| MemoryError::Ingestion(e.to_string()))
                        })
                        .await;
                }
            }

            tracing::info!(
                target: "reg.memory",
                thread_id = %thread_id,
                "Stored explicit private memory in curator-private.db"
            );
            Ok(())
        })
    }
}

/// Per-pass memoized query embeddings keyed by `(private_context, dimensions)`.
///
/// Within one pre-turn injection pass (`BridgeContextInjector::inject_context`),
/// Curator recall (public store + optional private store) and federated external
/// passage search otherwise embed the exact same prompt multiple times when
/// their vector widths match (default `1024`). Sharing this cache across a
/// single pass cuts `/embeddings` HTTP calls to one per distinct width and
/// avoids repeating a failed provider request in the same turn.
#[derive(Default)]
pub(crate) struct QueryEmbedCache {
    entries: std::collections::HashMap<(bool, u32), QueryEmbedOutcome>,
}

#[derive(Clone)]
enum QueryEmbedOutcome {
    Embedded(hkask_types::EmbeddingBatch),
    Unwired(String),
    Failed(String),
}

impl RealMemoryPort {
    /// Search only user-selected registered sealed sources. An empty selection is
    /// opt-out; otherwise an invalid ID, unavailable source, incompatible query
    /// identity, or in-flight source change fails closed. Hits retain source and
    /// run provenance and are never written to Curator memory or linked to it.
    ///
    /// D100: when `private_context` is `true`, the query embeds through the
    /// local `private_embedding_port` ONLY — private prompts never cross to the
    /// cloud `embedding_port`.
    pub async fn search_external_passages(
        &self,
        manifest_path: &Path,
        query: &str,
        selected_source_ids: &[String],
        limit: usize,
        private_context: bool,
    ) -> Result<Vec<ExternalPassageBatch>, String> {
        let mut cache = QueryEmbedCache::default();
        self.search_external_passages_cached(
            manifest_path,
            query,
            selected_source_ids,
            limit,
            private_context,
            &mut cache,
        )
        .await
    }

    #[cfg(test)]
    async fn search_external_passages_at(
        &self,
        manifest_path: &Path,
        query: &str,
        selected_source_ids: &[String],
        limit: usize,
    ) -> Result<Vec<ExternalPassageBatch>, String> {
        self.search_external_passages(manifest_path, query, selected_source_ids, limit, false)
            .await
    }

    pub(crate) async fn search_external_passages_cached(
        &self,
        manifest_path: &Path,
        query: &str,
        selected_source_ids: &[String],
        limit: usize,
        private_context: bool,
        embed_cache: &mut QueryEmbedCache,
    ) -> Result<Vec<ExternalPassageBatch>, String> {
        if selected_source_ids.is_empty() {
            return Ok(Vec::new());
        }
        if limit == 0 {
            return Err("External passage search limit must be positive".to_string());
        }
        let mut ids = selected_source_ids.to_vec();
        ids.sort();
        if ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err("External source selection contains duplicate IDs".to_string());
        }
        // Admit sources before embedding: an unregistered or unsealed source
        // fails closed without sending the query to the embedding provider.
        let cache = Arc::clone(&self.external_sources);
        let manifest_path = manifest_path.to_path_buf();
        let passphrase = self.external_passphrase.clone();
        let registry = self
            .tokio_handle
            .spawn_blocking(move || cache.admit_selected(&manifest_path, ids, &passphrase))
            .await
            .map_err(|error| format!("External source admission task failed: {error}"))?
            .map_err(|error| error.to_string())?;
        let (_port, model) = self.embedding_leg_for_context(private_context)?;
        // Embed the query once per distinct source width (reusing `embed_cache`
        // when Curator recall already embedded at that width in this pass).
        let mut distinct_dims: Vec<u32> = registry
            .sources()
            .iter()
            .map(|source| source.identity().dimensions as u32)
            .collect();
        distinct_dims.sort_unstable();
        distinct_dims.dedup();
        let mut vectors_by_dims: std::collections::HashMap<u32, Vec<f32>> =
            std::collections::HashMap::with_capacity(distinct_dims.len());
        for dims in distinct_dims {
            let (outcome, _) = self
                .embed_query_cached(query, private_context, dims, embed_cache)
                .await;
            match outcome {
                QueryEmbedOutcome::Embedded(batch) => {
                    let vector = batch
                        .vectors
                        .into_iter()
                        .next()
                        .ok_or("External query embedding returned no vector")?;
                    vectors_by_dims.insert(dims, vector);
                }
                QueryEmbedOutcome::Unwired(reason) => return Err(reason),
                QueryEmbedOutcome::Failed(error) => {
                    return Err(format!(
                        "External query embedding failed at {dims} dimensions: {error}"
                    ));
                }
            }
        }
        let query_text = query.to_string();
        self.tokio_handle
            .spawn_blocking(move || {
                let results = registry
                    .sources()
                    .iter()
                    .map(|source| {
                        let dims = source.identity().dimensions as u32;
                        let vector = vectors_by_dims.get(&dims).ok_or_else(|| {
                            format!(
                                "External source '{}': no query embedding at its {dims} dimensions",
                                source.identity().source_id
                            )
                        })?;
                        source
                            .search(&model, &query_text, vector, limit)
                            .map_err(|error| {
                                format!(
                                    "External source '{}': {error}",
                                    source.identity().source_id
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if !registry
                    .verify_unchanged()
                    .map_err(|error| error.to_string())?
                {
                    return Err(
                        "External source identity changed during retrieval; retry search"
                            .to_string(),
                    );
                }
                Ok(results)
            })
            .await
            .map_err(|error| format!("External source search task failed: {error}"))?
    }

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

    /// The configured memory life in days — read from the store that actually
    /// decays, so the regulation sensor can never report fiction: when the
    /// store is unavailable the default constant is an estimate only. A healed
    /// store retains its configured decay constant.
    pub fn memory_life_days(&self) -> f64 {
        self.curator_store
            .get()
            .map(|s| s.memory_life_days())
            .unwrap_or_else(hkask_memory::MemoryStore::default_memory_life_days)
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
        private_context: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemorySnippet>, MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            let mut embed_cache = QueryEmbedCache::default();
            self.recall_context_curator_cached(query, limit, private_context, &mut embed_cache)
                .await
        })
    }

    pub(crate) async fn recall_context_curator_cached(
        &self,
        query: &str,
        limit: usize,
        private_context: bool,
        embed_cache: &mut QueryEmbedCache,
    ) -> Result<Vec<MemorySnippet>, MemoryError> {
        let mut snippets: Vec<MemorySnippet> = Vec::new();
        // zed-kask: D100/FR-4 (operator ruling 2026-11-05) — store
        // federation: the public store always recalls; a private-marked
        // context (the PI's stated use) additionally federates the
        // PRIVATE store. A public context never touches it — the
        // private rows are structurally unreachable, not filtered.
        if let Some(ref curator_store) = self.curator_store.get() {
            snippets.extend(
                self.recall_from(
                    curator_store,
                    query,
                    limit,
                    "recall_context_curator",
                    private_context,
                    self.curator_store.embedding_dim() as u32,
                    embed_cache,
                )
                .await?,
            );
        }
        if private_context && let Some(ref private_curator_store) = self.private_curator_store.get()
        {
            snippets.extend(
                self.recall_from(
                    private_curator_store,
                    query,
                    limit,
                    "recall_context_curator_private",
                    private_context,
                    self.private_curator_store.embedding_dim() as u32,
                    embed_cache,
                )
                .await?,
            );
        }
        Ok(snippets)
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
        private_context: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemorySnippet>, MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            let mut snippets: Vec<MemorySnippet> = Vec::new();
            // zed-kask: D100/FR-4 — the same federation on the entity-keyed
            // path: a thread marked mid-life has chunks in BOTH stores; a
            // private context recalls both, a public context only the
            // public copy.
            if let Some(ref curator_store) = self.curator_store.get() {
                snippets.extend(
                    self.recall_thread_from(
                        curator_store,
                        thread_id,
                        limit,
                        "recall_thread_curator",
                        private_context,
                    )
                    .await?,
                );
            }
            if private_context
                && let Some(ref private_curator_store) = self.private_curator_store.get()
            {
                snippets.extend(
                    self.recall_thread_from(
                        private_curator_store,
                        thread_id,
                        limit,
                        "recall_thread_curator_private",
                        private_context,
                    )
                    .await?,
                );
            }
            Ok(snippets)
        })
    }

    /// Recall standing explicit private preferences/notes (`curator:private:memory`)
    /// from `curator-private.db`. Never touches the public `curator.db`.
    pub(crate) async fn recall_standing_private_memories(
        &self,
        limit: usize,
    ) -> Result<Vec<MemorySnippet>, MemoryError> {
        let Some(private_store) = self.private_curator_store.get() else {
            return Ok(Vec::new());
        };
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.store_work(move || {
            let h_mems = private_store
                .query_deduped_untouched(ingest::PRIVATE_MEMORY_ENTITY)
                .map_err(|e| MemoryError::Recall(format!("curator-private.db read failed: {e}")))?;
            let mut items: Vec<(MemorySnippet, hkask_storage::HMemId, i64)> = h_mems
                .into_iter()
                .filter_map(|h_mem| {
                    let text = hkask_memory::semantic_passage_for_h_mem(&h_mem)?;
                    let created = h_mem.observed_at.timestamp_millis();
                    Some((
                        MemorySnippet {
                            text,
                            entity: h_mem.entity,
                            confidence: h_mem.confidence.value(),
                            relevance_score: 1.0,
                        },
                        h_mem.id,
                        created,
                    ))
                })
                .collect();
            items.sort_by_key(|a| std::cmp::Reverse(a.2));
            items.truncate(limit);
            for (_, id, _) in &items {
                let _ = private_store.touch_recall(id);
            }
            Ok(items.into_iter().map(|(snippet, _, _)| snippet).collect())
        })
        .await
    }

    /// Recall prompt-salient and thread-scoped chunks exclusively from
    /// `curator-private.db` (used for non-Curator privacy-marked threads so
    /// locked Z-K Agent threads read their private database without pulling
    /// unrelated public Curator turns).
    pub(crate) async fn recall_private_store_only_cached(
        &self,
        query: &str,
        thread_id: &str,
        prompt_limit: usize,
        thread_limit: usize,
        embed_cache: &mut QueryEmbedCache,
    ) -> Result<(Vec<MemorySnippet>, Vec<MemorySnippet>), MemoryError> {
        let Some(ref private_curator_store) = self.private_curator_store.get() else {
            return Ok((Vec::new(), Vec::new()));
        };
        let prompt_snippets = self
            .recall_from(
                private_curator_store,
                query,
                prompt_limit,
                "recall_context_private_thread",
                true,
                self.private_curator_store.embedding_dim() as u32,
                embed_cache,
            )
            .await?;
        let thread_snippets = self
            .recall_thread_from(
                private_curator_store,
                thread_id,
                thread_limit,
                "recall_thread_private_only",
                true,
            )
            .await?;
        Ok((prompt_snippets, thread_snippets))
    }

    /// Record co-occurrence links between entities recalled in the same
    /// context. Routes to `private_curator_store` when `private_context` is
    /// true so private thread entities never cross into `curator.db`.
    pub async fn record_co_occurrence(&self, entities: Vec<String>, private_context: bool) {
        let store = if private_context {
            self.private_curator_store.get()
        } else {
            self.curator_store.get()
        };
        let Some(store) = store else {
            return;
        };
        let recorded = self
            .store_work(move || {
                store
                    .record_co_occurrence(&entities)
                    .map_err(|error| MemoryError::Recall(error.to_string()))
            })
            .await;
        if let Err(e) = recorded {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                "Failed to record co-occurrence links"
            );
        }
    }

    /// Run synchronous store work on the runtime's blocking pool.
    ///
    /// Recall is awaited from the editor's foreground turn task, so a
    /// statement issued on the calling thread freezes the window for as
    /// long as it takes — including the seconds-scale key derivation a
    /// fresh pooled connection pays.
    async fn store_work<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, MemoryError> + Send + 'static,
    ) -> Result<T, MemoryError> {
        self.tokio_handle
            .spawn_blocking(work)
            .await
            .map_err(|error| MemoryError::Recall(format!("memory store task failed: {error}")))?
    }

    /// Get the connectedness score for an entity — the total co-occurrence
    /// count across all links. Higher = more connected = more salient.
    /// Returns 0 when the curator store is not configured (documented
    /// default) or when the lookup fails — and a lookup failure is WARNED
    /// with the entity and error, never silently swallowed: a store
    /// outage must not read as "no connections" in recall ranking.
    pub fn connectedness(&self, entity: &str) -> u64 {
        self.curator_store
            .get()
            .as_ref()
            .and_then(|store| match store.connectedness(entity) {
                Ok(count) => Some(count),
                Err(error) => {
                    tracing::warn!(
                        entity = %entity,
                        error = %error,
                        "connectedness lookup failed — ranking as 0"
                    );
                    None
                }
            })
            .unwrap_or(0)
    }

    /// Embed one recall query through `port` (None = no port wired) at the
    /// store's width — the shared request/response ladder for the public
    /// (configured cloud) and private (local ollama) recall legs. Every
    /// failure is surfaced as a warned `None` (the KNN leg degrades to
    /// keyword-only); a `None` port is the already-surfaced wiring state
    /// and returns quietly.
    /// Resolve `(embedding_port, embedding_model)` for the active provenance
    /// leg (`private_context`). Shared by `recall_from` and
    /// `search_external_passages_cached` so D100 routing lives in one place:
    /// private contexts NEVER widen to `self.embedding_port`.
    fn embedding_leg_for_context(
        &self,
        private_context: bool,
    ) -> Result<(LanguageModelEmbeddingPort, String), String> {
        if private_context {
            match (
                self.private_embedding_port.clone(),
                self.private_embedding_model.is_empty(),
            ) {
                (Some(port), false) => Ok((port, self.private_embedding_model.clone())),
                _ => Err("private context: the local embedding port is not wired \
                     (set kask.private_inference.embedding_model to lift the degrade; \
                     private content never widens to the configured cloud embedder)"
                    .to_string()),
            }
        } else {
            self.embedding_port
                .clone()
                .map(|port| (port, self.embedding_model.clone()))
                .ok_or_else(|| {
                    "External passage search requires a configured embedding port".to_string()
                })
        }
    }

    /// Embed `query` at `dimensions` under the active provenance leg, memoizing
    /// the outcome in `embed_cache` so matching-width recall legs share one HTTP
    /// call (and one failure outcome) per turn. Returns `(outcome, computed_now)`.
    async fn embed_query_cached(
        &self,
        query: &str,
        private_context: bool,
        dimensions: u32,
        embed_cache: &mut QueryEmbedCache,
    ) -> (QueryEmbedOutcome, bool) {
        let key = (private_context, dimensions);
        if let Some(existing) = embed_cache.entries.get(&key) {
            return (existing.clone(), false);
        }
        let outcome = match self.embedding_leg_for_context(private_context) {
            Err(reason) => QueryEmbedOutcome::Unwired(reason),
            Ok((port, model)) => {
                let query_owned =
                    hkask_inference::openai_compat::format_retrieval_query_for_model(&model, query);
                match self
                    .tokio_handle
                    .spawn(async move {
                        port.embed_with_dimensions(&model, &[query_owned], Some(dimensions))
                            .await
                    })
                    .await
                {
                    Ok(Ok(batch)) => QueryEmbedOutcome::Embedded(batch),
                    Ok(Err(e)) => QueryEmbedOutcome::Failed(e.to_string()),
                    Err(e) => QueryEmbedOutcome::Failed(format!("embedding task panicked: {e}")),
                }
            }
        };
        embed_cache.entries.insert(key, outcome.clone());
        (outcome, true)
    }

    /// Shared recall implementation for the curator's store.
    ///
    /// `log_label` is used in tracing so recall paths are distinguishable in
    /// logs.
    ///
    /// This was previously duplicated verbatim between `recall_context` and
    /// `recall_context_curator`; the duplication was a maintenance hazard
    /// (a fix to one had to be manually mirrored in the other).
    async fn recall_from<'a>(
        &'a self,
        store: &'a Arc<MemoryStore>,
        query: &'a str,
        limit: usize,
        log_label: &'static str,
        private_context: bool,
        dimensions: u32,
        embed_cache: &mut QueryEmbedCache,
    ) -> Result<Vec<MemorySnippet>, MemoryError> {
        let (outcome, computed_now) = self
            .embed_query_cached(query, private_context, dimensions, embed_cache)
            .await;
        let embedded = match outcome {
            QueryEmbedOutcome::Embedded(batch) => {
                Some((batch.vectors, batch.requested_model, batch.actual_model))
            }
            QueryEmbedOutcome::Unwired(_) => {
                if private_context && computed_now {
                    tracing::warn!(
                        target: "reg.memory",
                        label = log_label,
                        "private context: the local embedding port is not wired — recall degraded to keyword-only (set kask.private_inference.embedding_model to lift the degrade; private content never widens to the configured cloud embedder)"
                    );
                }
                None
            }
            QueryEmbedOutcome::Failed(error) => {
                if computed_now {
                    tracing::warn!(
                        target: "reg.memory",
                        %error,
                        label = log_label,
                        "Failed to embed recall query — embedding search skipped for this turn"
                    );
                }
                None
            }
        };
        let store = Arc::clone(store);
        let query = query.to_string();
        self.store_work(move || {
            Self::recall_from_blocking(&store, embedded, &query, limit, log_label, private_context)
        })
        .await
    }

    /// The store half of `recall_from`: the KNN and keyword legs, ranking,
    /// and the decay-clock touch. Synchronous — run it through `store_work`.
    fn recall_from_blocking(
        store: &MemoryStore,
        embedded: Option<(Vec<Vec<f32>>, String, Option<String>)>,
        query: &str,
        limit: usize,
        log_label: &'static str,
        private_context: bool,
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
        // FR-4 (D-2): rows excluded by the privacy filter this call —
        // surfaced after the legs, never silently dropped (an empty result
        // must not read as "no memory found").
        let mut excluded_private: usize = 0;

        if let Some((vectors, requested_model, actual_model)) = embedded
            && let Some(query_vector) = vectors.into_iter().next()
        {
            match store.search_similar(
                &query_vector,
                limit,
                &requested_model,
                actual_model.as_deref(),
            ) {
                Ok(outcome) => {
                    // Model-gate degradation must be visible: rows excluded
                    // because they were embedded under a different model are
                    // a migration in progress, not an empty memory — EXCEPT
                    // on a private-context leg, where the exclusion is by
                    // construction (D100 item 3): the query embeds in the
                    // LOCAL model space, so this store's cloud-model vectors
                    // cannot join, and its rows stay keyword-reachable.
                    // Debug there, not warn — it fires on every
                    // private-context recall against a cloud-model store.
                    if outcome.excluded_model_mismatch > 0 && private_context {
                        tracing::debug!(
                            target: "reg.memory",
                            label = log_label,
                            excluded = outcome.excluded_model_mismatch,
                            "KNN excluded this store's cloud-model vectors — the private context's query embeds locally (D100 item 3); keyword recall still serves this leg"
                        );
                    } else if outcome.excluded_model_mismatch > 0 {
                        tracing::warn!(
                            target: "reg.memory",
                            label = log_label,
                            excluded = outcome.excluded_model_mismatch,
                            query_model = %requested_model,
                            "KNN excluded stored embeddings from a different model — re-embed pending"
                        );
                    }
                    for result in outcome.results {
                        // Retrieve the h_mem associated with this embedding
                        // to get the full text content. Use the untouched
                        // variant — we touch only the injected ones below.
                        let entity_ref = &result.embedding.entity_ref;
                        // Chunk writes store the chunk's text as the vector's
                        // passage_text — the KNN result pinpoints the matched
                        // chunk and only that chunk is injected. No
                        // passage_text, no injection: a vector that cannot
                        // name its passage would inject the whole entity —
                        // the 500KB-blob behavior this pipeline replaces.
                        let Some(matched_passage) = result
                            .embedding
                            .passage_text
                            .as_deref()
                            .filter(|passage| !passage.is_empty())
                        else {
                            continue;
                        };
                        if let Ok(h_mems) = store.query_deduped_untouched(entity_ref) {
                            for h_mem in h_mems {
                                // FR-4 (D-2): the recall filter — private-
                                // marked rows never enter a non-private
                                // recall context. `private_context` is the
                                // D100 mark of the thread whose prompt this
                                // recall feeds (passed down from the injector
                                // call site); a private-marked context
                                // recalls its own rows.
                                if !private_context
                                    && h_mem.inference_provenance
                                        == hkask_types::InferenceProvenance::PrivateInference
                                {
                                    excluded_private += 1;
                                    continue;
                                }
                                let Some(text) = hkask_memory::semantic_passage_for_h_mem(&h_mem)
                                else {
                                    continue;
                                };
                                if text != matched_passage {
                                    continue;
                                }
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
        // Load the thread h_mems ONCE, filter by keyword overlap in memory,
        // and touch only the injected h_mems (touching every scanned row
        // turns recall into a write storm).
        let query_words: Vec<String> = query
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .take(5)
            .map(|w| w.to_lowercase())
            .collect();

        if !query_words.is_empty() {
            // One prefix query loads all curator:thread:* chunk h_mems.
            //
            // The recall budget caps the number of rows loaded — without
            // it, a session with thousands of past turns would load all of
            // them into memory on every recall call. We load 10x the
            // recall limit (most recent first) to give the keyword filter a
            // reasonable pool to filter from without unbounded loading.
            let entity_prefix = TURN_ENTITY_PREFIX.to_string();
            let recall_budget = limit.saturating_mul(10).max(50);
            match store.query_deduped_untouched_by_prefix(&entity_prefix, recall_budget) {
                Ok(h_mems) => {
                    for h_mem in h_mems {
                        // FR-4 (D-2): the keyword leg carries the same
                        // filter — a private-marked row never enters a
                        // non-private context by either leg.
                        if !private_context
                            && h_mem.inference_provenance
                                == hkask_types::InferenceProvenance::PrivateInference
                        {
                            excluded_private += 1;
                            continue;
                        }
                        let Some(text) = hkask_memory::semantic_passage_for_h_mem(&h_mem) else {
                            continue;
                        };
                        let overlap =
                            hkask_memory::salience::keyword_overlap_score(&query_words, &text);
                        if overlap == 0 {
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
                                // Keep the keyword leg's ceiling below a strong KNN
                                // match, but distinguish full-query from incidental hits.
                                relevance_score: 0.5 * overlap as f64 / query_words.len() as f64,
                            },
                            h_mem_id: h_mem.id,
                        });
                    }
                }
                Err(error) => {
                    // A failed store read must not read as "no keyword hits" —
                    // the semantic leg's partial results stand, and the failure
                    // is surfaced for the operator (the unwrap_or(0) trap).
                    tracing::warn!(
                        target: "hkask.bridge.memory",
                        %error,
                        "keyword recall leg failed — returning semantic-leg results only"
                    );
                }
            }
        }

        // FR-4 (D-2): the privacy filter's exclusions are surfaced, never
        // silently dropped — an empty recall in a non-private context must
        // be distinguishable from "no memory found".
        if excluded_private > 0 {
            tracing::warn!(
                target: "reg.memory",
                label = log_label,
                excluded = excluded_private,
                private_context,
                "Private-inference-marked rows excluded from this non-private recall context (FR-4)"
            );
        }

        // ── 3. Sort by combined relevance × confidence × connectedness and truncate ─
        //
        // Precompute connectedness once per unique candidate entity. The
        // previous implementation called `connectedness` — a live SQLCipher
        // query — inside the sort comparator, running O(N log N) queries per
        // recall on every prompt. One query per unique entity is the floor:
        // the value is a per-entity property, not per-comparison.
        let mut connectedness_by_entity: std::collections::HashMap<String, u64> =
            std::collections::HashMap::with_capacity(candidates.len());
        for candidate in &candidates {
            if !connectedness_by_entity.contains_key(&candidate.snippet.entity) {
                // zed-kask: D100/FR-4 — the connectedness comes from the
                // STORE being recalled (each store carries its own
                // co-occurrence links): private-store candidates rank on
                // private-store connectedness, never cross-store.
                let score = match store.connectedness(&candidate.snippet.entity) {
                    Ok(score) => score,
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.memory",
                            error = %e,
                            entity = %candidate.snippet.entity,
                            label = log_label,
                            "connectedness lookup failed during recall — ranking as 0"
                        );
                        0
                    }
                };
                connectedness_by_entity.insert(candidate.snippet.entity.clone(), score);
            }
        }
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
            let a_conn = connectedness_by_entity
                .get(&a.snippet.entity)
                .copied()
                .unwrap_or(0) as f64;
            let b_conn = connectedness_by_entity
                .get(&b.snippet.entity)
                .copied()
                .unwrap_or(0) as f64;
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

    /// Shared thread-scoped recall implementation. Mirrors `recall_from` but
    /// uses exact-entity queries instead of content-similarity / keyword
    /// overlap.
    ///
    /// The thread entity is `curator:thread:{thread_id}`, the one copy every
    /// turn's chunks are written under.
    async fn recall_thread_from<'a>(
        &'a self,
        store: &'a Arc<MemoryStore>,
        thread_id: &'a str,
        limit: usize,
        log_label: &'static str,
        private_context: bool,
    ) -> Result<Vec<MemorySnippet>, MemoryError> {
        let store = Arc::clone(store);
        let thread_id = thread_id.to_string();
        self.store_work(move || {
            Self::recall_thread_from_blocking(&store, &thread_id, limit, log_label, private_context)
        })
        .await
    }

    /// The store half of `recall_thread_from`. Synchronous — run it through
    /// `store_work`.
    fn recall_thread_from_blocking(
        store: &MemoryStore,
        thread_id: &str,
        limit: usize,
        log_label: &'static str,
        private_context: bool,
    ) -> Result<Vec<MemorySnippet>, MemoryError> {
        let mut candidates: Vec<(MemorySnippet, hkask_storage::HMemId)> = Vec::new();
        let mut excluded_private: usize = 0;

        // Exact entity match — every chunk of the thread. A failed read is an
        // error, not "the thread has no memories" (the unwrap_or(0) trap):
        // this query is the function's only source, so propagate.
        let thread_entity = format!("{TURN_ENTITY_PREFIX}{thread_id}");
        let h_mems = store
            .query_deduped_untouched(&thread_entity)
            .map_err(|error| {
                MemoryError::Recall(format!(
                    "thread recall read failed for '{thread_entity}': {error}"
                ))
            })?;
        for h_mem in h_mems {
            // FR-4 (D-2): a private-marked thread's own turns recall only
            // into a private-marked context; a non-private context never
            // loads them.
            if !private_context
                && h_mem.inference_provenance == hkask_types::InferenceProvenance::PrivateInference
            {
                excluded_private += 1;
                continue;
            }
            let Some(text) = hkask_memory::semantic_passage_for_h_mem(&h_mem) else {
                continue;
            };
            candidates.push((
                MemorySnippet {
                    text,
                    entity: h_mem.entity.clone(),
                    confidence: h_mem.confidence.value(),
                    relevance_score: 1.0,
                },
                h_mem.id,
            ));
        }

        // Truncate to limit. The query returns most-recent-first, so the
        // candidates are already in recency order. All candidates have
        // relevance_score 1.0 (exact entity match), so no sort is needed.
        candidates.truncate(limit);

        // FR-4 (D-2): surfaced, never silently dropped.
        if excluded_private > 0 {
            tracing::warn!(
                target: "reg.memory",
                label = log_label,
                excluded = excluded_private,
                "Private-inference-marked rows excluded from this non-private thread recall (FR-4)"
            );
        }

        // Touch only the injected h_mems — resets the decay clock on
        // memories that actually got used.
        for (_, h_mem_id) in &candidates {
            if let Err(e) = store.touch_recall(h_mem_id) {
                tracing::warn!(
                    target: "reg.memory.decay",
                    triple_id = %h_mem_id.as_uuid(),
                    error = %e,
                    label = log_label,
                    "Failed to touch_recall h_mem during thread recall"
                );
            }
        }

        let touched = candidates.len();
        let snippets: Vec<MemorySnippet> =
            candidates.into_iter().map(|(snippet, _)| snippet).collect();

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
/// Retained as an explicit boundary because `ingest_turn` translates
/// `agent::ThreadTurnRecord` (`AgentId`, `private_inference: bool`) to
/// `hkask_types::TurnRecord` and invokes `hkask_tool_invoker::correlate_reask`
/// (which `agent` does not depend on).
pub struct BridgeMemoryPort {
    inner: std::sync::Arc<dyn MemoryPort>,
}

#[async_trait::async_trait]
impl hkask_regulation::MemoryHealthSource for RealMemoryPort {
    async fn sheddable_h_mem_count(&self) -> Option<usize> {
        let store = self.curator_store.get()?;
        match store.sheddable_h_mem_count() {
            Ok(count) => Some(count),
            Err(e) => {
                tracing::warn!(
                    target: "reg.sensor.memory",
                    error = %e,
                    "sheddable_h_mem_count: store query failed — returning None (not 0)"
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

    async fn memory_life_days(&self) -> f64 {
        RealMemoryPort::memory_life_days(self)
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
                    // FR-4: the producing thread's routing state is the only
                    // source of the mark — the D100 flag the zed side read at
                    // turn completion. Never caller-supplied.
                    inference_provenance: hkask_types::InferenceProvenance::from_private_flag(
                        record.private_inference,
                    ),
                })
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn store_private_memory<'a>(
        &'a self,
        thread_id: &'a str,
        content: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            inner
                .store_private_memory(thread_id, content)
                .await
                .map_err(|e| e.to_string())
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
