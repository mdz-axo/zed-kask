//! Sovereign Curator SQLCipher store opening, path resolution, and self-healing handle.
//!
//! Centralizes the invariants shared by the host process (`kask_bridge`) and the
//! stdio MCP server (`hkask-mcp-curator`):
//! - One canonical path resolution for `curator.db` and `curator-private.db`
//!   (filtering blank env overrides and ensuring parent directories exist).
//! - Single-driver store initialization (`open_curator_store_set`) so
//!   `RegulationArchive`, `HMemStore`, and `EmbeddingStore` share one SQLCipher
//!   pool and key derivation (Hang audit Pr1).
//! - One throttled self-healing state machine (`SelfHealingStore`) with
//!   warn-once outage logging, side-effect-free availability checks, and
//!   instance-accurate `db_path` attribution.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{EmbeddingStore, HMemStore, RegulationArchive, open_or_repair};
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};

use crate::MemoryStore;

/// Default minimum interval between self-heal re-open attempts during an outage,
/// preventing every call from triggering SQLCipher key derivation and schema checks.
pub const DEFAULT_HEAL_RETRY_INTERVAL: Duration = Duration::from_secs(5);

/// Resolve an operator-overridable Curator DB path purely (side-effect-free):
/// non-blank `env_var` if set (absolute as-is, relative under the hKask data dir),
/// else `default_relative` under the hKask data dir.
fn resolve_curator_path_with_override(env_var: &str, default_relative: &Path) -> String {
    let resolved = match std::env::var(env_var)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
    {
        Some(path) if path.is_absolute() => path,
        Some(relative) => hkask_types::agent_paths::resolve_under_data_dir(&relative),
        None => hkask_types::agent_paths::resolve_under_data_dir(default_relative),
    };
    resolved.to_string_lossy().into_owned()
}

/// Ensure the parent directory of `db_path` exists before opening a SQLCipher file (D28).
pub fn ensure_db_parent_dir(db_path: &str) {
    let path = Path::new(db_path);
    if let Some(Err(error)) = path.parent().map(std::fs::create_dir_all) {
        tracing::warn!(
            target: "reg.storage",
            path = %path.display(),
            %error,
            "Failed to create Curator database parent directory"
        );
    }
}

/// Resolve the Curator's sovereign public `curator.db` path (`HKASK_CURATOR_DB`
/// when set non-blank, else `agents/curator/curator.db` under the hKask data dir).
#[must_use]
pub fn resolve_curator_db_path() -> String {
    resolve_curator_path_with_override(
        "HKASK_CURATOR_DB",
        &hkask_types::agent_paths::agent_db("curator"),
    )
}

/// Resolve the Curator's private `curator-private.db` path (`HKASK_CURATOR_PRIVATE_DB`
/// when set non-blank, else `agents/curator/curator-private.db` under the hKask data dir).
#[must_use]
pub fn resolve_curator_private_db_path() -> String {
    let relative = hkask_types::agent_paths::agent_dir("curator").join("curator-private.db");
    resolve_curator_path_with_override("HKASK_CURATOR_PRIVATE_DB", &relative)
}

/// Parse a raw `HKASK_MEMORY_LIFE_DAYS` value into a positive finite day count,
/// warning and falling back to [`MemoryStore::default_memory_life_days`] on invalid input.
#[must_use]
pub fn parse_memory_life_days_value(raw: &str) -> f64 {
    match raw.trim().parse::<f64>() {
        Ok(value) if value.is_finite() && value > 0.0 => value,
        _ => {
            tracing::warn!(
                target: "reg.memory",
                env = "HKASK_MEMORY_LIFE_DAYS",
                value = %raw,
                "Malformed memory life setting — must be a positive number of days; using default"
            );
            MemoryStore::default_memory_life_days()
        }
    }
}

