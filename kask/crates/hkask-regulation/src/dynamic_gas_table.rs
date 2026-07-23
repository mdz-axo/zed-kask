//! DynamicGasTable — Per-server gas cost calibration from Regulation event observations.
//!
//! Closes the Good Regulator feedback loop (P9): observes `reg.gas.settled`
//! spans via `GasReport`, compares actual vs estimated gas costs per server, and
//! adjusts hardcoded `TableEnergyEstimator` costs via exponential moving average (EMA).
//!
//! # Feedback Loop (P9 — Homeostatic Self-Regulation)
//!
//! 1. **Observe**: `GasReport::query_all_agents()` reads settled gas events from the event store
//! 2. **Compare**: Actual gas consumed vs estimated gas per server yields a consumption ratio
//! 3. **Adjust**: EMA-smoothed ratios exceeding ±20% tolerance trigger per-server cost calibration
//!
//! # Design Decisions
//!
//! - **Self-contained**: Does not modify `TableEnergyEstimator` — exports a calibrated table
//!   that callers feed into the estimator at construction time.
//! - **EMA alpha = 0.1**: Each observation contributes 10% to the moving average, matching
//!   `WalletEnergyEstimator`'s calibration smoothing.
//! - **Tolerance = ±20%**: Servers whose actual/estimated ratio deviates beyond 0.8–1.2
//!   are flagged for recalibration.
//! - **Floor at 1**: Gas costs must remain positive (no zero-cost tools).
//!
//! # Contracts
//!
//! REQ: P9-reg-dynamic-gas-table-core — Property-based test: feed observations, verify EMA convergence.
//! REQ: P9-reg-dynamic-gas-table-obs — Tracer bullet: single observation initializes EMA per server.
//! REQ: P9-reg-dynamic-gas-table-integration — Integration: calibrated table replaces hardcoded `TableEnergyEstimator` costs.

use std::collections::{HashMap, HashSet};

/// Default EMA alpha for calibration smoothing.
/// Each observation contributes 10% to the moving average.
const DEFAULT_EMA_ALPHA: f64 = 0.1;

/// Default tolerance for cost ratio deviation (±20%).
const DEFAULT_TOLERANCE: f64 = 0.2;

/// Per-server dynamic gas cost calibration table.
///
/// # Contract
/// expect: "I can calibrate per-server gas costs from real observations using exponential moving averages"
/// pre:  `server_costs` contains known servers with initialized costs
/// post: after `calibrate()`, costs reflect EMA-smoothed actual/estimated ratios
///
/// # Properties
/// - Each server has an EMA of its actual/estimated gas ratio
/// - Ratios within ±20% tolerance do not trigger recalibration
/// - Gas costs are floored at 1 (no zero-cost tools)
/// - Unobserved servers retain their initial cost
///
/// # Public Surface (≤7 items — deep-module discipline)
/// - `DynamicGasTable` (struct)
/// - `new()` — construct from default gas table
/// - `record_observation()` — feed a single server observation
/// - `calibrate()` — apply EMA to all servers with observations
/// - `report_table()` — export current calibrated server costs
/// - `current_ratios()` — export per-server EMA ratios for diagnostics
pub struct DynamicGasTable {
    /// Per-server gas cost estimates (initially from `TableEnergyEstimator::default_gas_table()`).
    server_costs: HashMap<String, u64>,
    /// Per-server EMA: actual_gas / estimated_gas. None if no observations yet.
    ema_ratios: HashMap<String, f64>,
    /// Number of observations per server (for debugging/confidence).
    observation_counts: HashMap<String, u64>,
    /// Servers that have received at least one observation since the last `calibrate()`.
    /// Only these servers are considered on the next calibration pass, preventing
    /// already-applied EMA ratios from being re-applied to current costs.
    observed_since_last_calibrate: HashSet<String>,
    /// EMA smoothing factor.
    ema_alpha: f64,
    /// Tolerance band for triggering recalibration.
    tolerance: f64,
}

impl DynamicGasTable {
    /// Create a new DynamicGasTable with the default gas cost table.
    ///
    /// expect: "I can run a calibration pass that adjusts server costs when EMA ratios exceed tolerance"
    /// expect: "I can create a dynamic gas table initialized from the default server cost table"
    /// post: returns DynamicGasTable with default server costs and no observations
    pub fn new() -> Self {
        let server_costs: HashMap<String, u64> = crate::table_energy_estimator::default_gas_table()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        Self {
            server_costs,
            ema_ratios: HashMap::new(),
            observation_counts: HashMap::new(),
            observed_since_last_calibrate: HashSet::new(),
            ema_alpha: DEFAULT_EMA_ALPHA,
            tolerance: DEFAULT_TOLERANCE,
        }
    }

