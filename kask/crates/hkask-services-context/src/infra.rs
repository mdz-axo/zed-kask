//! Infrastructure context — inference, memory, MCP, pods, wallet,
//!
//! Extracted from `AgentService` as part of the strangler-fig decomposition.

use hkask_communication::matrix::MatrixTransport;
use hkask_mcp::McpRuntime;
use hkask_memory::{EpisodicStoragePort, SemanticStoragePort};
use hkask_pods::pod::ActivePods;
use hkask_regulation::{SeamSummary, SeamWatcher, WalletGasCalibrator};
use hkask_services_runtime::ServiceDaemonHandler;
use hkask_services_wallet::WalletService;
use hkask_types::InferencePort;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Consolidated infrastructure context — inference, memory, MCP, pods,
pub struct InfraContext {
    pub inference: Option<Arc<dyn InferencePort>>,
    pub episodic: Arc<dyn EpisodicStoragePort>,
    pub semantic: Arc<dyn SemanticStoragePort>,
    pub mcp: Arc<McpRuntime>,
    pub pods: Arc<ActivePods>,
    pub wallet: Option<Arc<WalletService>>,
    pub daemon: Arc<ServiceDaemonHandler>,
    pub matrix: Option<Arc<Mutex<MatrixTransport>>>,
    pub seams: Arc<RwLock<Option<SeamWatcher>>>,
    pub wallet_gas: Option<Arc<WalletGasCalibrator>>,
}

impl InfraContext {
    /// InfraContext constructor — assembles the full infrastructure layer at startup.
    /// The argument count reflects the real architectural wiring; a builder pattern
    /// would add indirection without reducing the essential surface area.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        inference: Option<Arc<dyn InferencePort>>,
        episodic: Arc<dyn EpisodicStoragePort>,
        semantic: Arc<dyn SemanticStoragePort>,
        mcp: Arc<McpRuntime>,
        pods: Arc<ActivePods>,
        wallet: Option<Arc<WalletService>>,
        daemon: Arc<ServiceDaemonHandler>,
        matrix: Option<Arc<Mutex<MatrixTransport>>>,
        seams: Arc<RwLock<Option<SeamWatcher>>>,
        wallet_gas: Option<Arc<WalletGasCalibrator>>,
    ) -> Self {
        Self {
            inference,
            episodic,
            semantic,
            mcp,
            pods,
            wallet,
            daemon,
            matrix,
            seams,
            wallet_gas,
        }
    }

    /// Fetch seam watcher summary, if available.
    #[must_use]
    pub async fn seam_summary(&self) -> Option<SeamSummary> {
        let guard = self.seams.read().await;
        guard.as_ref().map(|w| w.summary())
    }
}
