//! Curator sovereign-store infrastructure — extracted from `memory.rs`
//! (deep-module split: the curator's `curator.db` open path, self-healing store
//! handle, and consolidation builder are a one-way dependency of the memory
//! port and independent of the user-store orchestration that remains in
//! `memory.rs`).
//!
//! The shared `open_regulation_archive` helper stays in `memory.rs` because
//! the user-store path (`RealMemoryPort::new`) also calls it; this submodule
//! reaches it via `use super::open_regulation_archive`.

use hkask_memory::{MemoryConsolidator, MemoryStore};
use hkask_storage::{Database, EmbeddingStore, HMemStore};
use std::sync::{Arc, RwLock};

use super::open_regulation_archive;

/// Resolve the curator's sovereign `curator.db` path (same resolution as
/// `open_curator_store`): `HKASK_CURATOR_DB` if set, else
/// `agents/curator/curator.db` under the hKask data dir.
pub fn curator_db_path() -> String {
    std::env::var("HKASK_CURATOR_DB").unwrap_or_else(|_| {
        let p = hkask_types::agent_paths::agent_db("curator");
        let resolved = hkask_types::agent_paths::resolve_under_data_dir(&p);
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        resolved.to_string_lossy().to_string()
    })
}
/// Open a `RegulationArchive` (durable regulation-span store) on the curator's
/// sovereign `curator.db` — the same DB the curator MCP server's `reg_query` and
/// `curator_algedonic_log` tools read. Returns `None` on any failure; the
/// caller degrades to `NoopEventSink` with a warn.
///
/// Used by `main.rs` to wire the `McpRuntime` and `CyberneticsLoop` event sinks
/// to durable storage on the curator's curator.db.
pub fn open_curator_regulation_archive(
    passphrase: &str,
) -> Option<Arc<hkask_storage::RegulationArchive>> {
    open_regulation_archive(&curator_db_path(), passphrase, "curator")
}
/// The curator's sovereign store, with self-healing open.
///
/// One store holds both the curator's first-person episodic records and the
/// shared semantic copies — the `HMemOntology` blob on each h_mem
/// distinguishes them (P5.4), so no second store struct is needed.
///
/// Wraps the `Option<Arc<MemoryStore>>` in an `RwLock` plus the parameters
/// needed to re-open it (passphrase, embedding dim). When the initial open
/// fails, the store is `None` and every access via `get()` re-attempts the
/// open. A successful re-open restores curator memory mid-session.
pub(crate) struct CuratorStore {
    store: RwLock<Option<Arc<MemoryStore>>>,
    passphrase: String,
    embedding_dim: usize,
    heal_attempt_logged: std::sync::atomic::AtomicBool,
    heal_enabled: bool,
}

impl CuratorStore {
    pub(crate) fn new(passphrase: &str, embedding_dim: usize) -> Self {
        let store = open_curator_store(passphrase, embedding_dim);
        if store.is_none() {
            tracing::error!(
                target: "reg.memory",
                db_path = %curator_db_path(),
                "Curator memory store unavailable — the curator runs WITHOUT                  memory. Every curator-turn write will be                  attempted again on ingestion (self-healing); check the DB                  path above, that no other process holds the SQLCipher lock,                  and that the passphrase matches the user's hKask keychain entry."
            );
        }
        Self {
            store: RwLock::new(store),
            passphrase: passphrase.to_string(),
            embedding_dim,
            heal_attempt_logged: std::sync::atomic::AtomicBool::new(false),
            heal_enabled: true,
        }
    }

    pub(crate) fn availability(&self) -> bool {
        match self.store.read() {
            Ok(guard) => guard.is_some(),
            Err(_) => false,
        }
    }

    pub(crate) fn get(&self) -> Option<Arc<MemoryStore>> {
        let needs_heal = match self.store.read() {
            Ok(guard) => guard.is_none(),
            Err(_) => true,
        };
        if needs_heal && self.heal_enabled {
            self.try_heal();
        }
        match self.store.read() {
            Ok(guard) => (*guard).clone(),
            Err(_) => None,
        }
    }