    /// Record a single gas observation for a server.
    ///
    /// Feeds the observed actual_gas / estimated_gas ratio into the per-server EMA.
    /// The ratio is clamped to [0.1, 10.0] to prevent extreme outliers from
    /// destabilizing the EMA.
    ///
    /// expect: "I can feed a single gas observation into the table to initialize or update the EMA per server"
    /// pre:  estimated_gas > 0 (no division by zero)
    /// post: ema_ratios\[server\] updated with EMA of actual/estimated ratio
    /// post: observation_counts\[server\] incremented
    pub fn record_observation(&mut self, server: &str, estimated_gas: u64, actual_gas: u64) {
        let ratio = actual_gas as f64 / estimated_gas.max(1) as f64;
        // Clamp to [0.1, 10.0] to prevent extreme outliers destabilizing EMA
        let ratio = ratio.clamp(0.1, 10.0);
        let server_key = server.to_string();

        // Update EMA: first observation initializes, subsequent smooth
        let new_ema = match self.ema_ratios.get(&server_key) {
            Some(current) => self.ema_alpha * ratio + (1.0 - self.ema_alpha) * current,
            None => ratio, // first observation initializes the EMA
        };
        self.ema_ratios.insert(server_key.clone(), new_ema);
        self.observed_since_last_calibrate
            .insert(server_key.clone());

        // Increment observation count
        let count = self.observation_counts.entry(server_key).or_insert(0);
        *count += 1;
    }

    /// Calibrate per-server costs based on observed actual/estimated ratios.
    ///
    /// For each server with **new** observations since the last `calibrate()`,
    /// checks if the EMA ratio exceeds tolerance. If the ratio is outside
    /// [1.0 - tolerance, 1.0 + tolerance], the server cost is adjusted:
    /// `new_cost = old_cost * ratio`, floored at 1.
    ///
    /// Servers with no new observations since the last calibration are skipped,
    /// preventing already-applied EMA ratios from being repeatedly re-applied.
    ///
    /// expect: "I can run a calibration pass that adjusts server costs when EMA ratios exceed tolerance"
    /// post: server_costs\[server\] is updated if its EMA ratio exceeds tolerance
    /// post: returns the number of servers whose costs were adjusted
    ///
    /// # Returns
    /// Number of servers whose costs were adjusted.
    #[must_use]
    pub fn calibrate(&mut self) -> usize {
        let servers: Vec<String> = self.observed_since_last_calibrate.iter().cloned().collect();
        self.observed_since_last_calibrate.clear();

        let mut adjusted = 0;
        for server in servers {
            if let Some(ema) = self.ema_ratios.get(&server) {
                // Check if ratio exceeds tolerance band
                if (*ema - 1.0).abs() > self.tolerance {
                    let old_cost = self.server_costs.get(&server).copied().unwrap_or(10);
                    let new_cost = (old_cost as f64 * *ema) as u64;
                    let floored = new_cost.max(1); // floor at 1
                    if floored != old_cost {
                        self.server_costs.insert(server.clone(), floored);
                        adjusted += 1;
                    }
                }
            }
        }
        adjusted
    }

    /// Export the current calibrated per-server cost table.
    ///
    /// Returns a snapshot of `server_costs` suitable for constructing a
    /// `TableEnergyEstimator` or feeding into `CompositeEnergyEstimator`.
    ///
    /// expect: "I can run a calibration pass that adjusts server costs when EMA ratios exceed tolerance"
    /// expect: "I can export the calibrated server cost table for estimator construction"
    /// post: returns a Hash`Map<String, u64>` of server → cost mappings
    #[must_use]
    pub fn report_table(&self) -> HashMap<String, u64> {
        self.server_costs.clone()
    }

    /// Export per-server EMA ratios for diagnostics.
    ///
    /// Returns (server_name → current_ema_ratio) for all servers with observations.
    /// Unobserved servers are omitted.
    ///
    /// expect: "I can query per-server EMA ratios for diagnostics and monitoring"
    /// post: returns a Hash`Map<String, f64>` of server → EMA ratio mappings
    #[must_use]
    pub fn current_ratios(&self) -> HashMap<String, f64> {
        self.ema_ratios.clone()
    }

    /// Number of observations accumulated for a specific server.
    ///
    /// Returns 0 if the server has never been observed.
    ///
    /// expect: "I can query the observation count for a server to assess calibration confidence"
    /// post: returns the count of recorded observations for `server`, or 0 if unobserved
    #[must_use]
    pub fn observation_count(&self, server: &str) -> u64 {
        self.observation_counts.get(server).copied().unwrap_or(0)
    }
}

