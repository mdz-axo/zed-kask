//! McpServerGuard Loop — Proactive MCP server health monitoring (Loop 8)
//!
//! Lives in `hkask-services-context` (not `hkask-regulation`) to avoid a circular
//! dependency — it needs both `McpRuntime` from `hkask-mcp` and the `RegulationLoop`
//! trait from `hkask-regulation`. Both are available here.

use hkask_mcp::McpRuntime;
use hkask_regulation::types::loops::{
    ActionType, Deviation, LoopId, RegulationLoop, RegulatoryAction, RegulatoryActionParams,
    Signal, SignalMetric,
};
use std::sync::Arc;

pub struct McpServerGuardLoop {
    runtime: Arc<McpRuntime>,
}

impl McpServerGuardLoop {
    pub fn new(runtime: Arc<McpRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait]
impl RegulationLoop for McpServerGuardLoop {
    fn id(&self) -> LoopId {
        LoopId::McpServerGuard
    }

    async fn sense(&self) -> Vec<Signal> {
        let total = self.runtime.servers().await.len() as f64;
        if total == 0.0 {
            return Vec::new();
        }
        let alive = self.runtime.connection_count().await as f64;
        let ratio = alive / total;

        vec![Signal::new(
            LoopId::McpServerGuard,
            SignalMetric::McpServerHealth,
            ratio,
            1.0,
        )]
    }

    async fn compare(&self, signals: &[Signal]) -> Vec<Deviation> {
        signals
            .iter()
            .filter(|s| s.metric == SignalMetric::McpServerHealth && s.value < s.set_point)
            .filter_map(Deviation::from_signal)
            .collect()
    }

    async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
        if deviations.is_empty() {
            return Vec::new();
        }

        vec![RegulatoryAction::new(
            LoopId::McpServerGuard,
            ActionType::Notify,
            RegulatoryActionParams::reason("mcp_server_health"),
        )]
    }

    async fn act(&self, _actions: &[RegulatoryAction]) {
        // Read dead server state directly from the runtime instead of
        // extracting it from action data (which was JSON key-sniffing).
        let servers = self.runtime.servers().await;
        let connections = self.runtime.connections().await;
        let dead: Vec<_> = servers
            .iter()
            .filter(|(id, _)| !connections.contains_key(*id))
            .collect();

        if !dead.is_empty() {
            tracing::warn!(
                target = "reg.mcp_server_guard",
                dead_count = dead.len(),
                dead_servers = ?dead.iter().map(|(id, s)| if s.name != **id { &s.name } else { *id }).collect::<Vec<_>>(),
                "MCP servers dead or unreachable — operator restart required"
            );
        }
    }
}
