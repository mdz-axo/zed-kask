//! ToolStats — Statistical learning for MCP tool invocations.
//!
//! Implements the principle: tools learn statistically from their use.
//! Each tool accumulates cost observations (LogNormal distribution) and
//! success/failure outcomes, enabling:
//!
//! - **Layer 1 (cost):** Reserve at the 90th percentile instead of a point estimate,
//!   tightening with more observations. Intended as a distribution-based cost
//!   signal for the governed tool-call membrane (the per-agent call cap charges a
//!   flat one call per invocation today; this layer is the seam for a future
//!   calibrated per-tool cost).
//! - **Layer 2 (reliability):** Pre-escalate when success probability drops below
//!   a threshold, detecting degrading tools before they fail.
//! - **Layer 3 (auto-calibration):** Cost data feeds back into the estimator —
//!   if the distribution's p90 is consistently lower than the point estimate,
//!   reserves tighten automatically.
//!
//! ## Distribution Choice
//!
//! - **LogNormal for cost** — tool costs are positive and right-skewed.
//!   Fit by method of moments (population variance) on log-transformed observations.
//! - **Reliability** — Beta(α = successes + 1, β = failures + 1) conjugate prior
//!   with Laplace smoothing. Computed inline in `reliability_alerts()`.

use std::collections::{HashMap, VecDeque};
use tokio::sync::RwLock;

/// Maximum cost observations retained per tool for distribution fitting.
const MAX_COST_OBSERVATIONS: usize = 200;

/// Minimum observations before a distribution fit is considered reliable.
const MIN_OBSERVATIONS_FOR_FIT: usize = 10;

/// Default success probability threshold for reliability alerts.
pub const DEFAULT_RELIABILITY_THRESHOLD: f64 = 0.80;

/// Statistical state for a single MCP tool.
#[derive(Debug, Clone, Default)]
pub(crate) struct ToolState {
    /// Settled cost observations (units charged, not raw resource usage).
    costs: VecDeque<f64>,
    /// Count of successful invocations.
    successes: u64,
    /// Count of failed invocations.
    failures: u64,
}

/// A fitted cost distribution for reserve estimation.
#[derive(Debug, Clone)]
pub struct CostDistribution {
    /// 90th percentile — recommended reserve point.
    pub p90: f64,
    /// Number of observations used for the fit.
    pub n_observations: usize,
    /// Whether the fit is reliable (≥ MIN_OBSERVATIONS_FOR_FIT).
    pub reliable: bool,
}

/// Per-tool reliability alert, emitted when success probability drops.
#[derive(Debug, Clone)]
pub struct ToolReliabilityAlert {
    pub tool_name: String,
    pub success_probability: f64,
    pub threshold: f64,
    pub n_observations: u64,
}

/// Thread-safe statistical learner for all MCP tools.
///
/// Owned by `RegState`. Called by `McpRuntime::invoke` (via `CyberneticsLoop::charge_call`)
/// after each governed tool call to record outcomes. Queried by `McpRuntime::invoke`
/// for distribution-based estimates via `reserve_estimate()`.
pub struct ToolStats {
    state: RwLock<HashMap<String, ToolState>>,
    reliability_threshold: f64,
}