/// Journal one `reg.storage.open.<role>` span — best-effort wall-clock telemetry
/// for database/archive opens (Hang audit Pr5).
pub fn journal_storage_open(archive: &RegulationArchive, role: &str, duration_ms: u64) {
    let Some(namespace) = SpanNamespace::new("reg.storage") else {
        tracing::warn!(
            target: "reg.storage",
            "reg.storage namespace is not registered — open timing not journaled"
        );
        return;
    };
    let event = RegulationRecord::new(
        WebID::from_persona(b"curator"),
        Span::new(namespace, &format!("open.{role}")),
        CyclePhase::Sense,
        serde_json::json!({ "duration_ms": duration_ms }),
        0,
    );
    if let Err(error) = archive.persist(&event) {
        tracing::warn!(
            target: "reg.storage",
            %error,
            role,
            "Failed to journal reg.storage.open record"
        );
    }
}

/// Paired sovereign stores opened over a single shared SQLCipher driver.
#[derive(Clone, Default)]
pub struct CuratorStoreSet {
    pub regulation_store: Option<Arc<RegulationArchive>>,
    pub memory: Option<Arc<MemoryStore>>,
}

impl CuratorStoreSet {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// True when every store in the set failed to open (DB-level outage eligible for heal).
    #[must_use]
    pub fn is_down(&self) -> bool {
        self.regulation_store.is_none() && self.memory.is_none()
    }
}

/// Configuration for [`open_curator_store_set`].
#[derive(Debug, Clone, Copy)]
pub struct CuratorOpenConfig {
    pub embedding_dim: usize,
    pub memory_life_days: f64,
    /// When true, `MemoryStore` falls back to `try_new_without_embeddings` if
    /// `EmbeddingStore` fails to initialize (used by the MCP server so EAV/entity
    /// recall remains available). When false, embeddings are required.
    pub allow_degraded_embeddings: bool,
    /// When true, journals `reg.storage.open.<file_stem>` into the opened archive.
    pub journal_open_span: bool,
}

/// Pure domain failure classification for [`open_curator_store_set`].
#[derive(Debug)]
pub enum CuratorOpenFailure {
    MissingDbPath,
    MissingPassphrase,
    DatabaseOpen(hkask_storage::DatabaseError),
    PartialInit(String),
}

