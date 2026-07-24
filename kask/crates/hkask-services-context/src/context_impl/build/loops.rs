//! Loop wiring: cybernetics, inference, episodic, semantic, curation, snapshot, backup.

use super::super::*;
use hkask_storage::database::sqlite::SqliteDriver;

use super::foundation::Foundation;
use crate::reg_store_slo_provider::RegStoreSloProvider;
use hkask_regulation::DEFAULT_SET_POINT_CALIBRATION_INTERVAL;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::{LedgerStoragePort, escalation::EscalationPort, event::RegulationSink};
use std::path::PathBuf;
use tokio::sync::RwLock;

/// Loop wiring: cybernetics, inference, episodic, semantic, curation, snapshot, backup.
pub(super) struct LoopWiring {
    pub loop_system: Arc<LoopScheduler>,
    /// Concrete GixCasAdapter for pod-directory backup operations.
    pub pod_backup_adapter: Arc<hkask_git_cas::GixCasAdapter>,
    pub cybernetics_loop: Arc<RwLock<CyberneticsLoop>>,
    pub inference_port: Option<Arc<dyn InferencePort>>,
    pub episodic_storage: Arc<dyn EpisodicStoragePort>,
    pub semantic_storage: Arc<dyn SemanticStoragePort>,
    pub a2a_runtime: Arc<hkask_pods::A2ARuntime>,
    /// CuratorContext — late-bound ManifestExecutor set after MCP pods built.
    pub curator_context: Arc<hkask_pods::CuratorContext>,
}

impl LoopWiring {
    pub(super) async fn validate(&self) -> Result<(), ServiceError> {
        if self.inference_port.is_some() && !self.curator_context.has_manifest_executor().await {
            return Err(ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Infrastructure,
                source: None,
                message: "Loop wiring incomplete: inference enabled but CuratorContext lacks ManifestExecutor"
                    .to_string(),
            });
        }
        Ok(())
    }
}

