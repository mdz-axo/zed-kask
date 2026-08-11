//! Budget tracking — gas (compute) and rJoule (inference) accounting.
//!
//! Extracted from the executor to collapse the 5-argument budget threading
//! in `execute_select` and unify the duplicated exhaustion/alert logic that
//! appeared twice in the cascade loop (gas post-select + gas end-of-pass)
//! and once for rJoule.
//!
//! # Design
//!
//! `BudgetTracker` is a pure state machine: charge iterations, check
//! exhaustion, fire alerts once per threshold crossing, and snapshot to the
//! `_gas`/`_rjoule` context JSON shape that templates depend on. It has no
//! dependency on `InferencePort`, `ToolPort`, or the executor — it's a leaf
//! module that the executor composes.

use crate::bundle::config::{BundleGasConfig, RjouleConfig};
use serde_json::{Value, json};
use tracing::info;

/// Which budget was exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetExhaustion {
    Gas,
    Rjoule,
}

/// A snapshot of the current budget state, serializable to the `_gas`/`_rjoule`
/// context JSON that templates reference for budget-aware rendering.
#[derive(Debug, Clone)]
pub struct BudgetSnapshot {
    pub gas_used: u64,
    pub gas_cap: u64,
    pub gas_remaining: u64,
    pub gas_cost_per_iteration: u64,
    pub rjoule_used: f64,
    pub rjoule_cap: f64,
    pub rjoule_remaining: f64,
    pub rjoule_enabled: bool,
}

impl BudgetSnapshot {
    /// Serialize as the `_gas` context value (matches the shape templates expect).
    pub fn gas_json(&self) -> Value {
        json!({
            "used": self.gas_used,
            "cap": self.gas_cap,
            "remaining": self.gas_remaining,
            "cost_per_iteration": self.gas_cost_per_iteration,
        })
    }

    /// Serialize as the `_rjoule` context value (matches the shape templates expect).
    pub fn rjoule_json(&self) -> Value {
        json!({
            "used": self.rjoule_used,
            "cap": self.rjoule_cap,
            "remaining": self.rjoule_remaining,
            "enabled": self.rjoule_enabled,
        })
    }
}

/// Tracks gas (compute cycles) and rJoule (inference energy) consumption
/// against declared caps, with once-per-threshold alerting and hard-limit
/// exhaustion checks.
///
/// Constructed from a manifest's `gas:` and `rjoule:` config blocks. The
/// executor charges iterations via `charge_iteration`, checks exhaustion
/// after each charge and at end-of-pass, and snapshots the state into the
/// context map for template awareness.
pub struct BudgetTracker {
    // Gas (compute)
    gas_used: u64,
    gas_cap: u64,
    gas_cost_per_iter: u64,
    gas_alert_threshold: f64,
    gas_hard_limit: bool,
    gas_alerted: bool,
    // rJoule (inference)
    rjoule_used: f64,
    rjoule_cap: f64,
    rjoule_alert_threshold: f64,
    rjoule_hard_limit: bool,
    rjoule_enabled: bool,
    rjoule_alerted: bool,
}

impl BudgetTracker {
    /// Construct from manifest config blocks.
    pub fn new(gas: &BundleGasConfig, rjoule: &RjouleConfig) -> Self {
        let rjoule_cap = rjoule.cap as f64;
        Self {
            gas_used: 0,
            gas_cap: gas.cap as u64,
            gas_cost_per_iter: gas.cost_per_iteration as u64,
            gas_alert_threshold: gas.alert_threshold,
            gas_hard_limit: gas.hard_limit,
            gas_alerted: false,
            rjoule_used: 0.0,
            rjoule_cap,
            rjoule_alert_threshold: rjoule.alert_threshold,
            rjoule_hard_limit: rjoule.hard_limit,
            rjoule_enabled: rjoule_cap > 0.0,
            rjoule_alerted: false,
        }
    }

    /// Charge one iteration of compute gas. Called after each `select` step.
    pub fn charge_iteration(&mut self) {
        self.gas_used = self.gas_used.saturating_add(self.gas_cost_per_iter);
    }

