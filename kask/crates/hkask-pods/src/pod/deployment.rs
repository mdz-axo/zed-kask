//! PodDeployment — Per-pod deployment unit (Solid Pod isomorphism)
//!
//! Each pod IS the deployment unit with its own SQLCipher file, Regulation runtime,
//! and MCP server bindings. No shared state. No service collision surface.
//!
//! This replaces the centralized PodManager. PodRegistry provides filesystem-based
//! pod discovery (scanning {data_dir}/pods/*.db) — no HashMap cache.
//!
//! # Principles
//!
//! - \[P6\] Goal: Space for UserPods — each user inhabits their own userpod (1:1)
//!   - Constraining: Digital Public/Private Sphere — per-pod SQLCipher boundary
//!   - Constraining: Clear Boundaries — OCAP tokens scoped to this pod
//! - \[P5\] Goal: Essentialism — factory only; no runtime cache
//! - \[P9\] Goal: Homeostatic Self-Regulation — per-pod variety tracking

use hkask_capability::CapabilityChecker;
use hkask_mcp::McpRuntime;
use hkask_regulation::RegulationLedger;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::database::types::DbProvider;
use hkask_storage::{Database, EmbeddingStore, HMemStore};
use hkask_types::InferencePort;
use hkask_types::WebID;
use hkask_types::event::SpanNamespace;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info};

use super::types::{PodID, PodKind};
use super::{AgentPod, AgentPodError};
use crate::SovereigntyChecker;
use crate::curation::SemanticIndex;
use hkask_memory::{EpisodicStoragePort, SemanticStoragePort};
use hkask_templates::TemplateCrateLoader;

// ── PodDeployment — The pod IS the deployment unit ──────────────────────────

/// A pod IS the deployment unit. Constructing a PodDeployment
/// means: a SQLCipher database file exists at {data_dir}/pods/{pod_id}.db,
/// a Regulation runtime is initialized with namespace reg.agent_pod.{pod_id}.*,
/// and MCP servers are bound. No shared state. No service collision surface.
///
///Constraining: Digital Public/Private Sphere — per-pod SQLCipher boundary
pub struct PodDeployment {
    /// Pod identity — WebID is the root of all authority (P1)
    pub pod_id: PodID,
    /// The underlying AgentPod (identity, lifecycle, persona, capability token)
    pub pod: AgentPod,
    /// Dedicated database. The file IS the pod. No shared store.
    pub storage: PerPodStorage,
    /// Dedicated Regulation runtime. Variety counters scoped to this pod.
    pub ledger: PerPodLedger,
    /// Capability checker for OCAP verification
    pub capability_checker: Arc<CapabilityChecker>,
    /// Sovereignty checker wired to consent port
    pub sovereignty_checker: SovereigntyChecker,

    /// MCP runtime for tool dispatch
    pub mcp_runtime: Arc<McpRuntime>,
    /// Episodic storage port (backed by this pod's SQLCipher)
    pub episodic_storage: Arc<dyn EpisodicStoragePort>,
    /// Semantic storage port (backed by this pod's SQLCipher)
    pub semantic_storage: Arc<dyn SemanticStoragePort>,
    /// Inference port for LLM generation (None if inference unavailable)
    pub inference_port: Option<Arc<dyn InferencePort>>,
    /// Pod tier — determines isolation model
    pub pod_kind: PodKind,
    /// Semantic index — only set on CuratorPod
    pub semantic_index: Option<Arc<std::sync::RwLock<SemanticIndex>>>,
}

/// PerPodStorage owns a SQLCipher database file for a single pod.
/// The file IS the pod's data. Backup IS copying the file.
/// This type makes "shared store" an invalid state — you cannot
/// accidentally query another pod's data because you have no
/// connection handle to its file.
///
/// \[P11\] Goal: Digital Public/Private Sphere — storage isolation
pub struct PerPodStorage {
    /// The encrypted database connection
    pub db: Database,
    /// HMem store backed by this pod's database
    pub h_mems: HMemStore,
    /// Embedding store backed by this pod's database
    pub embeddings: EmbeddingStore,
    /// Path to this pod's database file
    pub db_path: PathBuf,
}