pub(super) async fn build_loops(
    config: &ServiceConfig,
    f: &mut Foundation,
    system_webid: WebID,
) -> Result<LoopWiring, ServiceError> {
    let loop_system = Arc::new(LoopScheduler::new());

    let (curator_directive_tx, curator_directive_rx) =
        tokio::sync::mpsc::unbounded_channel::<CuratorDirective>();

    // Cybernetics loop
    let set_points = load_set_points();
    let cybernetics_loop =
        CyberneticsLoop::with_set_points(Arc::clone(&f.ledger_runtime), set_points)
            .with_event_sink(Arc::clone(&f.reg_event_sink))
            .with_alerts_channel(f.curation_inbox_tx.clone())
            .with_curator_directive_channel(curator_directive_rx)
            .with_slo_provider(Arc::new(RegStoreSloProvider::new(Arc::clone(
                &f.regulation_store,
            ))))
            .with_set_point_calibrator(
                Arc::clone(&f.regulation_store) as Arc<dyn LedgerStoragePort>,
                DEFAULT_SET_POINT_CALIBRATION_INTERVAL,
            )
            .with_seam_watcher();
    let cybernetics_loop = if let Some(ref sink) = f.alert_email_sink {
        cybernetics_loop.with_alert_email_sink(Arc::clone(sink))
    } else {
        cybernetics_loop
    };
    let cybernetics_loop = Arc::new(RwLock::new(cybernetics_loop));
    loop_system
        .register_loop(Arc::clone(&cybernetics_loop) as Arc<dyn RegulationLoop>)
        .await;

    // Inference loop (optional)
    let inference_port: Option<Arc<dyn InferencePort>> = if config.in_memory {
        None
    } else {
        let router = hkask_inference::InferenceRouter::new(config.inference_config.clone())
            .with_governance(
                Arc::clone(&cybernetics_loop),
                Arc::clone(&f.reg_event_sink),
                system_webid,
            );
        // Wrap with GuardedInferencePort so every InferencePort call (executor
        // select/populate, REPL chat turn, condenser) is content-scanned at the
        // LLM I/O boundary — universal by construction, not caller opt-in.
        let guarded_port: Arc<dyn InferencePort> =
            Arc::new(hkask_guard::GuardedInferencePort::new(
                Arc::new(router) as Arc<dyn InferencePort>,
                hkask_guard::ContentGuard::mandatory(&hkask_guard::GuardConfig::from_env()),
            ));
        let inference_loop = hkask_pods::InferenceLoop::new()
            .with_energy_budget(config.energy_budget_cap, config.gas_replenish_rate)
            .with_model(&config.default_model);
        loop_system.register_loop(Arc::new(inference_loop)).await;
        Some(guarded_port)
    };

    // Episodic + Semantic memory
    let mem_driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> = if config.in_memory {
        let pool = f.db.sqlite_pool().map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: format!("pool error: {e}"),
        })?;
        Arc::new(SqliteDriver::new(pool))
    } else {
        let path = config
            .effective_memory_db_path()
            .expect("effective_memory_db_path returns Some when !in_memory");
        let mem_db =
            Database::open(&path, &config.db_passphrase).map_err(|e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: None,
                message: e.to_string(),
            })?;
        let pool = mem_db.sqlite_pool().map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: format!("pool error: {e}"),
        })?;
        Arc::new(SqliteDriver::new(pool))
    };
    let h_mem_store = HMemStore::from_driver(Arc::clone(&mem_driver));
    let memory_life_days = config.memory_life_days;
    let episodic_memory = Arc::new(
        EpisodicMemory::new(h_mem_store)
            .with_memory_life_days(memory_life_days)
            .with_ledger(Arc::clone(&f.reg_event_sink)),
    );
    let storage_budget = episodic_memory.storage_budget();
    let episodic_loop =
        EpisodicLoop::new(Arc::clone(&episodic_memory), system_webid, storage_budget);
    loop_system.register_loop(Arc::new(episodic_loop)).await;

    let h_mem_store2 = HMemStore::from_driver(Arc::clone(&mem_driver));
    let embedding_store = EmbeddingStore::from_driver(Arc::clone(&mem_driver), 1024);
    let semantic_memory = Arc::new(
        SemanticMemory::new(h_mem_store2, embedding_store)
            .with_ledger(Arc::clone(&f.reg_event_sink)),
    );
    let semantic_loop = SemanticLoop::new(Arc::clone(&semantic_memory));
    loop_system.register_loop(Arc::new(semantic_loop)).await;

    // Memory adapter — reuses configured memory instances (shared store, shared config)
    let memory_adapter = Arc::new(
        hkask_pods::adapters::memory_loop_adapter::MemoryLoopForwarder::new(
            Arc::clone(&episodic_memory),
            Arc::clone(&semantic_memory),
        ),
    );
    let episodic_storage: Arc<dyn EpisodicStoragePort> = memory_adapter.clone();
    let semantic_storage: Arc<dyn SemanticStoragePort> = memory_adapter.clone();

    // Curation loop
    let reg_for_curator: Arc<RegulationLedger> = Arc::new(f.ledger_runtime.read().await.clone());
    let a2a_runtime = Arc::new(hkask_pods::A2ARuntime::new(&config.a2a_secret));
    let curator_context = Arc::new(
        CuratorContext::with_regulation_store(
            CuratorHandle::system(),
            reg_for_curator,
            Some(curator_directive_tx.clone()),
            Arc::clone(&f.escalation_queue) as Arc<dyn EscalationPort>,
            Arc::clone(&f.regulation_store) as Arc<dyn LedgerStoragePort>,
        )
        .with_a2a(a2a_runtime.clone())
        .with_consent_manager(Arc::clone(&f.consent_manager))
        .with_regulation_sink(Arc::clone(&f.regulation_store) as Arc<dyn RegulationSink>),
    );
    // Clone before move into CuratorAgent — stored in LoopWiring for late-binding
    // ManifestExecutor wiring after MCP pods are built.
    let curator_context_for_loops = Arc::clone(&curator_context);
    let consolidation_bridge = Arc::new(ConsolidationBridge::new(
        Arc::clone(&episodic_memory),
        Arc::clone(&semantic_memory),
    ));
    let curator_agent = CuratorAgent::with_consolidation(
        curator_context,
        Default::default(),
        Arc::clone(&consolidation_bridge),
        f.curation_inbox_rx
            .take()
            .expect("curation_inbox_rx consumed once"),
        config.curator_auto_consolidation_enabled,
    );
    curator_agent.curation_loop().restore_cursor();
    let curation_loop: Arc<dyn RegulationLoop> = curator_agent.curation_loop().clone();
    loop_system.register_loop(curation_loop).await;
    let metacognition_loop: Arc<dyn RegulationLoop> = curator_agent.metacognition().clone();
    loop_system.register_loop(metacognition_loop).await;

    // ── StorageGuard (Loop 7) — autonomous disk space management ──────
    let storage_guard_config = crate::storage_guard::StorageGuardConfig {
        data_dir: std::env::var("HKASK_DATA_DIR").unwrap_or_else(|_| "/data".to_string()),
        ..Default::default()
    };
    let storage_guard = Arc::new(crate::storage_guard::StorageGuardLoop::new(
        storage_guard_config,
    ));
    loop_system.register_loop(storage_guard).await;

    // Snapshot + Backup loops — keep concrete GixCasAdapter for pod-directory ops
    let gix_adapter: Arc<hkask_git_cas::GixCasAdapter> =
        match hkask_git_cas::GixCasAdapter::from_env() {
            Ok(adapter) => Arc::new(adapter),
            Err(e) => {
                tracing::warn!(target: "hkask.services", error = %e, "Git CAS port from env failed — using fallback");
                Arc::new(
                    hkask_git_cas::GixCasAdapter::new(PathBuf::from("/tmp/hkask-templates"))
                        .map_err(|e| {
                            ServiceError::Infra(hkask_types::InfrastructureError::Io(e.to_string()))
                        })?,
                )
            }
        };

    Ok(LoopWiring {
        loop_system,
        pod_backup_adapter: gix_adapter,
        cybernetics_loop,
        inference_port,
        episodic_storage,
        semantic_storage,
        a2a_runtime,
        curator_context: curator_context_for_loops,
    })
}