    /// Construct a per-task budget tracker from remaining capacity.
    /// Used by the concurrent wave executor — each concurrent step gets
    /// its own tracker so they don't share mutable state. The caller
    /// merges consumption back into the parent tracker after the wave.
    pub fn from_remaining(gas_remaining: u32, rjoule_remaining: f64) -> Self {
        Self {
            gas_used: 0,
            gas_cap: gas_remaining as u64,
            gas_cost_per_iter: 1,
            gas_alert_threshold: 0.8,
            gas_hard_limit: true,
            gas_alerted: false,
            rjoule_used: 0.0,
            rjoule_cap: rjoule_remaining,
            rjoule_alert_threshold: 0.8,
            rjoule_hard_limit: true,
            rjoule_enabled: rjoule_remaining > 0.0,
            rjoule_alerted: false,
        }
    }

    /// Return the rJoule cost charged by the most recent `charge_rjoule` call.
    /// Used by the concurrent wave executor to merge per-task costs back
    /// into the parent tracker.
    pub fn last_rjoule_cost(&self) -> Option<f64> {
        if self.rjoule_used > 0.0 {
            Some(self.rjoule_used)
        } else {
            None
        }
    }

    /// Charge rJoule for an inference call. The executor calls this with the
    /// inference result's observed USD cost (`InferenceResult.cost_usd`, which
    /// prefers the provider's `market_cost` over `cost`) once per `select` step.
    pub fn charge_rjoule(&mut self, rjoules: f64) {
        self.rjoule_used += rjoules;
    }

    /// Charge gas + rJoule consumed by a sub-cascade (flowdef). The sub-cascade's
    /// budget was capped to the parent's remaining budget, so this is at most
    /// the parent's remaining allocation. Conservative: may over-count, never
    /// under-counts.
    pub fn consume_child(&mut self, gas: u64, rjoule: f64) {
        self.gas_used = self.gas_used.saturating_add(gas);
        self.rjoule_used = (self.rjoule_used + rjoule).max(0.0);
    }

    /// Check whether either budget is exhausted (hard limit hit). Returns the
    /// exhausted budget if so, emits the appropriate `reg.skill.budget.*_exhausted`
    /// span, and fires the alert if the threshold was crossed this check.
    ///
    /// `iteration` is used for span emission.
    pub fn check_exhausted(&mut self, iteration: u32) -> Option<BudgetExhaustion> {
        // Fire alerts first (once per threshold crossing) so the alert span
        // precedes the exhaustion span when both fire on the same check.
        self.fire_alerts();

        if self.gas_hard_limit && self.gas_used >= self.gas_cap {
            info!(
                target: "reg.skill.budget.gas_exhausted",
                iteration = iteration,
                gas_used = self.gas_used,
                gas_cap = self.gas_cap,
                "REG"
            );
            return Some(BudgetExhaustion::Gas);
        }
        if self.rjoule_enabled && self.rjoule_hard_limit && self.rjoule_used >= self.rjoule_cap {
            info!(
                target: "reg.skill.budget.rjoule_exhausted",
                iteration = iteration,
                rjoule_used = self.rjoule_used,
                rjoule_cap = self.rjoule_cap,
                "REG"
            );
            return Some(BudgetExhaustion::Rjoule);
        }
        None
    }

    /// Fire the gas and rJoule alert spans if the threshold was crossed and
    /// not already alerted. Idempotent — only fires once per budget per
    /// tracker lifetime.
    fn fire_alerts(&mut self) {
        if !self.gas_alerted
            && self.gas_cap > 0
            && (self.gas_used as f64 / self.gas_cap as f64) >= self.gas_alert_threshold
        {
            self.gas_alerted = true;
            info!(
                target: "reg.skill.budget.gas_alert",
                gas_used = self.gas_used,
                gas_cap = self.gas_cap,
                pct = (self.gas_used as f64 / self.gas_cap as f64) * 100.0,
                "REG"
            );
        }
        if !self.rjoule_alerted
            && self.rjoule_cap > 0.0
            && (self.rjoule_used / self.rjoule_cap) >= self.rjoule_alert_threshold
        {
            self.rjoule_alerted = true;
            info!(
                target: "reg.skill.budget.rjoule_alert",
                rjoule_used = self.rjoule_used,
                rjoule_cap = self.rjoule_cap,
                pct = (self.rjoule_used / self.rjoule_cap) * 100.0,
                "REG"
            );
        }
    }

