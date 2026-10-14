//! Curator database and store lifecycle (`CuratorStores`, `CuratorDb`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use hkask_mcp_server::server::{McpToolError, resolve_db_passphrase};
use hkask_storage::database::sqlite::SqliteDriver;

use crate::federated;

/// Minimum interval between self-heal re-open attempts, so a DB outage does
/// not trigger a full DB open + store construction on every tool call.
const HEAL_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

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
    pub(crate) fn all_none(&self) -> bool {
        self.regulation_store.is_none() && self.memory.is_none()
    }

    /// Empty store set — used when the DB cannot be opened at all.
    pub fn empty() -> Self {
        Self {
            regulation_store: None,
            memory: None,
        }
    }

    pub(crate) fn regulation_store(
        &self,
    ) -> Result<&Arc<hkask_storage::RegulationArchive>, McpToolError> {
        self.regulation_store
            .as_ref()
            .ok_or_else(|| McpToolError::permission_denied("RegulationArchive not available"))
    }

    pub(crate) fn memory(&self) -> Result<&Arc<hkask_memory::MemoryStore>, McpToolError> {
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
    pub(crate) passphrase: Option<String>,
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
    pub(crate) fn from_context(ctx: &hkask_mcp_server::server::ServerContext) -> Self {
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

    pub(crate) fn federated_sources(&self) -> Arc<federated::FederatedSourceRegistry> {
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
    pub(crate) fn get(&self) -> CuratorStores {
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

/// Resolve the memory decay constant from `HKASK_MEMORY_LIFE_DAYS`. Absent →
/// the store default (180). Malformed values warn naming the value and fall
/// back to the default — never a silent fallback.
pub(crate) fn memory_life_days_from_env() -> f64 {
    match std::env::var("HKASK_MEMORY_LIFE_DAYS") {
        Ok(raw) => parse_memory_life_days_value(&raw),
        Err(_) => hkask_memory::MemoryStore::default_memory_life_days(),
    }
}

/// Pure parse for the decay constant — separated so the malformed path is
/// testable without env mutation (env writes are unsafe in edition 2024).
pub(crate) fn parse_memory_life_days_value(raw: &str) -> f64 {
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

/// Open the curator's sovereign `curator.db` and construct all stores from
/// a single shared driver. Called at construction and on every heal attempt.
pub(crate) fn open_curator_stores(
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
