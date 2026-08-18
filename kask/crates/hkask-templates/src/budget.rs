//! Budget tracking — rJoule (inference) accounting.
//!
//! Extracted from the executor to unify the exhaustion/alert logic for rJoule.
//!
//! # Design
//!
//! `BudgetTracker` is a pure state machine: charge rJoule per inference call,
//! check exhaustion, fire alerts once per threshold crossing, and snapshot to
//! the `_rjoule` context JSON shape that templates depend on. It has no
//! dependency on `InferencePort`, `ToolPort`, or the executor — it's a leaf
//! module that the executor composes.
//!
//! Timeout is the sole kill switch for runaway processes. rJoule is the
//! budget/feedback/calibration metric for inference spend.

use crate::bundle::config::RjouleConfig;
use crate::step_context::ContextMap;
use serde_json::{Value, json};
use tracing::info;

/// Which budget was exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetExhaustion {
    Rjoule,
}

/// A snapshot of the current budget state, serializable to the `_rjoule`
/// context JSON that templates reference for budget-aware rendering.
#[derive(Debug, Clone)]
pub struct BudgetSnapshot {
    pub rjoule_used: f64,
    pub rjoule_cap: f64,
    pub rjoule_remaining: f64,
    pub rjoule_enabled: bool,
}

impl BudgetSnapshot {
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

/// Tracks rJoule (inference energy) consumption against a declared cap, with
/// once-per-threshold alerting and hard-limit exhaustion checks.
///
/// Constructed from a manifest's `rjoule:` config block. The executor charges
/// rJoule via `charge_rjoule` after each inference call (using the observed
/// USD cost from `InferenceResult.cost_usd`), checks exhaustion after each
/// charge, and snapshots the state into the context map for template awareness.
pub struct BudgetTracker {
    // rJoule (inference)
    rjoule_used: f64,
    rjoule_cap: f64,
    rjoule_alert_threshold: f64,
    rjoule_hard_limit: bool,
    rjoule_enabled: bool,
    rjoule_alerted: bool,
}

impl BudgetTracker {
    /// Construct from manifest config block.
    ///
    /// When `rjoule.cap == 0`, the cascade runs with no inference budget —
    /// `rjoule_enabled` is false and `check_exhausted` never trips on rJoule.
    /// This is the "forgot to configure" failure mode: the operator cannot
    /// distinguish "intentionally unlimited" from "forgot to set the cap"
    /// without this warn. `manifest_compliance.rs` catches the "uses inference
    /// but cap == 0" case at validation time; this warn catches the runtime
    /// case where the manifest was loaded without the compliance gate.
    pub fn new(rjoule: &RjouleConfig) -> Self {
        let rjoule_cap = rjoule.cap as f64;
        if rjoule.cap == 0 {
            tracing::warn!(
                target: "hkask.templates",
                "BudgetTracker::new: rjoule.cap == 0 — cascade runs with no inference \
                 budget. If this cascade uses `select` steps, inference calls will \
                 not be charged. Set rjoule.cap > 0 to enable charging, or set \
                 hard_limit = false to make the absence explicit."
            );
        }
        Self {
            rjoule_used: 0.0,
            rjoule_cap,
            rjoule_alert_threshold: rjoule.alert_threshold,
            rjoule_hard_limit: rjoule.hard_limit,
            rjoule_enabled: rjoule_cap > 0.0,
            rjoule_alerted: false,
        }
    }

    /// Construct a per-task budget tracker from remaining rJoule capacity.
    /// Each concurrent branch gets its own tracker so they don't share
    /// mutable state; the caller merges consumption back into the parent via
    /// `consume_child` after the wave.
    pub fn from_remaining(rjoule_remaining: f64) -> Self {
        Self {
            rjoule_used: 0.0,
            rjoule_cap: rjoule_remaining,
            rjoule_alert_threshold: 0.8,
            rjoule_hard_limit: true,
            rjoule_enabled: rjoule_remaining > 0.0,
            rjoule_alerted: false,
        }
    }

    /// Return the rJoule cost charged by the most recent `charge_rjoule` call.
    /// Intended for merging a per-task tracker's cost back into the parent
    /// (per-branch settle + join-sum).
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

    /// Charge rJoule consumed by a sub-cascade (flowdef). The sub-cascade's
    /// budget was capped to the parent's remaining budget, so this is at most
    /// the parent's remaining allocation. Conservative: may over-count, never
    /// under-counts.
    pub fn consume_child(&mut self, rjoule: f64) {
        self.rjoule_used = (self.rjoule_used + rjoule).max(0.0);
    }

