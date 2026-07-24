//! Agent operational context — the full environment an agent needs to function.
//!
//! `AgentService` is the canonical composition root for hKask. It assembles
//! every piece of shared infrastructure an agent requires: Regulation for variety
//! sensing, cybernetics for energy budgeting, MCP for tool discovery, wallet
//! for rJoule payments, memory for episodic/semantic recall, and all stores
//! (consent, goals, registry, sovereignty).
//!
//! Both `ReplState` and `ApiState` compose an `AgentService` and add only
//! their surface-specific presentation fields. All infrastructure (inference,
//! memory, tool dispatch, gas tracking, consolidation) is accessed through
//! `AgentService` accessors — the four independent assembly paths that
//! previously existed (`ReplState` init, `ApiState` construction, loop
//! system wiring, CLI loop commands) have been consolidated into
//! `AgentService::build()`.
//!
//! # Adding new fields
//!
//! `AgentService` is the agent's operational world — not a dumping ground.
//! Before adding a field, apply the deletion test:
//! 1. Does the agent need this to function? If not, it belongs elsewhere.
//! 2. Does it already have a home crate/module? If yes, access it there.
//! 3. Is it surface-specific (CLI-only or API-only)? If yes, put it in the surface.

use std::sync::Arc;

use hkask_capability::CapabilityChecker;
use hkask_memory::{EpisodicStoragePort, SemanticStoragePort};
use hkask_pods::CuratorContext;
use hkask_pods::InferenceLoop;
use hkask_pods::LoopScheduler;
use hkask_pods::consent::ConsentManager;
use hkask_pods::curator_agent::CuratorAgent;
use hkask_pods::pod::ActivePods;
use hkask_regulation::types::loops::CuratorHandle;
use hkask_regulation::types::loops::RegulationLoop;
use hkask_regulation::types::loops::{CurationInput, CuratorDirective};
use hkask_regulation::{
    CalibratedEnergyEstimator, CyberneticsLoop, EnergyEstimator, RegulationLedger, SeamSummary,
    SeamWatcher, load_set_points,
};
use hkask_storage::database::sqlite::SqliteDriver;

use hkask_mcp::runtime::McpRuntime;
use hkask_memory::{
    ConsolidationBridge, EpisodicLoop, EpisodicMemory, SemanticLoop, SemanticMemory,
};
use hkask_storage::EscalationQueue;
use hkask_storage::goals::SqliteGoalRepository;
use hkask_storage::regulation_store::RegulationArchive;
use hkask_storage::user_store::UserStore;
use hkask_storage::{
    ConsentStore, Database, EmbeddingStore, HMemStore, SovereigntyBoundaryStore, WalletStore,
};
use hkask_templates::SqliteRegistry;
use hkask_types::DataCategory;
use hkask_types::WebID;
use hkask_types::event::RegulationSink;
use hkask_types::id::WalletId;
use hkask_types::{ConsolidationOutcome, ConsolidationRequest, InferencePort, LedgerStoragePort};

use hkask_services_core::{DomainKind, ErrorKind, ServiceConfig, ServiceError};

use hkask_services_wallet::WalletService;

mod matrix;
mod seam_monitor;

use crate::governance;
use crate::infra;
use crate::regulation;
use crate::storage;

/// Agent operational context — canonical composition root for hKask.
///
/// Holds every piece of shared infrastructure an agent needs: Regulation,
/// cybernetics, MCP, wallet, memory, stores, pod manager, Matrix transport.
/// Surfaces (`ReplState`, `ApiState`) compose this struct and add only
/// presentation-specific fields.
///
/// Construct via `AgentService::build(config)`. The config provides all
/// deployment-varying parameters (DB paths, secrets, thresholds, model names).
/// The builder resolves the dependency graph canonically: stores → Regulation →
/// loop system → governed tool → pod manager.
///
/// # Field discipline
///
/// This is the agent's operational world — not a dumping ground. Before
/// adding a field, apply the deletion test (see module docs). Every field
/// here must be something an agent needs to function, not something that
/// was convenient to stash.
///
/// `#[non_exhaustive]` prevents external crates from constructing this struct
/// with struct literal syntax — use `AgentService::build()` instead.
#[non_exhaustive]
pub struct AgentService {
    /// Infrastructure context — inference, memory, MCP, pods,
    infra: infra::InfraContext,

