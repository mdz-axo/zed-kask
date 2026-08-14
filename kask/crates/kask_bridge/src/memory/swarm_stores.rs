//! Swarm memory store — self-healing handle to `swarm_memory.db`.
//!
//! Mirrors `CuratorStore` but for the swarm's sovereign memory DB. The swarm
//! store is opened directly in the bridge process (same pattern as the
//! curator store) so that `recall_context_swarm` can read swarm memory
//! without an IPC round-trip to the swarm MCP server. SQLite handles
//! concurrent readers across processes, so the bridge and the swarm MCP
//! server can both have the DB open.
//!
//! The store is opened lazily and self-heals: when the initial open fails
//! (DB locked, passphrase mismatch, path missing), the store is `None` and
//! every access re-attempts the open. A successful re-open restores swarm
//! memory mid-session.

use hkask_memory::MemoryStore;
use hkask_storage::{Database, EmbeddingStore, HMemStore};
use std::sync::{Arc, RwLock};

/// Resolve the swarm memory DB path: `HKASK_SWARM_MEMORY_DB` if set (absolute
/// override), else `swarm_memory.db` under the hKask data dir. Mirrors the
/// resolution in `hkask-mcp-swarm/src/config.rs`.
pub fn swarm_db_path() -> String {
    std::env::var("HKASK_SWARM_MEMORY_DB")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            let p = std::path::Path::new("swarm_memory.db");
            hkask_types::agent_paths::resolve_under_data_dir(p)
                .to_string_lossy()
                .to_string()
        })
}

/// The swarm's memory store, with self-healing open.
///
/// Mirrors `CuratorStore`: wraps `Option<Arc<MemoryStore>>` in an `RwLock`
/// plus the parameters needed to re-open it. When the initial open fails,
/// the store is `None` and every access via `get()` re-attempts the open.
pub(crate) struct SwarmStore {
    store: RwLock<Option<Arc<MemoryStore>>>,
    passphrase: String,
    embedding_dim: usize,
    heal_attempt_logged: std::sync::atomic::AtomicBool,
    heal_enabled: bool,
}

impl SwarmStore {
    pub(crate) fn new(passphrase: &str, embedding_dim: usize) -> Self {
        let store = open_swarm_store(passphrase, embedding_dim);
        if store.is_none() {
            tracing::warn!(
                target: "reg.memory",
                db_path = %swarm_db_path(),
                "Swarm memory store unavailable — swarm recall will return empty. \
                 The store will self-heal on the next access; check the DB path, \
                 that no other process holds the SQLCipher lock, and that the \
                 passphrase matches HKASK_SWARM_MEMORY_PASSPHRASE."
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

    #[cfg(test)]
    pub(crate) fn for_tests(store: Option<Arc<MemoryStore>>) -> Self {
        Self {
            store: RwLock::new(store),
            passphrase: String::new(),
            embedding_dim: 1024,
            heal_attempt_logged: std::sync::atomic::AtomicBool::new(false),
            heal_enabled: false,
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

    fn try_heal(&self) {
        let fresh = open_swarm_store(&self.passphrase, self.embedding_dim);
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
                    "Swarm store lock poisoned — cannot attempt heal"
                );
                false
            }
        };
        if replaced {
            tracing::info!(
                target: "reg.memory",
                db_path = %swarm_db_path(),
                "Swarm memory store healed — swarm recall restored"
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
                    db_path = %swarm_db_path(),
                    "Swarm memory store still unavailable after re-open attempt"
                );
            }
        }
    }
}

fn open_swarm_store(passphrase: &str, embedding_dim: usize) -> Option<Arc<MemoryStore>> {
    let db_path = swarm_db_path();
    let db = match Database::open(&db_path, passphrase) {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                db_path = %db_path,
                "Failed to open swarm memory DB — swarm recall will be unavailable"
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
                "Failed to get SQLite pool for swarm DB"
            );
            return None;
        }
    };
    let driver: Arc<dyn hkask_storage::DatabaseDriver> = Arc::new(
        hkask_storage::database::sqlite::SqliteDriver::new_labeled(pool, db_path.as_str()),
    );
    let h_mem_store = match HMemStore::from_driver(Arc::clone(&driver)) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                "Failed to create HMemStore for swarm DB"
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
                "Failed to create EmbeddingStore for swarm DB"
            );
            return None;
        }
    };
    let store = Arc::new(MemoryStore::new(h_mem_store, embedding_store));
    tracing::info!(
        target: "reg.memory",
        db_path = %db_path,
        "Swarm memory store opened — swarm recall available"
    );
    Some(store)
}