impl ToolStats {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(HashMap::new()),
            reliability_threshold: DEFAULT_RELIABILITY_THRESHOLD,
        }
    }

    /// Record a settled cost and success/failure outcome.
    ///
    /// `settled_cost` is the units actually charged (the settled value,
    /// which may differ from the initial reserve estimate). Guarded to ≥1
    /// to prevent `ln(0) = -inf` from degenerating the distribution.
    pub async fn record(&self, tool_name: &str, settled_cost: u64, success: bool) {
        let mut state = self.state.write().await;
        let entry = state.entry(tool_name.to_string()).or_default();
        entry.costs.push_back((settled_cost.max(1)) as f64);
        if entry.costs.len() > MAX_COST_OBSERVATIONS {
            entry.costs.pop_front();
        }
        if success {
            entry.successes += 1;
        } else {
            entry.failures += 1;
        }
    }

    /// Return the recommended reserve amount for a tool invocation.
    ///
    /// Uses the 90th percentile of the fitted LogNormal cost distribution
    /// when enough observations exist (≥ MIN_OBSERVATIONS_FOR_FIT).
    /// Falls back to raw mean when data is scarce.
    /// Returns `None` when no observations exist — caller should use its point estimate.
    pub async fn reserve_estimate(&self, tool_name: &str) -> Option<u64> {
        let state = self.state.read().await;
        let entry = state.get(tool_name)?;
        // Build distribution inside the lock — only clone costs for the computation.
        let dist = CostDistribution::from_state(entry);
        let result = if dist.reliable {
            Some(dist.p90.ceil() as u64)
        } else if dist.n_observations > 0 {
            let mean = entry.costs.iter().sum::<f64>() / entry.costs.len() as f64;
            Some(mean.ceil() as u64)
        } else {
            None
        };
        drop(state);
        result
    }

    /// Check all tracked tools and return reliability alerts for degraded tools.
    ///
    /// A tool is degraded when its Beta posterior success probability
    /// falls below `reliability_threshold`.
    pub async fn reliability_alerts(&self) -> Vec<ToolReliabilityAlert> {
        let state = self.state.read().await;
        let mut alerts = Vec::new();
        for (tool_name, entry) in state.iter() {
            let n = entry.successes + entry.failures;
            if n == 0 {
                continue;
            }
            let alpha = entry.successes as f64 + 1.0;
            let beta = entry.failures as f64 + 1.0;
            let prob = alpha / (alpha + beta);
            if prob < self.reliability_threshold {
                alerts.push(ToolReliabilityAlert {
                    tool_name: tool_name.clone(),
                    success_probability: prob,
                    threshold: self.reliability_threshold,
                    n_observations: n,
                });
            }
        }
        alerts
    }

    /// Serialize tool stats state for persistence across restarts.
    /// Returns a JSON value suitable for inclusion in the budget persistence wrapper.
    pub async fn save_state(&self) -> serde_json::Value {
        let state = self.state.read().await;
        let tools: serde_json::Map<String, serde_json::Value> = state
            .iter()
            .map(|(name, ts)| {
                let costs: Vec<f64> = ts.costs.iter().copied().collect();
                (
                    name.clone(),
                    serde_json::json!({
                        "costs": costs,
                        "successes": ts.successes,
                        "failures": ts.failures,
                    }),
                )
            })
            .collect();
        serde_json::Value::Object(tools)
    }

    /// Restore tool stats state from a previously saved JSON value.
    ///
    /// Missing or malformed scalar fields (`successes`, `failures`) are
    /// tolerated by falling back to 0, but emit a `tracing::warn!` naming the
    /// field, the expected type, and the actual value. This lets an operator
    /// reading logs distinguish a genuinely-zero tool (no recorded outcomes)
    /// from a corrupted state file (field present but wrong type). Without the
    /// warning, `unwrap_or(0)` masks corruption as a measured zero — a broken
    /// feedback loop, since the reliability layer reads these counts as
    /// measurements.
    pub async fn load_state(&self, saved: &serde_json::Value) {
        let mut state = self.state.write().await;
        if let Some(obj) = saved.as_object() {
            for (name, val) in obj {
                let mut ts = ToolState::default();
                if let Some(costs) = val.get("costs").and_then(|c| c.as_array()) {
                    for c in costs {
                        if let Some(v) = c.as_f64() {
                            ts.costs.push_back(v);
                        }
                    }
                }
                ts.successes = read_count_field(val, name, "successes");
                ts.failures = read_count_field(val, name, "failures");
                state.insert(name.clone(), ts);
            }
        }
    }
}