    /// Governance context — OCAP, consent, dispatch, A2A, escalations.
    governance: governance::GovernanceContext,

    /// Regulation context — variety sensing, cybernetic regulation,
    /// loop orchestration, event audit trail, energy estimation.
    ledger: regulation::RegulationContext,

    /// Storage context — registry, goals, agents, users,
    /// sovereignty boundaries, wallet store.
    storage: storage::StorageContext,

    /// System WebID for signing capabilities.
    system_webid: WebID,

    /// Signals CuratorPod activation.
    curator_ready: Option<tokio::sync::oneshot::Receiver<()>>,

    /// Configuration used to build this context.
    config: ServiceConfig,

    /// Inference loop for gas budget queries. Set by surfaces after
    /// construction via `set_inference_loop()`, then queried via
    /// `gas_remaining()` and `gas_cap()`.
    inference_loop: Option<Arc<InferenceLoop>>,
}

/// Per-agent memory infrastructure — storage ports and ConsolidationService
/// constructed from a single agent-scoped Database connection.
///
/// All components share the same underlying DB, so consolidation operates
/// on the agent's actual episodic and semantic h_mems.
pub struct PerAgentMemory {
    pub episodic_storage: Arc<dyn EpisodicStoragePort>,
    pub semantic_storage: Arc<dyn SemanticStoragePort>,
    pub consolidation_service: hkask_memory::ConsolidationService,
}

impl From<&AgentService> for hkask_services_inference::InferenceContext {
    fn from(ctx: &AgentService) -> Self {
        Self {
            shared_port: ctx.infra().inference.clone(),
            default_model: ctx.config().default_model.clone(),
            inference_config: ctx.config().inference_config.clone(),
        }
    }
}

impl AgentService {
    // === Configuration ===

    /// Access configuration.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  self must be fully built
    /// post: returns reference to ServiceConfig
    pub fn config(&self) -> &ServiceConfig {
        &self.config
    }

    /// System WebID for signing capabilities and identity.
    /// Replaces the common `ctx.webid()` pattern.
    pub fn webid(&self) -> &WebID {
        &self.system_webid
    }

    // --- Sub-contexts ---

    /// Regulation context — variety sensing, cybernetic regulation,
    /// loop orchestration, events, and energy estimation.
    pub fn ledger(&self) -> &regulation::RegulationContext {
        &self.ledger
    }

    /// Storage context — registry, goals, specs, agents,
    /// users, sovereignty boundaries, and wallet store.
    pub fn storage(&self) -> &storage::StorageContext {
        &self.storage
    }

    // Memory — episodic and semantic storage ports.

    /// Public seam watcher — delegated to infra context.
    pub async fn seam_summary(&self) -> Option<SeamSummary> {
        self.infra.seam_summary().await
    }

    // --- Governance ---
    /// Consolidated governance context — OCAP, consent, dispatch,
    /// A2A registration, escalation queue, curation signals.
    pub fn governance(&self) -> &governance::GovernanceContext {
        &self.governance
    }

    /// Infrastructure context — inference, memory, MCP, pods,
    pub fn infra(&self) -> &infra::InfraContext {
        &self.infra
    }

    /// System WebID + A2A runtime.
    pub fn identity(&self) -> (&WebID, &Arc<hkask_pods::A2ARuntime>) {
        (&self.system_webid, &self.governance.a2a)
    }