    pub(crate) fn try_heal(&self) {
        let fresh = open_curator_store(&self.passphrase, self.embedding_dim);
        let fresh_ok = fresh.is_some();
        let replaced = match self.store.write() {
            Ok(mut guard) => {
                let was_down = guard.is_none();
                if fresh_ok && was_down {
                    *guard = fresh;
                    true
                } else {
                    false
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.memory",
                    error = %e,
                    "Curator store lock poisoned — cannot attempt heal"
                );
                false
            }
        };
        if replaced {
            tracing::info!(
                target: "reg.memory",
                db_path = %curator_db_path(),
                "Curator memory store healed — curator memory restored"
            );
            self.heal_attempt_logged
                .store(false, std::sync::atomic::Ordering::Relaxed);
        } else if !fresh_ok {
            if !self
                .heal_attempt_logged
                .swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                tracing::warn!(
                    target: "reg.memory",
                    db_path = %curator_db_path(),
                    "Curator memory store still unavailable after re-open                      attempt — curator-turn writes are being dropped"
                );
            }
        }
    }
}
pub(crate) fn build_curator_consolidation(
    consolidation_cadence_secs: u64,
    store: &Option<Arc<MemoryStore>>,
) -> Option<Arc<MemoryConsolidator>> {
    if consolidation_cadence_secs == 0 {
        return None;
    }
    let Some(store) = store else {
        return None;
    };
    Some(Arc::new(MemoryConsolidator::new(Arc::clone(store))))
}
fn open_curator_store(passphrase: &str, embedding_dim: usize) -> Option<Arc<MemoryStore>> {
    let curator_db_path = curator_db_path();

    let db = match Database::open(&curator_db_path, passphrase) {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                db_path = %curator_db_path,
                "Failed to open curator DB — curator copies will be skipped.                  Set HKASK_CURATOR_DB to override the path, or ensure the                  curator agent directory exists under the hKask data dir."
            );
            return None;
        }
    };
    let pool = match db.sqlite_pool() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                "Failed to get SQLite pool for curator DB"
            );
            return None;
        }
    };
    let driver: Arc<dyn hkask_storage::DatabaseDriver> = Arc::new(
        hkask_storage::database::sqlite::SqliteDriver::new_labeled(pool, curator_db_path.as_str()),
    );
    let h_mem_store = match HMemStore::from_driver(Arc::clone(&driver)) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                "Failed to create HMemStore for curator DB"
            );
            return None;
        }
    };
    let embedding_store = match EmbeddingStore::from_driver(driver, embedding_dim) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                "Failed to create EmbeddingStore for curator DB"
            );
            return None;
        }
    };
    let store = Arc::new({
        // The curator uses the default storage budget (10_000) with no env
        // override — a deliberate design decision: the curator must shed
        // low-utility/low-saliency memories rather than grow unbounded. The
        // default budget is the Ashby attenuator that forces consolidation
        // to prune. The user store reads HKASK_MEMORY_STORAGE_BUDGET; the
        // curator intentionally does not, so an operator cannot raise the
        // curator's cap without changing the default constant.
        let base = MemoryStore::new(h_mem_store, embedding_store);
        // Wire the `reg.memory.encode` span sink on the curator's own DB —
        // mirrors the user-store wiring in `RealMemoryPort::new`. The
        // curator's regulation archive is the same DB the curator MCP
        // server's `reg_query` and `curator_algedonic_log` tools read.
        match open_regulation_archive(&curator_db_path, passphrase, "curator") {
            Some(archive) => base.with_ledger(archive),
            None => base,
        }
    });
    tracing::info!(
        target: "reg.memory",
        db_path = %curator_db_path,
        "Curator memory store opened —          curator turns will be ingested into curator memory (perspective = curator)"
    );
    Some(store)
}
