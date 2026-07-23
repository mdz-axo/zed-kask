//! CompositeEnergyEstimator — Routes inference tools to InferenceEnergyEstimator,
//! all other tools to TableEnergyEstimator.

use crate::dynamic_gas_table::DynamicGasTable;
use crate::energy_estimator::EnergyEstimator;
use crate::inference_estimator::InferenceEnergyEstimator;
use crate::table_energy_estimator::TableEnergyEstimator;
use serde_json::Value;

/// Composite gas estimator that routes inference tools to InferenceEnergyEstimator
/// and all other tools to TableEnergyEstimator.
///
/// \[NORMATIVE\] This is the production estimator — it should be the default for all (P9 — Homeostatic Self-Regulation).
/// GovernedTool instances. Inference calls use token-based estimation;
/// everything else uses the per-server table.
pub struct CompositeEnergyEstimator {
    inference: InferenceEnergyEstimator,
    table: TableEnergyEstimator,
}

impl CompositeEnergyEstimator {
    /// Create a new CompositeEnergyEstimator with default table costs.
    ///
    /// expect: "The system creates a composite estimator that routes inference and table costs"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — composite estimator enables feedback loops
    /// \[P5\] Constraining: Essentialism — minimal constructor, empty estimators
    /// post: returns CompositeEnergyEstimator with empty estimators
    pub fn new() -> Self {
        Self {
            inference: InferenceEnergyEstimator,
            table: TableEnergyEstimator::new(),
        }
    }

    /// Create a CompositeEnergyEstimator calibrated from a DynamicGasTable.
    ///
    /// Non-inference server costs are taken from `table.report_table()`;
    /// inference routing still uses `InferenceEnergyEstimator`.
    ///
    /// expect: "I can build a calibrated estimator from a dynamic gas table so per-server costs reflect observed usage"
    /// pre:  table was calibrated (or default) via DynamicGasTable::calibrate()
    /// post: estimate_cost(server, ...) uses table.report_table()\[server\] for non-inference servers
    pub fn from_dynamic_table(table: &DynamicGasTable) -> Self {
        Self {
            inference: InferenceEnergyEstimator,
            table: TableEnergyEstimator::with_server_costs(table.report_table()),
        }
    }

    /// The inference routing key used for energy estimation.
    ///
    /// Routes through `InferencePort` directly, not MCP dispatch.
    pub const INFERENCE_SERVER: &'static str = "inference";
}

impl Default for CompositeEnergyEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl EnergyEstimator for CompositeEnergyEstimator {
    fn estimate_cost(&self, server: &str, tool: &str, args: &Value) -> u64 {
        if server == Self::INFERENCE_SERVER {
            self.inference.estimate_cost(server, tool, args)
        } else {
            self.table.estimate_cost(server, tool, args)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_dynamic_table_uses_calibrated_server_cost() {
        let mut table = DynamicGasTable::new();
        // Ratio 2.0 for hkask-mcp-media triggers cost doubling (100 -> 200)
        table.record_observation("hkask-mcp-media", 100, 200);
        assert_eq!(table.calibrate(), 1);

        let estimator = CompositeEnergyEstimator::from_dynamic_table(&table);
        let cost = estimator.estimate_cost("hkask-mcp-media", "search", &serde_json::json!({}));
        assert_eq!(
            cost, 200,
            "calibrated cost should replace hardcoded default"
        );
    }

    #[test]
    fn from_dynamic_table_retains_default_for_unobserved_servers() {
        let table = DynamicGasTable::new();
        let estimator = CompositeEnergyEstimator::from_dynamic_table(&table);
        let cost =
            estimator.estimate_cost("hkask-mcp-memory", "memory_query", &serde_json::json!({}));
        assert_eq!(cost, 5, "unobserved server should retain default cost");
    }

    #[test]
    fn from_dynamic_table_still_routes_inference() {
        let table = DynamicGasTable::new();
        let estimator = CompositeEnergyEstimator::from_dynamic_table(&table);
        let args = serde_json::json!({"prompt": "x", "max_tokens": 100});
        let cost = estimator.estimate_cost(
            CompositeEnergyEstimator::INFERENCE_SERVER,
            "generate",
            &args,
        );
        assert_eq!(cost, 100, "inference cost uses token estimator, not table");
    }

    #[test]
    fn from_dynamic_table_preserves_tool_overrides() {
        let table = DynamicGasTable::new();
        let estimator = CompositeEnergyEstimator::from_dynamic_table(&table);
        let cost = estimator.estimate_cost(
            "hkask-mcp-condenser",
            "condenser_thread_summary",
            &serde_json::json!({}),
        );
        assert_eq!(cost, 25, "per-tool override should be preserved");
    }
}