/// Open a sovereign Curator SQLCipher file once and construct [`RegulationArchive`],
/// [`HMemStore`], and [`EmbeddingStore`] over the single shared [`SqliteDriver`].
#[must_use]
pub fn open_curator_store_set(
    db_path: Option<&str>,
    passphrase: Option<&str>,
    config: CuratorOpenConfig,
) -> (CuratorStoreSet, Option<CuratorOpenFailure>) {
    let Some(db_path) = db_path else {
        tracing::warn!(target: "reg.memory", "Curator DB path not resolved");
        return (
            CuratorStoreSet::empty(),
            Some(CuratorOpenFailure::MissingDbPath),
        );
    };
    let Some(passphrase) = passphrase else {
        tracing::warn!(target: "reg.memory", "HKASK_DB_PASSPHRASE not set");
        return (
            CuratorStoreSet::empty(),
            Some(CuratorOpenFailure::MissingPassphrase),
        );
    };

    let opened_at = Instant::now();
    let open_role = Path::new(db_path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string());

    ensure_db_parent_dir(db_path);

    let db = match open_or_repair(db_path, passphrase) {
        Ok(db) => db,
        Err(error) => {
            tracing::warn!(
                target: "reg.memory",
                %error,
                db_path = %db_path,
                "Failed to open Curator SQLCipher database"
            );
            return (
                CuratorStoreSet::empty(),
                Some(CuratorOpenFailure::DatabaseOpen(error)),
            );
        }
    };
    let pool = match db.sqlite_pool() {
        Ok(pool) => pool,
        Err(error) => {
            tracing::warn!(
                target: "reg.memory",
                %error,
                db_path = %db_path,
                "Failed to obtain SQLite pool for Curator database"
            );
            return (
                CuratorStoreSet::empty(),
                Some(CuratorOpenFailure::DatabaseOpen(error)),
            );
        }
    };

    let mut fault: Option<String> = None;
    let driver: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new_labeled(pool, db_path));

    let regulation_store = match RegulationArchive::from_driver(Arc::clone(&driver)) {
        Ok(store) => Some(Arc::new(store)),
        Err(error) => {
            tracing::warn!(
                target: "reg.storage",
                %error,
                db_path = %db_path,
                "Failed to initialize RegulationArchive on shared Curator driver"
            );
            fault.get_or_insert_with(|| {
                format!("the Regulation archive did not initialise: {error}")
            });
            None
        }
    };

    let embedding_store =
        match EmbeddingStore::from_driver(Arc::clone(&driver), config.embedding_dim) {
            Ok(store) => Some(store),
            Err(error) => {
                tracing::warn!(
                    target: "reg.memory",
                    %error,
                    db_path = %db_path,
                    "Failed to initialize EmbeddingStore on shared Curator driver"
                );
                if !config.allow_degraded_embeddings {
                    return (
                        CuratorStoreSet {
                            regulation_store,
                            memory: None,
                        },
                        Some(CuratorOpenFailure::PartialInit(format!(
                            "the embedding store did not initialise: {error}"
                        ))),
                    );
                }
                None
            }
        };

    let attach_ledger = |store: MemoryStore| -> MemoryStore {
        match &regulation_store {
            Some(archive) => {
                let sink: Arc<dyn RegulationSink> = Arc::clone(archive) as Arc<dyn RegulationSink>;
                store.with_ledger(sink)
            }
            None => store,
        }
    };

    let memory = match HMemStore::from_driver(driver) {
        Ok(h_mem_store) => match embedding_store {
            Some(embeddings) => Some(Arc::new(attach_ledger(
                MemoryStore::new(h_mem_store, embeddings)
                    .with_memory_life_days(config.memory_life_days),
            ))),
            None => match MemoryStore::try_new_without_embeddings(h_mem_store) {
                Ok(store) => {
                    tracing::warn!(
                        target: "reg.memory",
                        db_path = %db_path,
                        "EmbeddingStore unavailable — Curator memory opened without embeddings"
                    );
                    Some(Arc::new(attach_ledger(
                        store.with_memory_life_days(config.memory_life_days),
                    )))
                }
                Err(error) => {
                    tracing::warn!(
                        target: "reg.memory",
                        %error,
                        db_path = %db_path,
                        "Failed to open Curator memory without embeddings"
                    );
                    fault = Some(format!("the memory store did not initialise: {error}"));
                    None
                }
            },
        },
        Err(error) => {
            tracing::warn!(
                target: "reg.memory",
                %error,
                db_path = %db_path,
                "Failed to initialize HMemStore on shared Curator driver"
            );
            fault = Some(format!("the memory store did not initialise: {error}"));
            None
        }
    };

    let open_duration_ms = opened_at.elapsed().as_millis() as u64;
    if config.journal_open_span
        && memory.is_some()
        && let Some(archive) = &regulation_store
    {
        journal_storage_open(archive, &open_role, open_duration_ms);
    }

    (
        CuratorStoreSet {
            regulation_store,
            memory,
        },
        fault.map(CuratorOpenFailure::PartialInit),
    )
}

/// Generic self-healing handle over a sovereign store state `S` and outage `E`.
///
/// Enforces retry backoff (`retry_interval`, default 5s), warn-once outage logging
/// keyed to this instance's own `db_label`, and side-effect-free [`Self::is_available`].
pub struct SelfHealingStore<S, E> {
    db_label: String,
    state: RwLock<S>,
    outage: RwLock<Option<E>>,
    heal_enabled: bool,
    retry_interval: Duration,
    heal_attempt_logged: AtomicBool,
    last_heal_attempt: Mutex<Option<Instant>>,
    is_down: fn(&S) -> bool,
    reopen: Box<dyn Fn() -> (S, Option<E>) + Send + Sync>,
}