    /// Await CuratorPod activation. Consumes the oneshot — call once.
    pub async fn curator_ready(&mut self) -> anyhow::Result<()> {
        let rx = self
            .curator_ready
            .take()
            .ok_or_else(|| anyhow::anyhow!("curator_ready already consumed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("CuratorPod failed to activate — check startup logs"))
    }

    /// Build per-agent memory infrastructure from an agent-scoped Database.
    ///
    /// Constructs storage ports (`EpisodicStoragePort`, `SemanticStoragePort`)
    /// and a `ConsolidationService` — all sharing the same underlying DB
    /// connection so consolidation operates on the agent's actual h_mems.
    ///
    /// This is used by the REPL to build agent-scoped memory (separate from
    /// the shared `AgentService` memory adapted for loops).
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  db must be a valid opened Database
    /// post: returns PerAgentMemory with episodic_storage, semantic_storage, and consolidation_service all sharing the same DB
    #[must_use]
    pub fn build_per_agent_memory(
        db: Database,
        reg_event_sink: Option<Arc<dyn RegulationSink>>,
    ) -> PerAgentMemory {
        // EpisodicMemory + SemanticMemory for ConsolidationService
        let pool_for_mem = db.sqlite_pool().expect("sqlite pool error");
        let mem_driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
            Arc::new(SqliteDriver::new(pool_for_mem));
        let ts1 = HMemStore::from_driver(Arc::clone(&mem_driver));
        let mut episodic_memory = EpisodicMemory::new(ts1);
        if let Some(ref sink) = reg_event_sink {
            episodic_memory = episodic_memory.with_ledger(Arc::clone(sink));
        }
        let episodic_memory = Arc::new(episodic_memory);
        let ts2 = HMemStore::from_driver(mem_driver);
        let pool = db.sqlite_pool().expect("sqlite pool error");
        let driver = SqliteDriver::new(pool);
        let emb = EmbeddingStore::from_driver(
            Arc::new(driver) as Arc<dyn hkask_storage::database::driver::DatabaseDriver>,
            1024,
        );
        let mut semantic_memory = SemanticMemory::new(ts2, emb);
        if let Some(ref sink) = reg_event_sink {
            semantic_memory = semantic_memory.with_ledger(Arc::clone(sink));
        }
        let semantic_memory = Arc::new(semantic_memory);

        // ConsolidationService from the shared memories
        let bridge = Arc::new(ConsolidationBridge::new(
            Arc::clone(&episodic_memory),
            Arc::clone(&semantic_memory),
        ));
        let consolidation_service =
            hkask_memory::ConsolidationService::new(bridge, Arc::clone(&semantic_memory));

        // Storage ports via MemoryLoopForwarder — reuse configured memories
        let adapter = Arc::new(hkask_pods::adapters::MemoryLoopForwarder::new(
            Arc::clone(&episodic_memory),
            Arc::clone(&semantic_memory),
        ));

        PerAgentMemory {
            episodic_storage: adapter.clone() as Arc<dyn EpisodicStoragePort>,
            semantic_storage: adapter as Arc<dyn SemanticStoragePort>,
            consolidation_service,
        }
    }

    /// Consolidate episodic memory into semantic memory for a specific agent.
    ///
    /// This is the canonical entry point for all user- and Curator-triggered
    /// consolidation operations. It verifies P2 affirmative consent for both
    /// `EpisodicMemory` and `SemanticMemory` before opening the agent's
    /// per-agent memory DB and running the consolidation pipeline.
    ///
    /// \[P2\] Constraining: Affirmative Consent — consolidation is blocked unless
    /// both memory categories are explicitly granted for the target agent's WebID.
    /// \[P4\] Constraining: Clear Boundaries — all consolidation flows through
    /// `AgentService`, preventing direct `Database::open` bypasses.
    ///
    /// pre:  userpod_name is non-empty; request is a valid ConsolidationRequest
    /// post: returns ConsolidationOutcome on success; Err(ConsentDenied) if either
    ///       memory category lacks consent; Err(Storage) on DB open failure;
    ///       Err(Consolidation) on pipeline failure
    #[must_use = "result must be used"]
    pub fn consolidate_userpod_memory(
        &self,
        userpod_name: &str,
        request: ConsolidationRequest,
    ) -> Result<ConsolidationOutcome, ServiceError> {
        let target_webid = WebID::for_userpod_name(userpod_name);

        // P2: require explicit consent for both sovereign memory categories.
        let categories = [DataCategory::EpisodicMemory, DataCategory::SemanticMemory];
        let missing: Vec<String> = categories
            .iter()
            .filter(|cat| {
                !self
                    .governance
                    .consent
                    .has_consent(&target_webid.to_string(), cat)
                    .unwrap_or(false)
            })
            .map(|cat| cat.to_string())
            .collect();

        if !missing.is_empty() {
            let grant_help = if userpod_name == "curator" {
                "Grant consent with: kask sovereignty grant --category <category> --agent curator"
                    .to_string()
            } else {
                "Grant consent with: kask sovereignty grant --category <category>".to_string()
            };
            return Err(ServiceError::Domain {
                kind: ErrorKind::Forbidden,
                domain: DomainKind::Consent,
                source: None,
                message: format!(
                    "consolidation denied for agent {} — missing consent for: {}. {grant_help}",
                    target_webid.redacted_display(),
                    missing.join(", ")
                ),
            });
        }

        let db_path = hkask_types::agent_paths::userpod_memory_db(userpod_name);
        let db = Database::open(&db_path.to_string_lossy(), &self.config.db_passphrase).map_err(
            |e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: None,
                message: e.to_string(),
            },
        )?;

        let per_agent_memory =
            Self::build_per_agent_memory(db, Some(Arc::clone(&self.ledger.events)));
        per_agent_memory
            .consolidation_service
            .consolidate(&target_webid, request)
            .map_err(|e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Memory,
                source: None,
                message: e.to_string(),
            })
    }

