//! ToolStats — Statistical learning for MCP tool invocations.
//!
//! Implements the principle: tools learn statistically from their use.
//! Each tool accumulates cost observations (LogNormal distribution) and
//! success/failure outcomes, enabling:
//!
//! - **Layer 1 (cost):** Reserve at the 90th percentile instead of a point estimate,
//!   tightening with more observations. Wired into GovernedTool as a distribution-based
//!   override of the EnergyEstimator's point estimate.
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
    /// Settled gas cost observations (gas units charged, not raw resource usage).
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
/// Owned by `RegState`. Called by `GovernedTool` at settle time to record
/// outcomes. Queried by `GovernedTool` at reserve time for distribution-based
/// estimates via `reserve_estimate()`.
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

    /// Record a settled gas cost and success/failure outcome.
    ///
    /// `settled_cost` is the gas units actually charged (the settled value,
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
                ts.successes = val.get("successes").and_then(|s| s.as_u64()).unwrap_or(0);
                ts.failures = val.get("failures").and_then(|f| f.as_u64()).unwrap_or(0);
                state.insert(name.clone(), ts);
            }
        }
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
