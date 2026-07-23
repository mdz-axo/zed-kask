//! Energy estimator trait — used by McpRuntime to estimate tool gas costs.

use serde_json::Value;

/// Estimate the energy cost of a tool invocation before it happens.
pub trait EnergyEstimator: Send + Sync {
    fn estimate_cost(&self, server: &str, tool: &str, args: &Value) -> u64;
}