    /// Check whether the rJoule budget is exhausted (hard limit hit). Returns
    /// the exhausted budget if so, emits the appropriate
    /// `reg.skill.budget.*_exhausted` span, and fires the alert if the threshold
    /// was crossed this check.
    ///
    /// `iteration` is used for span emission.
    pub fn check_exhausted(&mut self, iteration: u32) -> Option<BudgetExhaustion> {
        // Fire alerts first (once per threshold crossing) so the alert span
        // precedes the exhaustion span when both fire on the same check.
        self.fire_alerts();

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

    /// Fire the rJoule alert span if the threshold was crossed and not already
    /// alerted. Idempotent — only fires once per tracker lifetime.
    fn fire_alerts(&mut self) {
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
            rjoule_used: self.rjoule_used,
            rjoule_cap: self.rjoule_cap,
            rjoule_remaining: (self.rjoule_cap - self.rjoule_used).max(0.0),
            rjoule_enabled: self.rjoule_enabled,
        }
    }

    /// Remaining rJoule budget (for capping sub-cascade budgets).
    pub fn remaining_rjoule(&self) -> f64 {
        (self.rjoule_cap - self.rjoule_used).max(0.0)
    }

    /// Inject the `_rjoule` context key (for template awareness).
    pub fn inject_into_context<M: ContextMap>(&self, context: &mut M) {
        let snap = self.snapshot();
        context.insert("_rjoule".to_string(), snap.rjoule_json());
    }

    /// rJoule used so far.
    pub fn rjoule_used(&self) -> f64 {
        self.rjoule_used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rjoule_config(cap: u32, alert_threshold: f64, hard_limit: bool) -> RjouleConfig {
        RjouleConfig {
            cap,
            alert_threshold,
            hard_limit,
        }
    }

    #[test]
    fn check_exhausted_returns_rjoule_when_cap_hit() {
        let rjoule = rjoule_config(5, 0.8, true);
        let mut tracker = BudgetTracker::new(&rjoule);
        tracker.charge_rjoule(6.0);
        assert_eq!(tracker.check_exhausted(1), Some(BudgetExhaustion::Rjoule));
    }

    #[test]
    fn check_exhausted_returns_none_when_rjoule_disabled() {
        let rjoule = rjoule_config(0, 0.8, true); // cap=0 → disabled
        let mut tracker = BudgetTracker::new(&rjoule);
        tracker.charge_rjoule(100.0); // no-op effectively, rjoule disabled
        assert_eq!(tracker.check_exhausted(1), None);
    }

    #[test]
    fn alert_fires_once_per_threshold_crossing() {
        // alert at 80%, cap 10 rJoule → alert fires at 8.0 rJoule
        let rjoule = rjoule_config(10, 0.8, true);
        let mut tracker = BudgetTracker::new(&rjoule);
        tracker.charge_rjoule(7.0);
        tracker.check_exhausted(1);
        assert!(!tracker.rjoule_alerted);
        tracker.charge_rjoule(1.0); // 8.0 = 80%
        tracker.check_exhausted(1);
        assert!(tracker.rjoule_alerted);
        // Subsequent checks should not re-fire (idempotent).
        tracker.charge_rjoule(1.0);
        tracker.check_exhausted(1);
        assert!(tracker.rjoule_alerted);
    }

    #[test]
    fn snapshot_serializes_to_rjoule_json() {
        let rjoule = rjoule_config(5, 0.8, true);
        let mut tracker = BudgetTracker::new(&rjoule);
        tracker.charge_rjoule(1.5);
        let snap = tracker.snapshot();

        let json = snap.rjoule_json();
        assert_eq!(json["used"], 1.5);
        assert_eq!(json["cap"], 5.0);
        assert_eq!(json["remaining"], 3.5);
        assert_eq!(json["enabled"], true);
    }

    #[test]
    fn inject_into_context_writes_rjoule_key() {
        let rjoule = rjoule_config(5, 0.8, true);
        let mut tracker = BudgetTracker::new(&rjoule);
        tracker.charge_rjoule(1.0);

        let mut ctx = std::collections::HashMap::new();
        tracker.inject_into_context(&mut ctx);

        assert!(ctx.contains_key("_rjoule"));
    }

    #[test]
    fn consume_child_deducts_sub_cascade_budget() {
        let rjoule = rjoule_config(10, 0.8, true);
        let mut tracker = BudgetTracker::new(&rjoule);
        tracker.consume_child(4.0);
        assert_eq!(tracker.rjoule_used(), 4.0);
    }

    #[test]
    fn remaining_rjoule_for_sub_cascade_capping() {
        let rjoule = rjoule_config(10, 0.8, true);
        let mut tracker = BudgetTracker::new(&rjoule);
        tracker.charge_rjoule(2.0);
        assert_eq!(tracker.remaining_rjoule(), 8.0);
    }

    #[test]
    fn from_remaining_creates_tracker_with_specified_budget() {
        let tracker = BudgetTracker::from_remaining(15.0);
        assert_eq!(tracker.remaining_rjoule(), 15.0);
        assert!(tracker.rjoule_enabled);
    }
}
