use super::*;
use hkask_services_core::ServiceError;

mod foundation;
mod loops;
mod mcp_pods;
mod reg_wallet;

impl AgentService {
    /// Assemble all shared infrastructure from a `ServiceConfig`.
    ///
    /// This is the canonical construction path that replaces the four
    /// independent assemblies currently in the codebase. It resolves
    /// secrets, opens databases, constructs Regulation/loop system, governed
    /// tool membrane, and session manager in the correct dependency order.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  config must be a valid ServiceConfig with resolved secrets
    /// post: returns fully assembled AgentService with all infrastructure wired; Err on any construction step failure
    /// # Dependency order
    ///
    /// 1. Database connections (primary + per-purpose)
    /// 2. Stores (consent, escalation, goals, standing sessions)
    /// 3. Regulation runtime + event sink
    /// 4. Loop system + cybernetics loop
    /// 5. GovernedTool membrane + MCP dispatcher
    /// 6. A2A runtime + pod manager
    /// 7. Inference port (optional, based on config)
    /// 8. Memory adapters (episodic + semantic)
    pub async fn build(config: ServiceConfig) -> Result<Self, ServiceError> {
        Self::build_with_email(config, None).await
    }

    /// Assemble all shared infrastructure with an optional alert email sink.
    ///
    /// The sink closes the algedonic feedback loop via email when the live
    /// channel and archive persistence are both unavailable.
    pub async fn build_with_email(
        config: ServiceConfig,
        alert_email_sink: Option<Arc<dyn hkask_regulation::AlertEmailSink>>,
    ) -> Result<Self, ServiceError> {
        let system_webid = WebID::from_persona(config.user_name.as_bytes());

        // ── Foundation: database, stores, Regulation, seam watcher ──────────────
        let mut foundation = foundation::build_foundation(&config).await?;
        foundation.alert_email_sink = alert_email_sink;

        // ── Loops: cybernetics, inference, episodic, semantic, curation ──
        let loops = loops::build_loops(&config, &mut foundation, system_webid).await?;

        // ── MCP + pods: governed tool, dispatcher, pod manager, daemon ───
        let mcp_pods = mcp_pods::build_mcp_and_pods(&config, &loops, &foundation).await?;

        // ── Wire ManifestExecutor into CuratorContext (late-binding) ───
        // ManifestExecutor shares the governed MCP runtime built above.
        // The CuratorContext was created earlier (in build_loops) and stored
        // in LoopWiring for this late-binding step.
        mcp_pods::wire_manifest_executor(&loops, &mcp_pods.mcp_runtime, &config).await?;

        // ── Matrix transport + 7R7 listener ──────────────────────────────
        let matrix_transport =
            matrix::build_matrix(Some(Arc::clone(&foundation.reg_event_sink))).await;

        // Communication events are now pushed directly in CurationLoop.sense()
        // from the RegulationArchive query_algedonic results — no separate watcher needed.
        // See curator/curation_loop.rs for the integrated push.

        // ── MCP Server Guard (Loop 8) — proactive MCP server health ────
        let mcp_guard = Arc::new(crate::mcp_server_guard::McpServerGuardLoop::new(
            Arc::clone(&mcp_pods.mcp_runtime),
        ));
        loops.loop_system.register_loop(mcp_guard).await;

        // ── Registry + wallet: agent records, A2A restore, rJoule ───────
        let reg_wallet =
            reg_wallet::build_registry_and_wallet(&config, &foundation, &loops).await?;

        Ok(AgentServiceWiring {
            foundation,
            loops,
            mcp_pods,
            reg_wallet,
            matrix_transport,
            system_webid,
            config,
        }
        .into_service())
    }
}

// ── Build helpers ─────────────────────────────────────────────────────────-
// Extracted from build() for readability. Each helper constructs one
// subsystem and returns an intermediate struct consumed by the next step.

/// AgentService wiring — explicit composition boundary between subsystems.
struct AgentServiceWiring {
    foundation: foundation::Foundation,
    loops: loops::LoopWiring,
    mcp_pods: mcp_pods::McpPods,
    reg_wallet: reg_wallet::RegWallet,
    matrix_transport: Option<Arc<tokio::sync::Mutex<hkask_communication::matrix::MatrixTransport>>>,
    system_webid: WebID,
    config: ServiceConfig,
}

impl AgentServiceWiring {
    fn into_service(self) -> AgentService {
        let governance = governance::GovernanceContext::new(
            Arc::clone(&self.mcp_pods.capability_checker),
            Arc::clone(&self.foundation.consent_manager),
            Arc::clone(&self.loops.a2a_runtime),
            Arc::clone(&self.foundation.escalation_queue),
            Arc::clone(&self.foundation.reg_event_sink) as Arc<dyn RegulationSink>,
            self.foundation.curation_inbox_tx.clone(),
        );

        let ledger = regulation::RegulationContext::new(
            Arc::clone(&self.foundation.ledger_runtime),
            Arc::clone(&self.loops.cybernetics_loop),
            Arc::clone(&self.loops.loop_system),
            Arc::clone(&self.foundation.reg_event_sink) as Arc<dyn RegulationSink>,
            Arc::clone(&self.mcp_pods.energy_estimator),
            Arc::clone(&self.mcp_pods.tool_stats),
        );

        let storage = storage::StorageContext::new(
            self.reg_wallet.registry,
            self.foundation.goal_repo,
            self.foundation.user_store,
            self.foundation.sovereignty_boundary_store,
            self.reg_wallet.wallet_store,
        );

        let infra = infra::InfraContext::new(
            self.loops.inference_port,
            self.loops.episodic_storage,
            self.loops.semantic_storage,
            self.mcp_pods.mcp_runtime,
            self.mcp_pods.pod_manager,
            self.reg_wallet.wallet_service,
            self.mcp_pods.daemon_handler,
            self.matrix_transport,
            self.foundation.seam_watcher,
            self.reg_wallet.wallet_gas_calibrator,
        );

        AgentService {
            infra,
            governance,
            ledger,
            storage,
            system_webid: self.system_webid,
            curator_ready: Some(self.mcp_pods.curator_ready),
            config: self.config,
            inference_loop: None,
        }
    }
}
