//! Regulation context — variety sensing, cybernetic regulation, loop orchestration,
//! event audit trail, and energy estimation.
//!
//! Extracted from `AgentService` as part of the strangler-fig decomposition.

use hkask_pods::loop_system::LoopScheduler;
use hkask_regulation::{CalibratedEnergyEstimator, CyberneticsLoop, RegulationLedger, ToolStats};
use hkask_types::event::{RegulationSink, SpanNamespace};
use hkask_types::regulation::LedgerHealth;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Consolidated Regulation context — variety sensing, cybernetic regulation,
/// loop orchestration, event audit trail, and energy estimation.
pub struct RegulationContext {
    pub runtime: Arc<RwLock<RegulationLedger>>,
    pub cybernetics: Arc<RwLock<CyberneticsLoop>>,
    pub loops: Arc<LoopScheduler>,
    pub events: Arc<dyn RegulationSink>,
    pub energy: Arc<CalibratedEnergyEstimator>,
    /// Statistical learner for per-tool cost distributions and reliability.
    /// Shared with GovernedTool and CyberneticsLoop for closed-loop learning.
    pub tool_stats: Arc<ToolStats>,
}

impl RegulationContext {
    pub fn new(
        runtime: Arc<RwLock<RegulationLedger>>,
        cybernetics: Arc<RwLock<CyberneticsLoop>>,
        loops: Arc<LoopScheduler>,
        events: Arc<dyn RegulationSink>,
        energy: Arc<CalibratedEnergyEstimator>,
        tool_stats: Arc<ToolStats>,
    ) -> Self {
        Self {
            runtime,
            cybernetics,
            loops,
            events,
            energy,
            tool_stats,
        }
    }

    /// Read the current Regulation health snapshot.
    ///
    /// Acquires a read lock on the runtime and returns the health status.
    /// This is the canonical access path — replaces the pattern
    /// `ledger().runtime.read().await.health().await`.
    #[must_use]
    pub async fn health(&self) -> LedgerHealth {
        self.runtime.read().await.health().await
    }

    /// Read current variety counters across all monitored domains.
    ///
    /// Acquires a read lock on the runtime and returns per-namespace
    /// variety data. The returned map keys are namespace strings.
    #[must_use]
    pub async fn variety(&self) -> HashMap<SpanNamespace, u64> {
        self.runtime.read().await.variety().await
    }
}