impl Default for DynamicGasTable {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn first_observation_initializes_ema() {
        let mut table = DynamicGasTable::new();
        assert!(table.current_ratios().is_empty());

        // First observation: estimated 10, actual 15 → ratio 1.5
        table.record_observation("hkask-mcp-condenser", 10, 15);
        let ratios = table.current_ratios();
        assert_eq!(ratios["hkask-mcp-condenser"], 1.5);
        assert_eq!(table.observation_count("hkask-mcp-condenser"), 1);
    }

    #[test]
    fn ema_smooths_multiple_observations() {
        let mut table = DynamicGasTable::new();

        // First: ratio 2.0 → EMA = 2.0
        table.record_observation("hkask-mcp-research", 100, 200);
        assert!((table.current_ratios()["hkask-mcp-research"] - 2.0).abs() < 0.001);

        // Second: ratio 1.0 → EMA = 0.1*1.0 + 0.9*2.0 = 1.9
        table.record_observation("hkask-mcp-research", 100, 100);
        let expected_ema = 0.1 * 1.0 + 0.9 * 2.0; // 1.9
        assert!((table.current_ratios()["hkask-mcp-research"] - expected_ema).abs() < 0.001);
    }

    #[test]
    fn within_tolerance_no_adjustment() {
        let mut table = DynamicGasTable::new();
        // Ratio 1.1 is within ±20% tolerance
        table.record_observation("hkask-mcp-memory", 100, 110);
        let adjusted = table.calibrate();
        assert_eq!(adjusted, 0, "ratio 1.1 is within 20% tolerance");
    }

    #[test]
    fn exceeds_tolerance_triggers_adjustment() {
        let mut table = DynamicGasTable::new();
        // Ratio 2.0 > 1.2 tolerance
        table.record_observation("hkask-mcp-media", 100, 200);
        let adjusted = table.calibrate();
        assert_eq!(adjusted, 1, "ratio 2.0 exceeds ±20% tolerance");

        let reports = table.report_table();
        // media cost was 100, EMA ratio = 2.0, new cost = 100 * 2.0 = 200
        assert_eq!(reports["hkask-mcp-media"], 200);
    }

    #[test]
    fn cost_floored_at_one() {
        let mut table = DynamicGasTable::new();
        // memory cost is 5, ratio 0.1 → new cost = 5 * 0.1 = 0.5, floored at 1
        table.record_observation("hkask-mcp-memory", 5, 0);
        let adjusted = table.calibrate();
        assert_eq!(adjusted, 1);
        let reports = table.report_table();
        assert_eq!(reports["hkask-mcp-memory"], 1, "cost floored at 1");
    }

    #[test]
    fn unobserved_servers_retain_initial() {
        let table = DynamicGasTable::new();
        let reports = table.report_table();
        // hkask-mcp-memory should still have its default cost of 5
        assert_eq!(reports["hkask-mcp-memory"], 5);
    }

    #[test]
    fn calibrate_does_not_reapply_without_new_observations() {
        let mut table = DynamicGasTable::new();
        table.record_observation("hkask-mcp-media", 100, 200);
        assert_eq!(table.calibrate(), 1);
        assert_eq!(table.report_table()["hkask-mcp-media"], 200);

        // No new observations — calibrate should not re-adjust.
        assert_eq!(table.calibrate(), 0);
        assert_eq!(table.report_table()["hkask-mcp-media"], 200);
    }

    #[test]
    fn calibrate_readjusts_after_new_observation() {
        let mut table = DynamicGasTable::new();
        table.record_observation("hkask-mcp-media", 100, 200);
        assert_eq!(table.calibrate(), 1);
        assert_eq!(table.report_table()["hkask-mcp-media"], 200);

        // New observation at the updated estimate with actual still high.
        table.record_observation("hkask-mcp-media", 200, 400);
        assert_eq!(table.calibrate(), 1);
        assert_eq!(table.report_table()["hkask-mcp-media"], 400);
    }

    proptest! {
        fn costs_converge_after_multiple_observations(
            obs_count in 2usize..50usize,
        ) {
            let mut table = DynamicGasTable::new();
            // Feed constant ratio of 2.0 for obs_count times
            for _ in 0..obs_count {
                table.record_observation("hkask-mcp-research", 100, 200);
            }
            // After many observations, EMA → 2.0 (converges)
            // First: 2.0. After: 0.1*2.0 + 0.9*2.0 = 2.0 (stays 2.0 with constant observations)
            let ratio = table.current_ratios()["hkask-mcp-research"];
            prop_assert!((ratio - 2.0).abs() < 0.01);
            prop_assert_eq!(table.observation_count("hkask-mcp-research"), obs_count as u64);
        }
    }
}
