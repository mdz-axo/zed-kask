//! TableEnergyEstimator — per-server energy cost table
//
//! Each (server, tool) pair maps to a flat energy cost. Inference uses token-based
//! `InferenceEnergyEstimator` instead (cost 0 in table signals this).
//
//! | Tier | Servers | Cost | Rationale |
//! |------|---------|------|----------|
//! | Memory | memory | 5 | Internal storage read |
//! | Local I/O | spec | 5 | No network |
//! | Moderate | condenser, docproc, training | 10-15 | Compute + I/O |
//! | Moderate+Network | condenser (thread_summary) | 25 | HTTP call to inference engine |
//! | External | web, fmp, rss-reader | 20-50 | Network I/O |
//! | Heavy | fal | 100 | GPU compute |
//! | Inference | (routed via InferenceEnergyEstimator) | 0 | Token-based estimator |
//
//! Unknown servers default to 10. Use `CompositeEnergyEstimator` for production.

use crate::energy_estimator::EnergyEstimator;
use serde_json::Value;
use std::collections::HashMap;

/// Default energy costs per MCP server.
///
/// These are intentionally conservative — they prevent infinite loops
/// while being simple to understand and calibrate.
pub(crate) fn default_gas_table() -> HashMap<&'static str, u64> {
    let mut table = HashMap::new();
    // Memory — internal storage read
    table.insert("hkask-mcp-memory", 5);

    // Moderate — compute + local I/O
    table.insert("hkask-mcp-condenser", 10);
    table.insert("hkask-mcp-docproc", 15);
    table.insert("hkask-mcp-training", 10);

    // External API tools — expensive
    table.insert("hkask-mcp-research", 50);
    table.insert("hkask-mcp-companies", 40);
    table.insert("hkask-mcp-communication", 50);
    table.insert("hkask-mcp-media", 100);

    table.insert("hkask-mcp-replica", 30);

    // Inference is routed through InferenceEnergyEstimator directly.
    table.insert("inference", 0); // Overridden by InferenceEnergyEstimator

    table
}

/// Table-based gas estimator with configurable per-server costs.
///
/// # Gas Cost Philosophy
///
/// Gas units are dimensionless — they represent computational cost on a shared
/// scale, analogous to Ethereum gas. The principle is: cheap operations cost
/// little, expensive operations cost much. This prevents runaway agents while
/// keeping the implementation minimal.
///
/// ## Cost Tiers
///
/// | Tier | Servers | Cost Range | Rationale |
/// |------|---------|------------|----------|
/// | Memory | memory | 5 | Internal storage read |
/// | Local I/O | spec | 5 | Local I/O, no network |
/// | Moderate | condenser, docproc, training | 10-15 | Compute + local I/O |
/// | Moderate+Network | condenser (thread_summary) | 25 | HTTP call to inference engine |
/// | External API | web, fmp, rss-reader | 20-50 | Network I/O, rate-limited |
/// | Heavy external | fal | 100 | GPU compute, expensive |
/// | Inference | (routed via InferenceEnergyEstimator) | 0 (table) | Handled by `InferenceEnergyEstimator` |
///
/// Inference uses a token-based cost model (`InferenceEnergyEstimator`):
/// `prompt_chars / 4 + max_tokens`. This reflects that LLM compute scales
/// with token count, not with a flat per-call cost.
///
/// Unknown servers default to 10 (moderate — conservative middle ground).
///
/// For production, use `CompositeEnergyEstimator` which routes inference to
/// `InferenceEnergyEstimator` and all other tools to this table.
///
/// # Lookup Priority
///
/// Looks up energy cost by server name. If the server has a specific cost,
/// uses that. If not found, falls back to the `default_cost`.
///
/// For tools within a server, you can optionally provide per-tool costs
/// via `with_tool_cost()`. If no per-tool cost is found, the server cost
/// is used.
pub(crate) struct TableEnergyEstimator {
    /// Per-server energy costs.
    server_costs: HashMap<String, u64>,
    /// Per-(server, tool) energy costs (overrides server cost).
    tool_costs: HashMap<(String, String), u64>,
    /// Default cost when neither server nor tool cost is found.
    default_cost: u64,
}

impl TableEnergyEstimator {
    /// Create a TableEnergyEstimator with the default gas table.
    pub(crate) fn new() -> Self {
        Self::with_server_costs(
            default_gas_table()
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    /// Create a TableEnergyEstimator with custom per-server costs.
    ///
    /// pre:  `server_costs` contains the desired server → cost mappings
    /// post: per-tool overrides (e.g. condenser_thread_summary) are still applied
    ///       on top of the provided server costs
    pub(crate) fn with_server_costs(server_costs: HashMap<String, u64>) -> Self {
        let mut tool_costs: HashMap<(String, String), u64> = HashMap::new();
        // thread_summary makes an HTTP call to the inference engine — more expensive than local compression
        tool_costs.insert(
            (
                "hkask-mcp-condenser".to_string(),
                "condenser_thread_summary".to_string(),
            ),
            25,
        );
        Self {
            server_costs,
            tool_costs,
            default_cost: 10,
        }
    }

    /// Look up the energy cost for a (server, tool) pair.
    pub(crate) fn lookup(&self, server: &str, tool: &str) -> u64 {
        // Per-tool cost takes priority
        if let Some(cost) = self.tool_costs.get(&(server.to_string(), tool.to_string())) {
            return *cost;
        }
        // Then per-server cost
        if let Some(cost) = self.server_costs.get(server) {
            return *cost;
        }
        // Default
        self.default_cost
    }
}

impl Default for TableEnergyEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl EnergyEstimator for TableEnergyEstimator {
    fn estimate_cost(&self, server: &str, tool: &str, _args: &Value) -> u64 {
        self.lookup(server, tool)
    }
}
