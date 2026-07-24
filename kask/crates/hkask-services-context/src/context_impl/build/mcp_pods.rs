//! MCP + pods: governed tool, dispatcher, pod manager, daemon handler.

use super::super::*;
use super::foundation::Foundation;
use super::loops::LoopWiring;
use hkask_regulation::set_point_calibrator::SetPointCalibrator;
use hkask_services_core::ServiceError;

/// MCP + pods: governed tool, dispatcher, pod manager, daemon handler.
pub(super) struct McpPods {
    pub mcp_runtime: Arc<McpRuntime>,

    pub pod_manager: Arc<ActivePods>,
    pub capability_checker: Arc<CapabilityChecker>,
    pub daemon_handler: Arc<hkask_services_runtime::ServiceDaemonHandler>,
    pub energy_estimator: Arc<hkask_regulation::CalibratedEnergyEstimator>,
    /// Statistical learner shared between GovernedTool and CyberneticsLoop.
    pub tool_stats: Arc<hkask_regulation::ToolStats>,

    pub curator_ready: tokio::sync::oneshot::Receiver<()>,
}

pub(super) async fn build_mcp_and_pods(
    config: &ServiceConfig,
    l: &LoopWiring,
    f: &Foundation,
) -> Result<McpPods, ServiceError> {
    // Governed McpRuntime — OCAP + gas + Regulation wired in via with_governance.
    let energy_estimator: Arc<CalibratedEnergyEstimator> = Arc::new(
        CalibratedEnergyEstimator::new(Arc::clone(&f.gas_event_store))
            .with_event_sink(Arc::clone(&f.reg_event_sink)),
    );
    energy_estimator
        .clone()
        .spawn_calibration(hkask_regulation::DEFAULT_CALIBRATION_INTERVAL);

    // Set-point auto-tuning calibrator
    let set_point_calibrator: Arc<SetPointCalibrator> = Arc::new(SetPointCalibrator::new(
        Arc::clone(&f.gas_event_store),
        hkask_regulation::DEFAULT_INITIAL_LOOKBACK,
    ));
    {
        let loop_ref = Arc::clone(&l.cybernetics_loop);
        set_point_calibrator.clone().spawn_calibration(
            hkask_regulation::DEFAULT_SET_POINT_CALIBRATION_INTERVAL,
            move |adjustments| {
                if let Ok(mut guard) = loop_ref.try_write() {
                    let sp = guard.set_points_mut();
                    hkask_regulation::set_point_calibrator::SetPointCalibrator::apply_adjustments(
                        &adjustments,
                        &mut sp.stagnation_thresholds,
                        &mut sp.block_worsening_ratio,
                        &mut sp.substitution_after,
                    );
                }
            },
        );
    }
    let estimator: Arc<dyn EnergyEstimator> =
        Arc::clone(&energy_estimator) as Arc<dyn EnergyEstimator>;

    // Wire ToolStats into the CyberneticsLoop for reliability sensor.
    let tool_stats = f.ledger_runtime.read().await.tool_stats().await;
    l.cybernetics_loop
        .write()
        .await
        .set_tool_stats(Arc::clone(&tool_stats));

    let mcp_runtime = Arc::new(McpRuntime::new().with_governance(
        Arc::clone(&l.cybernetics_loop),
        Arc::clone(&f.reg_event_sink),
        estimator,
    ));

    // Pod manager — anchor the capability checker to BOTH the system OCAP
    // authority (pre-registration pod tokens) and the A2A root (post-registration
    // tokens), so legitimate pod tokens verify while forged tokens are rejected.
    // Fails the build if the system OCAP key is unavailable (P4 — fail closed).
    let capability_checker = Arc::new(
        hkask_pods::pod::system_capability_checker()
            .map_err(|e| {
                ServiceError::Infra(hkask_types::InfrastructureError::Io(format!(
                    "OCAP authority key unavailable: {e}"
                )))
            })?
            .trust_root(l.a2a_runtime.root_public_key()),
    );

    let mut pods = hkask_pods::pod::ActivePods::new(
        Arc::new(hkask_pods::pod::PodFactory::new(
            Arc::new(hkask_templates::TemplateCrateLoader::from_path(
                std::path::PathBuf::from(&config.template_cache_path),
            )),
            Arc::new(hkask_pods::DenyAllConsent),
            std::path::Path::new(&config.db_path)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf(),
            config.db_provider,
        )),
        Arc::clone(&l.a2a_runtime),
        Arc::clone(&mcp_runtime),
        Arc::clone(&capability_checker),
    );
    if let Some(inf) = l.inference_port.clone() {
        pods = pods.with_inference_port(inf);
    }
    let pod_manager: Arc<hkask_pods::pod::ActivePods> = Arc::new(pods);

    // Thin pod backup: iterate pods, snapshot each directory via GixCasAdapter.
    {
        let adapter = Arc::clone(&l.pod_backup_adapter);
        let pm = Arc::clone(&pod_manager);
        tokio::spawn(async move {
            super::loops::pod_backup_daemon(adapter, pm).await;
        });
    }

    // Start CuratorPod + CuratorSync (semantic aggregation loop).
    let (curator_ready_tx, curator_ready_rx) = tokio::sync::oneshot::channel();
    let curator_pm = Arc::clone(&pod_manager);
    let curator_data_dir = std::path::Path::new(&config.db_path)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    tokio::spawn(async move {
        match curator_pm.ensure_curator(curator_data_dir).await {
            Ok(Some(_)) => {
                tracing::info!(target: "hkask.startup", "CuratorPod activated and CuratorSync running");
                let _ = curator_ready_tx.send(());
            }
            Ok(None) => {
                tracing::info!(target: "hkask.startup", "CuratorPod already active");
                let _ = curator_ready_tx.send(());
            }
            Err(e) => {
                tracing::error!(target: "hkask.startup", error = %e, "Failed to start CuratorPod");
            }
        }
    });

    // Daemon handler + listener (skip socket in test mode)
    let daemon_handler = Arc::new(hkask_services_runtime::ServiceDaemonHandler::new(
        Arc::clone(&pod_manager),
        Arc::clone(&f.user_store),
        Some(Arc::clone(&f.ledger_runtime)),
        l.inference_port.clone(),
    ));
    if !config.in_memory {
        let mut daemon_listener = hkask_mcp_server::daemon::DaemonListener::new();
        daemon_listener.bind().await.map_err(|e| {
            ServiceError::Infra(hkask_types::InfrastructureError::Io(format!(
                "Failed to bind daemon socket: {}",
                e
            )))
        })?;
        let serve_handler = Arc::clone(&daemon_handler);
        tokio::spawn(async move {
            if let Err(e) = daemon_listener.serve(serve_handler).await {
                tracing::error!(
                    target: "hkask.daemon",
                    error = %e,
                    "Daemon listener serve loop exited with error"
                );
            }
        });
    }

    Ok(McpPods {
        mcp_runtime,
        pod_manager,
        capability_checker,
        daemon_handler,
        energy_estimator,
        tool_stats,
        curator_ready: curator_ready_rx,
    })
}

pub(super) async fn wire_manifest_executor(
    loops: &LoopWiring,
    mcp_runtime: &Arc<McpRuntime>,
    config: &ServiceConfig,
) -> Result<(), ServiceError> {
    if let Some(inference_port) = loops.inference_port.clone() {
        let executor = Arc::new(hkask_templates::ManifestExecutor::new(
            inference_port,
            mcp_runtime.clone() as Arc<dyn hkask_capability::ToolPort>,
            hkask_types::LLMParameters::default(),
            config.a2a_secret.clone(),
        ));
        loops.curator_context.set_manifest_executor(executor).await;
        tracing::info!(target: "hkask.startup", "ManifestExecutor wired into CuratorContext — template-driven metacognition enabled");
    }
    loops.validate().await
}