/// PerPodLedger is a Regulation runtime scoped to a single pod.
///
///Constraining: Digital Public/Private Sphere — Regulation isolation
#[derive(Clone)]
pub struct PerPodLedger {
    /// The pod this Regulation runtime is scoped to
    pod_id: PodID,
    /// Span namespace prefix: reg.agent_pod.{pod_id}
    span_namespace: String,
    /// The actual Regulation runtime — per-pod isolate
    inner: RegulationLedger,
}

// ── PodDeployError ──────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum PodDeployError {
    #[error("Failed to create pod storage at {path}: {reason}")]
    StorageInitFailed { path: PathBuf, reason: String },

    #[error("Failed to initialize Regulation runtime: {reason}")]
    RegInitFailed { reason: String },

    #[error("Pod lifecycle error: {0}")]
    PodError(#[from] AgentPodError),

    #[error("Template resolution failed: {0}")]
    TemplateError(String),

    #[error("Pod not found: {0}")]
    PodNotFound(PodID),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// ── PerPodLedger ────────────────────────────────────────────────────────

impl PerPodLedger {
    pub fn scoped(pod_id: PodID) -> Self {
        let span_namespace = format!("reg.pod.{}", pod_id);
        let inner = RegulationLedger::default();
        debug!(
            target: "reg.pod",
            pod_id = %pod_id,
            namespace = %span_namespace,
            "Per-pod Regulation runtime initialized"
        );
        Self {
            pod_id,
            span_namespace,
            inner,
        }
    }

    pub fn pod_id(&self) -> PodID {
        self.pod_id
    }
    pub fn span_namespace(&self) -> &str {
        &self.span_namespace
    }
    pub fn inner(&self) -> &RegulationLedger {
        &self.inner
    }

    pub async fn emit_tool_span(&self, tool_name: &str) {
        let domain = format!("reg.pod.{}.tool.{}", self.pod_id, tool_name);
        self.inner.increment_variety(&domain, tool_name).await;
    }

    pub async fn record_tool_outcome(
        &self,
        tool_name: &str,
        success: bool,
        error_kind: Option<&str>,
    ) {
        let domain = format!("reg.pod.{}.tool.{}", self.pod_id, tool_name);
        self.inner
            .record_outcome(&domain, success, error_kind)
            .await;
    }

    pub async fn health(&self) -> hkask_types::regulation::LedgerHealth {
        self.inner.health().await
    }

    pub async fn variety(&self) -> std::collections::HashMap<SpanNamespace, u64> {
        self.inner.variety().await
    }

    pub async fn variety_for_tool(&self, tool_name: &str) -> u64 {
        let domain = format!("reg.pod.{}.tool.{}", self.pod_id, tool_name);
        self.inner.variety_for_domain(&domain).await
    }

    pub async fn register_gas_budget(&self, agent: WebID, budget: hkask_regulation::GasBudget) {
        self.inner.register_gas_budget(agent, budget).await;
    }

    pub async fn agent_energy_status(
        &self,
        agent: &WebID,
    ) -> Option<hkask_regulation::AgentGasStatus> {
        self.inner.agent_gas_status(agent).await
    }
}

// ── PodFactory — Stateless pod constructor ──────────────────────────────────

/// PodFactory constructs PodDeployment instances from templates.
/// Stateless — does not cache, pool, or share pods.
///
pub struct PodFactory {
    template_loader: Arc<TemplateCrateLoader>,
    consent: Arc<dyn crate::SovereigntyConsent>,
    data_dir: PathBuf,
    db_provider: DbProvider,
}

impl PodFactory {
    pub fn new(
        template_loader: Arc<TemplateCrateLoader>,
        consent: Arc<dyn crate::SovereigntyConsent>,
        data_dir: PathBuf,
        db_provider: DbProvider,
    ) -> Self {
        Self {
            template_loader,
            consent,
            data_dir,
            db_provider,
        }
    }

    /// Deploy a new pod with its own SQLCipher database file,
    /// per-pod Regulation runtime, and MCP bindings.
    ///
    /// The pod's SQLCipher database uses the installation's canonical
    /// `HKASK_DB_PASSPHRASE`, resolved through `hkask-keystore`.
    ///
    ///Constraining: Digital Public/Private Sphere — per-pod SQLCipher
    #[allow(clippy::too_many_arguments)]
    pub async fn deploy(
        &self,
        template_name: &str,
        name: &str,
        webid: WebID,
        capabilities: Vec<String>,
        pod_kind: PodKind,
        mcp_runtime: Arc<McpRuntime>,
        capability_checker: Arc<CapabilityChecker>,
        inference_port: Option<Arc<dyn InferencePort>>,
    ) -> Result<PodDeployment, PodDeployError> {
        // 1. Create the underlying AgentPod
        let mut pod = AgentPod::new(
            template_name,
            name,
            webid,
            capabilities,
            self.template_loader.as_ref(),
            Arc::clone(&self.consent),
        )?;
        // Deterministic PodID from pod_kind + name (Solid Pod principle).
        let pod_id = PodID::from_name(&format!("{}:{}", pod_kind, name));
        pod.id = pod_id;

        // 2. Create per-pod SQLCipher database + storage adapters
        let (storage, memory_adapter) = self.create_pod_storage(pod_id, name, webid, pod_kind)?;

        // 3. Initialize per-pod Regulation runtime
        let ledger = PerPodLedger::scoped(pod_id);

        let sovereignty_checker = pod.sovereignty_checker.clone();

        let adapter: Arc<crate::adapters::memory_loop_adapter::MemoryLoopForwarder> =
            Arc::new(memory_adapter);
        let episodic: Arc<dyn EpisodicStoragePort> = adapter.clone();
        let semantic: Arc<dyn SemanticStoragePort> = adapter;

        // Create SemanticIndex if this is a CuratorPod
        let semantic_index = if pod_kind == PodKind::Curator {
            let pool = storage
                .db
                .sqlite_pool()
                .map_err(|e| PodDeployError::StorageInitFailed {
                    path: storage.db_path.clone(),
                    reason: format!("SQLite pool for curator semantic index: {e}"),
                })?;
            let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
                Arc::new(SqliteDriver::new(pool));
            let passphrase = super::resolve_db_passphrase()?;
            let index_store = HMemStore::from_driver(driver).with_passphrase(&passphrase);
            Some(Arc::new(std::sync::RwLock::new(SemanticIndex::new(
                index_store,
            ))))
        } else {
            None
        };

        info!(
            target: "hkask.pod.deployment",
            pod_id = %pod_id, template = %template_name,
            db_path = %storage.db_path.display(),
            reg_namespace = %ledger.span_namespace,
            "Pod deployed (self-contained SQLCipher storage)"
        );

        Ok(PodDeployment {
            pod_id,
            pod,
            storage,
            ledger,
            capability_checker,
            sovereignty_checker,
            mcp_runtime,
            episodic_storage: episodic,
            semantic_storage: semantic,
            inference_port,
            pod_kind,
            semantic_index,
        })
    }

    /// Create the per-pod SQLCipher database file, stores, and memory adapter.
    /// Returns both the storage struct and a MemoryLoopForwarder that wraps
    /// the pod's own HMemStore + EmbeddingStore for episodic/semantic I/O.
    fn create_pod_storage(
        &self,
        _pod_id: PodID,
        name: &str,
        webid: WebID,
        pod_kind: PodKind,
    ) -> Result<
        (
            PerPodStorage,
            crate::adapters::memory_loop_adapter::MemoryLoopForwarder,
        ),
        PodDeployError,
    > {
        let agent_name = name;
        let agent_dir = self
            .data_dir
            .join(hkask_types::agent_paths::USERPODS_DIR)
            .join(hkask_types::agent_paths::sanitize_name(agent_name));
        std::fs::create_dir_all(&agent_dir).map_err(|e| PodDeployError::StorageInitFailed {
            path: agent_dir.clone(),
            reason: e.to_string(),
        })?;

        let db_path = agent_dir.join("pod.db");
        let db_path_str = db_path.to_string_lossy().to_string();

        let passphrase =
            super::resolve_db_passphrase().map_err(|e| PodDeployError::StorageInitFailed {
                path: db_path.clone(),
                reason: e.to_string(),
            })?;

        // Memory is always SQLite/SQLCipher (per-agent encrypted storage)
        let db = hkask_storage::open_or_repair(&db_path_str, &passphrase).map_err(|e| {
            PodDeployError::StorageInitFailed {
                path: db_path.clone(),
                reason: format!("{e}"),
            }
        })?;

        // Embeddings: use configured provider
        let (embeddings, memory) = match self.db_provider {
            DbProvider::Sqlite => {
                let pool = db
                    .sqlite_pool()
                    .map_err(|e| PodDeployError::StorageInitFailed {
                        path: db_path.clone(),
                        reason: format!("SQLite pool creation failed: {e}"),
                    })?;
                let sqlite_driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
                    Arc::new(SqliteDriver::new(pool));
                let embeddings = EmbeddingStore::from_driver(Arc::clone(&sqlite_driver), 1024);
                let memory =
                    crate::adapters::memory_loop_adapter::MemoryLoopForwarder::from_driver(
                        sqlite_driver,
                    )
                    .map_err(|e| PodDeployError::StorageInitFailed {
                        path: db_path.clone(),
                        reason: e.to_string(),
                    })?;
                (embeddings, memory)
            }
            DbProvider::Postgres => {
                use hkask_storage::database::postgres::PostgresDriver;
                let pg_url = std::env::var("HKASK_DB_PATH")
                    .unwrap_or_else(|_| "postgresql://localhost:5432/hkask".into());
                let handle = tokio::runtime::Handle::current();
                let pool = handle.block_on(async {
                    sqlx::PgPool::connect(&pg_url).await.map_err(|e| {
                        PodDeployError::StorageInitFailed {
                            path: db_path.clone(),
                            reason: format!("Postgres connection failed: {e}"),
                        }
                    })
                })?;
                let pg_driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
                    Arc::new(PostgresDriver::new(pool, handle));
                let embeddings = EmbeddingStore::from_driver(Arc::clone(&pg_driver), 1024);
                let memory =
                    crate::adapters::memory_loop_adapter::MemoryLoopForwarder::from_driver(
                        pg_driver,
                    )
                    .map_err(|e| PodDeployError::StorageInitFailed {
                        path: db_path.clone(),
                        reason: e.to_string(),
                    })?;
                (embeddings, memory)
            }
        };

        // Write WebID sidecar for pod identity and CuratorSync provenance.
        let webid_path = db_path.with_extension("webid");
        std::fs::write(&webid_path, webid.to_string()).map_err(|e| {
            PodDeployError::StorageInitFailed {
                path: webid_path.clone(),
                reason: format!("Failed to write webid sidecar: {e}. CuratorSync depends on this file to sync the pod."),
            }
        })?;
        // Write pod kind sidecar for PodRegistry classification.
        let kind_path = db_path.with_extension("kind");
        let kind_str = match pod_kind {
            PodKind::Curator => "curator",
            PodKind::UserPod => "userpod",
        };
        std::fs::write(&kind_path, kind_str).map_err(|e| PodDeployError::StorageInitFailed {
            path: kind_path.clone(),
            reason: format!("Failed to write pod.kind sidecar: {e}"),
        })?;
        // Write pod.name sidecar with the original (unsanitized) agent name.
        // The directory name is sanitized for filesystem safety, but PodID
        // derivation and cross-pod references use the original name.
        let name_path = db_path.with_extension("name");
        std::fs::write(&name_path, name).map_err(|e| PodDeployError::StorageInitFailed {
            path: name_path.clone(),
            reason: format!("Failed to write pod.name sidecar: {e}"),
        })?;
        // Also write pod metadata into the database for backup/portability.
        {
            let pool = db
                .sqlite_pool()
                .map_err(|e| PodDeployError::StorageInitFailed {
                    path: db_path.clone(),
                    reason: format!("SQLite pool for metadata: {e}"),
                })?;
            let conn = pool.get().map_err(|e| PodDeployError::StorageInitFailed {
                path: db_path.clone(),
                reason: format!("Pool get failed: {e}"),
            })?;
            let now = chrono::Utc::now().to_rfc3339();
            for (key, value) in &[
                ("webid", webid.to_string()),
                ("pod_kind", format!("{:?}", pod_kind)),
                ("created_at", now),
            ] {
                conn.execute(
                    "INSERT OR REPLACE INTO pod_meta (key, value) VALUES (?1, ?2)",
                    rusqlite::params![key, value],
                )
                .map_err(|e| PodDeployError::StorageInitFailed {
                    path: db_path.clone(),
                    reason: format!("Failed to write pod_meta.{}: {e}", key),
                })?;
            }
        }

        // Create a driver for direct HMemStore access (persisted PerPodStorage.h_mems)
        let hmem_pool = db
            .sqlite_pool()
            .map_err(|e| PodDeployError::StorageInitFailed {
                path: db_path.clone(),
                reason: format!("SQLite pool for h_mems: {e}"),
            })?;
        let hmem_driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
            Arc::new(SqliteDriver::new(hmem_pool));
        let h_mems = HMemStore::from_driver(hmem_driver).with_passphrase(&passphrase);

        Ok((
            PerPodStorage {
                db,
                h_mems,
                embeddings,
                db_path,
            },
            memory,
        ))
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Export a pod as a container image build context.
    ///
    /// Produces a Containerfile + pod files in `output_dir`:
    /// - `Containerfile` — inlined at runtime via `format!()` (no external template)
    /// - `pods/{pod_id}.db` — copy of the pod's SQLCipher file
    /// - `pods/{pod_id}.webid` — copy of the webid sidecar
    ///
    /// After export: `docker build -t hkask-pod-{pod_id} {output_dir}`
    pub fn export_container(
        &self,
        pod_id: PodID,
        output_dir: &std::path::Path,
    ) -> Result<(), PodDeployError> {
        let registry = PodRegistry::new(&self.data_dir);
        let db_path = registry.db_path(&pod_id);
        if !db_path.exists() {
            return Err(PodDeployError::PodNotFound(pod_id));
        }

        let webid_path = db_path.with_extension("webid");
        let webid = std::fs::read_to_string(&webid_path).map_err(|_| {
            PodDeployError::StorageInitFailed {
                path: webid_path.clone(),
                reason: "Missing webid sidecar — pod identity cannot be resolved for export".into(),
            }
        })?;

        // Create output directory structure
        let pods_out = output_dir.join("pods");
        std::fs::create_dir_all(&pods_out).map_err(PodDeployError::Io)?;

        // Copy the pod's database, webid, and salt files
        std::fs::copy(&db_path, pods_out.join(format!("{}.db", pod_id)))
            .map_err(PodDeployError::Io)?;
        if webid_path.exists() {
            std::fs::copy(&webid_path, pods_out.join(format!("{}.webid", pod_id)))
                .map_err(PodDeployError::Io)?;
        }
        let salt_path = db_path.with_extension("db.salt");
        if salt_path.exists() {
            std::fs::copy(&salt_path, pods_out.join(format!("{}.db.salt", pod_id)))
                .map_err(PodDeployError::Io)?;
        }

        // Inline Containerfile via format!()
        let containerfile = format!(
            "# Pod Container — Generated by hKask v0.30.0\n\
             FROM hkask-runtime:0.30.0\n\
             LABEL hkask.pod.id=\"{pod_id}\"\n\
             LABEL hkask.pod.webid=\"{webid}\"\n\
             LABEL hkask.exported.at=\"{}\"\n\n\
             COPY pods/{pod_id}.db /data/pod.db\n\
             COPY pods/{pod_id}.webid /data/pod.webid\n\n\
             ENV HKASK_POD_ID={pod_id}\n\
             ENV HKASK_POD_MODE=standalone\n\n\
             ENTRYPOINT [\"kask\", \"pod\", \"serve\", \"--pod-id\", \"{pod_id}\"]\n",
            chrono::Utc::now().to_rfc3339()
        );
        std::fs::write(output_dir.join("Containerfile"), &containerfile)
            .map_err(PodDeployError::Io)?;

        info!(target: "hkask.pod.export", pod_id = %pod_id, output = %output_dir.display(),
            "Pod exported as container build context");
        Ok(())
    }
}

// ── PodRegistry — Filesystem-based pod discovery ────────────────────────────

/// Lightweight pod index — scans {data_dir}/agents/*/pod.db for deployed pods.
pub struct PodRegistry {
    agents_dir: PathBuf,
}

impl PodRegistry {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            agents_dir: data_dir.join(hkask_types::agent_paths::USERPODS_DIR),
        }
    }

    /// List all deployed pod IDs by scanning agent directories.
    pub fn list_pod_ids(&self) -> Result<Vec<PodID>, PodDeployError> {
        if !self.agents_dir.exists() {
            return Ok(Vec::new());
        }
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(&self.agents_dir)? {
            let entry = entry?;
            let agent_dir = entry.path();
            if !agent_dir.is_dir() {
                continue;
            }
            let pod_db = agent_dir.join("pod.db");
            if !pod_db.exists() {
                continue;
            }
            // Derive PodID from agent directory name (matches PodFactory naming)
            if let Some(agent_name) = agent_dir.file_name().and_then(|n| n.to_str()) {
                ids.push(PodID::from_name(agent_name));
            }
        }
        Ok(ids)
    }

    /// Check if a pod exists on disk.
    /// Scans agent directories for matching name-based PodID.
    pub fn pod_exists(&self, pod_id: &PodID) -> bool {
        self.db_path(pod_id).exists()
    }

    /// Get the database path for a pod by PodID.
    /// The PodID is derived from the agent name, so we reverse-lookup
    /// by scanning agent directories.
    pub fn db_path(&self, pod_id: &PodID) -> PathBuf {
        // Try to find the agent directory matching this PodID.
        // PodID is name-derived, so we iterate to find the match.
        if let Ok(entries) = std::fs::read_dir(&self.agents_dir) {
            for entry in entries.flatten() {
                let agent_dir = entry.path();
                if !agent_dir.is_dir() {
                    continue;
                }
                if let Some(agent_name) = agent_dir.file_name().and_then(|n| n.to_str())
                    && PodID::from_name(agent_name) == *pod_id
                {
                    return agent_dir.join("pod.db");
                }
            }
        }
        // Fallback: construct path from PodID string representation
        self.agents_dir.join(pod_id.to_string()).join("pod.db")
    }

    /// Scan agent directories for pod databases, read kind from pod.kind sidecar.
    /// Returns (PodKind, original_agent_name, db_path).
    pub fn scan_by_kind(&self) -> Result<Vec<(PodKind, String, PathBuf)>, PodDeployError> {
        if !self.agents_dir.exists() {
            return Ok(Vec::new());
        }
        let mut results = Vec::new();
        for entry in std::fs::read_dir(&self.agents_dir)? {
            let entry = entry?;
            let agent_dir = entry.path();
            if !agent_dir.is_dir() {
                continue;
            }
            let pod_db = agent_dir.join("pod.db");
            if !pod_db.exists() {
                continue;
            }
            // Read pod_kind from pod.kind sidecar
            let kind = read_pod_kind(&pod_db).unwrap_or_default();
            // Read original agent name from pod.name sidecar.
            // Falls back to directory name if sidecar is missing.
            let agent_name = read_pod_name(&pod_db).unwrap_or_else(|| {
                agent_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
            results.push((kind, agent_name, pod_db));
        }
        Ok(results)
    }

    /// Find the CuratorPod. Returns None if none found, first if multiple.
    pub fn find_curator(&self) -> Option<PathBuf> {
        let path = self.agents_dir.join("curator").join("pod.db");
        if path.exists() { Some(path) } else { None }
    }

    /// Find all TeamPods.
    pub fn find_teams(&self) -> Result<Vec<(String, PathBuf)>, PodDeployError> {
        // PodKind::Team removed in consolidation; no team pods exist.
        Ok(Vec::new())
    }
}

/// Read the pod kind from the pod.kind sidecar file.
fn read_pod_kind(db_path: &std::path::Path) -> Option<PodKind> {
    let kind_path = db_path.with_extension("kind");
    let content = std::fs::read_to_string(&kind_path).ok()?;
    match content.trim() {
        "curator" => Some(PodKind::Curator),
        "userpod" => Some(PodKind::UserPod),
        _ => None,
    }
}

/// Read the original (unsanitized) agent name from the pod.name sidecar.
fn read_pod_name(db_path: &std::path::Path) -> Option<String> {
    let name_path = db_path.with_extension("name");
    let content = std::fs::read_to_string(&name_path).ok()?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────