    /// Snapshot the current state for context injection.
    pub fn snapshot(&self) -> BudgetSnapshot {
        BudgetSnapshot {
            gas_used: self.gas_used,
            gas_cap: self.gas_cap,
            gas_remaining: self.gas_cap.saturating_sub(self.gas_used),
            gas_cost_per_iteration: self.gas_cost_per_iter,
            rjoule_used: self.rjoule_used,
            rjoule_cap: self.rjoule_cap,
            rjoule_remaining: (self.rjoule_cap - self.rjoule_used).max(0.0),
            rjoule_enabled: self.rjoule_enabled,
        }
    }

    /// Remaining gas budget (for capping sub-cascade budgets).
    pub fn remaining_gas(&self) -> u64 {
        self.gas_cap.saturating_sub(self.gas_used)
    }

    /// Remaining rJoule budget (for capping sub-cascade budgets).
    pub fn remaining_rjoule(&self) -> f64 {
        (self.rjoule_cap - self.rjoule_used).max(0.0)
    }

    /// Inject the `_gas` and `_rjoule` context keys (for template awareness).
    pub fn inject_into_context(&self, context: &mut std::collections::HashMap<String, Value>) {
        let snap = self.snapshot();
        context.insert("_gas".to_string(), snap.gas_json());
        context.insert("_rjoule".to_string(), snap.rjoule_json());
    }

    /// Gas used so far.
    pub fn gas_used(&self) -> u64 {
        self.gas_used
    }