impl<S, E> SelfHealingStore<S, E> {
    pub fn new(
        db_label: impl Into<String>,
        initial_state: S,
        initial_outage: Option<E>,
        heal_enabled: bool,
        is_down: fn(&S) -> bool,
        reopen: impl Fn() -> (S, Option<E>) + Send + Sync + 'static,
    ) -> Self {
        Self {
            db_label: db_label.into(),
            state: RwLock::new(initial_state),
            outage: RwLock::new(initial_outage),
            heal_enabled,
            retry_interval: DEFAULT_HEAL_RETRY_INTERVAL,
            heal_attempt_logged: AtomicBool::new(false),
            last_heal_attempt: Mutex::new(None),
            is_down,
            reopen: Box::new(reopen),
        }
    }

    /// Construct a handle with self-healing disabled (for pre-built test stores).
    pub fn disabled(initial_state: S, is_down: fn(&S) -> bool) -> Self
    where
        S: Clone + Send + Sync + 'static,
    {
        let fallback = initial_state.clone();
        Self::new(
            String::new(),
            initial_state,
            None,
            false,
            is_down,
            move || (fallback.clone(), None),
        )
    }

    /// Override the minimum retry interval between heal attempts.
    #[must_use]
    pub fn with_retry_interval(mut self, retry_interval: Duration) -> Self {
        self.retry_interval = retry_interval;
        self
    }

    /// Side-effect-free availability check: never triggers a re-open attempt.
    #[must_use]
    pub fn is_available(&self) -> bool {
        match self.state.read() {
            Ok(guard) => !(self.is_down)(&guard),
            Err(_) => false,
        }
    }

    /// Read the current store state, attempting a throttled re-open if down and enabled.
    pub fn get(&self) -> S
    where
        S: Clone + Default,
    {
        let currently_down = match self.state.read() {
            Ok(guard) => (self.is_down)(&guard),
            Err(_) => true,
        };
        if self.heal_enabled && currently_down && self.heal_due() {
            self.try_heal();
        }
        match self.state.read() {
            Ok(guard) => guard.clone(),
            Err(_) => S::default(),
        }
    }

    /// True when a non-`None` outage is currently recorded.
    #[must_use]
    pub fn has_outage(&self) -> bool {
        self.outage.read().ok().is_some_and(|guard| guard.is_some())
    }

    /// Read the most recently recorded outage reason, if any.
    pub fn outage(&self) -> Option<E>
    where
        E: Clone,
    {
        self.outage.read().ok().and_then(|guard| guard.clone())
    }

    /// Replace the store state directly (for test outage injection).
    pub fn replace_state(&self, state: S) {
        if let Ok(mut guard) = self.state.write() {
            *guard = state;
        }
    }

    fn heal_due(&self) -> bool {
        let now = Instant::now();
        let mut last = self
            .last_heal_attempt
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(prev) = *last
            && now.duration_since(prev) < self.retry_interval
        {
            return false;
        }
        *last = Some(now);
        true
    }

    fn try_heal(&self) {
        let (fresh, outage) = (self.reopen)();
        if let Ok(mut last) = self.last_heal_attempt.lock() {
            *last = Some(Instant::now());
        }
        if let Ok(mut recorded) = self.outage.write() {
            *recorded = outage;
        }
        let fresh_ok = !(self.is_down)(&fresh);
        match self.state.write() {
            Ok(mut guard) => {
                let was_down = (self.is_down)(&guard);
                if fresh_ok && was_down {
                    *guard = fresh;
                    tracing::info!(
                        target: "reg.memory",
                        db_path = %self.db_label,
                        "Curator sovereign store healed — store restored"
                    );
                    self.heal_attempt_logged.store(false, Ordering::Relaxed);
                } else if !fresh_ok && !self.heal_attempt_logged.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        target: "reg.memory",
                        db_path = %self.db_label,
                        "Curator sovereign store still unavailable after re-open attempt"
                    );
                }
            }
            Err(error) => {
                tracing::warn!(
                    target: "reg.memory",
                    %error,
                    db_path = %self.db_label,
                    "Curator sovereign store lock poisoned — cannot attempt heal"
                );
            }
        }
    }
}