/// Read a `u64` outcome count (`successes`/`failures`) from a saved tool-state
/// JSON object.
///
/// Tolerates a missing field (treats as 0) but emits a `tracing::warn!` when
/// the field is present but not a `u64`, naming the tool, the field, the
/// expected type, and the actual value. This distinguishes a measured zero
/// (tool with no recorded outcomes) from a malformed state file (field
/// present but wrong type) so an operator can detect corruption rather than
/// silently masking it as `0`.
fn read_count_field(val: &serde_json::Value, tool_name: &str, field: &str) -> u64 {
    match val.get(field) {
        // Missing field: a tool with no recorded outcomes legitimately has no
        // count — no warning, fall back to 0.
        None => 0,
        Some(v) => match v.as_u64() {
            Some(count) => count,
            None => {
                tracing::warn!(
                    tool = tool_name,
                    field,
                    expected = "u64",
                    actual = %v,
                    "tool_stats: malformed state field, falling back to 0"
                );
                0
            }
        },
    }
}

impl Default for ToolStats {
    fn default() -> Self {
        Self::new()
    }
}

impl CostDistribution {
    /// Build a cost distribution from tool state. Called internally by `reserve_estimate`.
    pub(crate) fn from_state(state: &ToolState) -> Self {
        let n = state.costs.len();
        if n < MIN_OBSERVATIONS_FOR_FIT {
            return Self {
                p90: 0.0,
                n_observations: n,
                reliable: false,
            };
        }
        let log_costs: Vec<f64> = state.costs.iter().map(|c| c.ln()).collect();
        let n_f = n as f64;
        let mu: f64 = log_costs.iter().sum::<f64>() / n_f;
        let variance: f64 = log_costs.iter().map(|lc| (lc - mu).powi(2)).sum::<f64>() / n_f;
        let sigma = variance.sqrt().max(0.01);
        let p90 = (mu + 1.28155 * sigma).exp();
        Self {
            p90,
            n_observations: n,
            reliable: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn save_load_roundtrips_counts() {
        let stats = ToolStats::new();
        stats.record("t1", 10, true).await;
        stats.record("t1", 12, false).await;
        let saved = stats.save_state().await;

        let restored = ToolStats::new();
        restored.load_state(&saved).await;
        // Reserve estimate round-trips (exercises the reloaded costs).
        let est = restored.reserve_estimate("t1").await;
        assert!(est.is_some());
        // Reliability alert fires for the failed tool (success prob < threshold).
        let alerts = restored.reliability_alerts().await;
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].tool_name, "t1");
        assert_eq!(alerts[0].n_observations, 2);
    }

    // Pins the M8 degradation: a missing `successes`/`failures` field is a
    // measured zero (no warn), while a present-but-wrong-type field falls back
    // to 0 (with a warn) instead of `unwrap_or(0)` silently masking corruption.
    // See .rules: "unwrap_or(0) on regulation-loop sense inputs is a broken
    // feedback loop".
    #[tokio::test]
    async fn load_state_missing_field_falls_back_to_zero() {
        let saved = serde_json::json!({
            "t_missing": { "costs": [5.0], "successes": 3 }
            // `failures` absent → 0
        });
        let stats = ToolStats::new();
        stats.load_state(&saved).await;
        // No alerts: 3 successes / 0 failures → prob 1.0, above threshold.
        assert!(stats.reliability_alerts().await.is_empty());
    }

    #[tokio::test]
    async fn load_state_malformed_field_falls_back_to_zero() {
        let saved = serde_json::json!({
            "t_bad": {
                "costs": [5.0],
                "successes": "not a number",
                "failures": 4
            }
        });
        let stats = ToolStats::new();
        stats.load_state(&saved).await;
        // `successes` malformed → 0; 0 successes / 4 failures → prob 0.2 < 0.8.
        let alerts = stats.reliability_alerts().await;
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].tool_name, "t_bad");
        assert_eq!(alerts[0].n_observations, 4);
    }
}