    /// rJoule used so far.
    pub fn rjoule_used(&self) -> f64 {
        self.rjoule_used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gas_config(cap: u32, cost_per_iter: u32, alert: f64, hard: bool) -> BundleGasConfig {
        BundleGasConfig {
            cap,
            cost_per_iteration: cost_per_iter,
            alert_threshold: alert,
            hard_limit: hard,
        }
    }

    fn rjoule_config(cap: u32, alert: f64, hard: bool) -> RjouleConfig {
        RjouleConfig {
            cap,
            alert_threshold: alert,
            hard_limit: hard,
        }
    }

    #[test]
    fn charge_iteration_deducts_cost_per_iter() {
        let gas = gas_config(1000, 100, 0.8, true);
        let rjoule = rjoule_config(0, 0.8, true);
        let mut tracker = BudgetTracker::new(&gas, &rjoule);
        assert_eq!(tracker.gas_used(), 0);
        tracker.charge_iteration();
        assert_eq!(tracker.gas_used(), 100);
        tracker.charge_iteration();
        assert_eq!(tracker.gas_used(), 200);
    }

    #[test]
    fn check_exhausted_returns_gas_when_cap_hit_with_hard_limit() {
        let gas = gas_config(200, 100, 0.8, true);
        let rjoule = rjoule_config(0, 0.8, true);
        let mut tracker = BudgetTracker::new(&gas, &rjoule);
        tracker.charge_iteration();
        assert_eq!(tracker.gas_used(), 100);
        assert_eq!(tracker.check_exhausted(1), None);
        tracker.charge_iteration();
        assert_eq!(tracker.gas_used(), 200);
        assert_eq!(tracker.check_exhausted(2), Some(BudgetExhaustion::Gas));
    }

    #[test]
    fn check_exhausted_returns_none_when_hard_limit_false() {
        let gas = gas_config(100, 100, 0.8, false);
        let rjoule = rjoule_config(0, 0.8, true);
        let mut tracker = BudgetTracker::new(&gas, &rjoule);
        tracker.charge_iteration();
        // gas_used == gas_cap but hard_limit is false → not exhausted
        assert_eq!(tracker.check_exhausted(1), None);
    }

    #[test]
    fn check_exhausted_returns_rjoule_when_cap_hit() {
        let gas = gas_config(100000, 100, 0.8, true);
        let rjoule = rjoule_config(5, 0.8, true);
        let mut tracker = BudgetTracker::new(&gas, &rjoule);
        tracker.charge_rjoule(6.0);
        assert_eq!(tracker.check_exhausted(1), Some(BudgetExhaustion::Rjoule));
    }

    #[test]
    fn check_exhausted_returns_none_when_rjoule_disabled() {
        let gas = gas_config(100000, 100, 0.8, true);
        let rjoule = rjoule_config(0, 0.8, true); // cap=0 → disabled
        let mut tracker = BudgetTracker::new(&gas, &rjoule);
        tracker.charge_rjoule(100.0); // no-op effectively, rjoule disabled
        assert_eq!(tracker.check_exhausted(1), None);
    }

    #[test]
    fn alert_fires_once_per_threshold_crossing() {
        // alert at 80%, cap 1000, cost 100/iter → alert fires at iteration 8
        let gas = gas_config(1000, 100, 0.8, true);
        let rjoule = rjoule_config(0, 0.8, true);
        let mut tracker = BudgetTracker::new(&gas, &rjoule);
        for _ in 0..7 {
            tracker.charge_iteration();
            tracker.check_exhausted(1);
        }
        // gas_used = 700, below 80% threshold → no alert yet
        assert!(!tracker.gas_alerted);
        tracker.charge_iteration(); // 800 = 80%
        tracker.check_exhausted(1);
        assert!(tracker.gas_alerted);
        // Subsequent checks should not re-fire (idempotent — verified by
        // the `!gas_alerted` guard; no panic on second check).
        tracker.charge_iteration();
        tracker.check_exhausted(1);
        assert!(tracker.gas_alerted);
    }

    #[test]
    fn snapshot_serializes_to_gas_rjoule_json() {
        let gas = gas_config(1000, 100, 0.8, true);
        let rjoule = rjoule_config(5, 0.8, true);
        let mut tracker = BudgetTracker::new(&gas, &rjoule);
        tracker.charge_iteration();
        tracker.charge_rjoule(1.5);
        let snap = tracker.snapshot();

        let gas_json = snap.gas_json();
        assert_eq!(gas_json["used"], 100);
        assert_eq!(gas_json["cap"], 1000);
        assert_eq!(gas_json["remaining"], 900);
        assert_eq!(gas_json["cost_per_iteration"], 100);

        let rjoule_json = snap.rjoule_json();
        assert_eq!(rjoule_json["used"], 1.5);
        assert_eq!(rjoule_json["cap"], 5.0);
        assert_eq!(rjoule_json["remaining"], 3.5);
        assert_eq!(rjoule_json["enabled"], true);
    }

    #[test]
    fn inject_into_context_writes_gas_and_rjoule_keys() {
        let gas = gas_config(1000, 100, 0.8, true);
        let rjoule = rjoule_config(5, 0.8, true);
        let mut tracker = BudgetTracker::new(&gas, &rjoule);
        tracker.charge_iteration();

        let mut ctx = std::collections::HashMap::new();
        tracker.inject_into_context(&mut ctx);

        assert!(ctx.contains_key("_gas"));
        assert!(ctx.contains_key("_rjoule"));
        assert_eq!(ctx["_gas"]["used"], 100);
        assert_eq!(ctx["_rjoule"]["enabled"], true);
    }

    #[test]
    fn consume_child_deducts_sub_cascade_budget() {
        let gas = gas_config(10000, 100, 0.8, true);
        let rjoule = rjoule_config(10, 0.8, true);
        let mut tracker = BudgetTracker::new(&gas, &rjoule);
        tracker.consume_child(3000, 4.0);
        assert_eq!(tracker.gas_used(), 3000);
        assert_eq!(tracker.rjoule_used(), 4.0);
    }

    #[test]
    fn remaining_gas_and_rjoule_for_sub_cascade_capping() {
        let gas = gas_config(10000, 100, 0.8, true);
        let rjoule = rjoule_config(10, 0.8, true);
        let mut tracker = BudgetTracker::new(&gas, &rjoule);
        tracker.charge_iteration(); // -100 gas
        tracker.charge_rjoule(2.0); // -2 rjoule
        assert_eq!(tracker.remaining_gas(), 9900);
        assert_eq!(tracker.remaining_rjoule(), 8.0);
    }
}