    /// Query consolidation status for an agent without running consolidation.
    ///
    /// Opens the per-agent memory DB temporarily and returns
    /// (candidates, semantic_count, low_confidence_count).
    /// This is the canonical read path for consolidation status — the REPL,
    /// TUI bridge, and CLI status display all route through this method.
    ///
    /// pre:  userpod_name is non-empty
    /// post: returns (candidates, semantic_count, low_confidence) on success;
    ///       Err(Storage) on DB open failure
    #[must_use = "result must be used"]
    pub fn consolidation_status_for(
        &self,
        userpod_name: &str,
    ) -> Result<(usize, usize, usize), ServiceError> {
        let target_webid = WebID::for_userpod_name(userpod_name);

        let db_path = hkask_types::agent_paths::userpod_memory_db(userpod_name);
        let db = Database::open(&db_path.to_string_lossy(), &self.config.db_passphrase).map_err(
            |e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: None,
                message: e.to_string(),
            },
        )?;

        let per_agent_memory = Self::build_per_agent_memory(db, None);
        let cs = &per_agent_memory.consolidation_service;
        let candidates = cs.consolidation_candidate_count(&target_webid);
        let semantic_count = cs.semantic_h_mem_count();
        let low_confidence = cs.semantic_low_confidence_count(0.33);

        Ok((candidates, semantic_count, low_confidence))
    }

    // --- Surface infrastructure accessors ---

    /// Wire a surface's InferenceLoop for gas queries.
    /// Call once after construction, before gas queries are made.
    pub fn set_inference_loop(&mut self, il: Arc<InferenceLoop>) {
        self.inference_loop = Some(il);
    }

    /// Access the inference loop for gas budget operations.
    /// Returns `None` if `set_inference_loop` was not called.
    pub fn inference_loop(&self) -> Option<&Arc<InferenceLoop>> {
        self.inference_loop.as_ref()
    }

    /// Gas budget remaining. Returns `None` if no inference loop is wired
    /// (callers should treat this as "gas tracking unavailable", not "budget exhausted").
    pub fn gas_remaining(&self) -> Option<u64> {
        self.inference_loop.as_ref().map(|il| il.gas_remaining())
    }

    /// Gas budget cap. Returns `None` if no inference loop is wired.
    pub fn gas_cap(&self) -> Option<u64> {
        self.inference_loop.as_ref().map(|il| il.gas_cap())
    }

    /// Inference port for surface inference calls. Returns the governed
    /// port if configured (preferred — Regulation-observable), or `None` if
    /// inference is unavailable.
    pub fn inference_port(&self) -> Option<Arc<dyn InferencePort>> {
        self.infra.inference.clone()
    }

    /// Open per-agent memory for the given agent and return all storage
    /// ports and consolidation service from a single DB open.
    ///
    /// Prefer this over individual accessors — it avoids opening the
    /// agent's memory DB multiple times when a caller needs more than
    /// one component.
    #[must_use = "result must be used"]
    pub fn per_agent_memory(&self, userpod_name: &str) -> Result<PerAgentMemory, ServiceError> {
        let db_path = hkask_types::agent_paths::userpod_memory_db(userpod_name);
        let db = Database::open(&db_path.to_string_lossy(), &self.config.db_passphrase).map_err(
            |e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: None,
                message: e.to_string(),
            },
        )?;
        Ok(Self::build_per_agent_memory(
            db,
            Some(Arc::clone(&self.ledger.events)),
        ))
    }
}

mod build;