/// Thin pod backup daemon: wake every 24h, snapshot all pod directories via gix.
/// One git repo per pod. The pod directory IS the unit.
pub(super) async fn pod_backup_daemon(
    adapter: Arc<hkask_git_cas::GixCasAdapter>,
    pod_manager: Arc<hkask_pods::pod::ActivePods>,
) {
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(86400); // 24h

    loop {
        tokio::time::sleep(INTERVAL).await;

        let pod_dirs = pod_manager.pod_db_paths().await;
        if pod_dirs.is_empty() {
            continue;
        }

        tracing::info!(
            target: "reg.backup",
            pod_count = pod_dirs.len(),
            "Pod backup: snapshotting {} pods",
            pod_dirs.len()
        );

        for (pod_name, db_path) in &pod_dirs {
            let pod_dir = match db_path.parent() {
                Some(d) => d.to_path_buf(),
                None => continue,
            };

            match adapter
                .snapshot_pod_dir(&pod_dir, &format!("auto: {}", pod_name))
                .await
            {
                Ok(commit) => {
                    tracing::info!(
                        target: "reg.backup",
                        pod = %pod_name,
                        commit = %commit,
                        "Pod backup: snapshot complete"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        target: "reg.backup",
                        pod = %pod_name,
                        error = %e,
                        "Pod backup: snapshot failed"
                    );
                }
            }
        }
    }
}
