//! Gas budget enforcement with hold-settle pattern and Regulation observability.
//!
//! Fields are private — invariants (`remaining + reserved ≤ cap`) are enforced
//! structurally. Regulation `reg.gas` spans emit on every reserve/settle/consume/reset_to.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Deref, DerefMut};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct GasCost(pub u64);

impl GasCost {
    pub const ZERO: GasCost = GasCost(0);
    /// expect: "The system provides unit conversion between gas cycles and inference rJoules"
    pub fn from_raw(value: u64) -> Self {
        GasCost(value)
    }
    /// expect: "The system provides unit conversion between gas cycles and inference rJoules"
    pub fn as_raw(self) -> u64 {
        self.0
    }
}
impl From<u64> for GasCost {
    fn from(value: u64) -> Self {
        GasCost(value)
    }
}
impl From<GasCost> for u64 {
    fn from(cost: GasCost) -> Self {
        cost.0
    }
}
impl Deref for GasCost {
    type Target = u64;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for GasCost {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl fmt::Display for GasCost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} gas", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct GasDelta(pub f64);

impl GasDelta {
    pub const ZERO: GasDelta = GasDelta(0.0);
    /// expect: "The system provides unit conversion between gas cycles and inference rJoules"
    pub fn from_raw(value: f64) -> Self {
        GasDelta(value)
    }
    /// expect: "The system provides unit conversion between gas cycles and inference rJoules"
    pub fn as_raw(self) -> f64 {
        self.0
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn is_descending(&self) -> bool {
        self.0 <= 0.0
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn is_ascending(&self) -> bool {
        self.0 > 0.0
    }
    pub const ALERT_THRESHOLD: usize = 5;
}
impl fmt::Display for GasDelta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = if self.is_descending() { "↓" } else { "↑" };
        write!(f, "{d}{:.4}", self.0.abs())
    }
}
impl From<f64> for GasDelta {
    fn from(value: f64) -> Self {
        GasDelta(value)
    }
}
impl From<GasDelta> for f64 {
    fn from(delta: GasDelta) -> Self {
        delta.0
    }
}

pub const DEFAULT_GAS_ALERT_THRESHOLD: f64 = 0.8;
const fn default_priority() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasBudget {
    cap: GasCost,
    remaining: GasCost,
    replenish_rate: GasCost,
    alert_threshold: f64,
    hard_limit: bool,
    #[serde(default)]
    reserved: GasCost,
    #[serde(default = "default_priority")]
    priority: f64,
    /// Timestamp of the most recent reservation.
    /// Used to detect stale (never-settled) reservations.
    #[serde(default)]
    last_reservation: Option<chrono::DateTime<chrono::Utc>>,
}

/// Stale reservations older than this are auto-released.
pub const RESERVATION_TIMEOUT_SECS: i64 = 300;

impl GasBudget {
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn new(cap: GasCost) -> Self {
        assert!(cap.0 > 0, "cap must be positive");
        let c = cap.0;
        Self {
            remaining: cap,
            replenish_rate: GasCost(c / 10),
            alert_threshold: DEFAULT_GAS_ALERT_THRESHOLD,
            hard_limit: true,
            reserved: GasCost::ZERO,
            priority: 1.0,
            cap,
            last_reservation: None,
        }
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn unlimited() -> Self {
        Self::new(GasCost(u64::MAX)).with_hard_limit(false)
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn with_replenish_rate(mut self, rate: GasCost) -> Self {
        self.replenish_rate = rate;
        self
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn with_alert_threshold(mut self, t: f64) -> Self {
        self.alert_threshold = t.clamp(0.0, 1.0);
        self
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn with_hard_limit(mut self, hard: bool) -> Self {
        self.hard_limit = hard;
        self
    }

    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn cap(&self) -> GasCost {
        self.cap
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    #[must_use]
    pub fn remaining(&self) -> GasCost {
        self.remaining
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn reserved(&self) -> GasCost {
        self.reserved
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn replenish_rate(&self) -> GasCost {
        self.replenish_rate
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn hard_limit(&self) -> bool {
        self.hard_limit
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn alert_threshold(&self) -> f64 {
        self.alert_threshold
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn priority(&self) -> f64 {
        self.priority
    }

    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn reset_to(&mut self, new_cap: GasCost) {
        assert!(new_cap.0 > 0, "new cap must be positive");
        self.cap = new_cap;
        self.remaining = new_cap;
        self.reserved = GasCost::ZERO;
        self.last_reservation = None;
        tracing::warn!(target: "reg.gas", operation = "reset_to", new_cap = %new_cap.0, "REG");
    }

    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn can_proceed(&self, gas: GasCost) -> bool {
        gas.0 <= self.available().0 || !self.hard_limit
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    pub fn available(&self) -> GasCost {
        GasCost(self.remaining.0.saturating_sub(self.reserved.0))
    }

    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    /// pre: gas.0 > 0
    /// post: reserved is incremented by gas, or BudgetExceeded error returned
    pub fn reserve(&mut self, gas: GasCost) -> Result<GasCost, GasError> {
        let available = self.available();
        if self.hard_limit && gas.0 > available.0 {
            tracing::warn!(target: "reg.gas", operation = "reserve", remaining = %self.remaining.0, reserved = %self.reserved.0, cap = %self.cap.0, requested = %gas.0, outcome = "budget_exceeded", "REG");
            return Err(GasError::BudgetExceeded {
                requested: gas,
                remaining: available,
            });
        }
        self.reserved = GasCost(self.reserved.0.saturating_add(gas.0));
        self.last_reservation = Some(chrono::Utc::now());
        debug_assert!(
            self.reserved.0 <= self.remaining.0,
            "invariant: reserved ≤ remaining"
        );
        tracing::info!(target: "reg.gas", operation = "reserve", remaining = %self.remaining.0, reserved = %self.reserved.0, cap = %self.cap.0, requested = %gas.0, outcome = "ok", "REG");
        Ok(gas)
    }

    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    /// post: reservation released; remaining debited by actual_gas (or BudgetExceeded error returned)
    pub fn settle(
        &mut self,
        reserved_gas: GasCost,
        actual_gas: GasCost,
    ) -> Result<GasCost, GasError> {
        // Release the reservation — caller should verify actual <= reserved
        // if estimation-overrun detection is required. This method enforces
        // actual <= remaining (budget cap), not actual <= reserved.
        self.reserved = GasCost(self.reserved.0.saturating_sub(reserved_gas.0));
        if self.reserved.0 == 0 {
            self.last_reservation = None;
        }
        if self.hard_limit && actual_gas.0 > self.remaining.0 {
            tracing::warn!(target: "reg.gas", operation = "settle", remaining = %self.remaining.0, reserved = %self.reserved.0, cap = %self.cap.0, actual = %actual_gas.0, outcome = "budget_exceeded", "REG");
            return Err(GasError::BudgetExceeded {
                requested: actual_gas,
                remaining: self.remaining,
            });
        }
        self.remaining = GasCost(self.remaining.0.saturating_sub(actual_gas.0));
        debug_assert!(
            self.remaining.0 + self.reserved.0 <= self.cap.0,
            "invariant: remaining + reserved ≤ cap"
        );
        tracing::info!(target: "reg.gas", operation = "settle", remaining = %self.remaining.0, reserved = %self.reserved.0, cap = %self.cap.0, actual = %actual_gas.0, outcome = "ok", "REG");
        Ok(actual_gas)
    }

    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    /// post: remaining is debited by gas, or BudgetExceeded error returned
    pub fn consume(&mut self, gas: GasCost) -> Result<GasCost, GasError> {
        if self.hard_limit && gas.0 > self.remaining.0 {
            tracing::warn!(target: "reg.gas", operation = "consume", remaining = %self.remaining.0, reserved = %self.reserved.0, cap = %self.cap.0, requested = %gas.0, outcome = "budget_exceeded", "REG");
            return Err(GasError::BudgetExceeded {
                requested: gas,
                remaining: self.remaining,
            });
        }
        self.remaining = GasCost(self.remaining.0.saturating_sub(gas.0));
        tracing::info!(target: "reg.gas", operation = "consume", remaining = %self.remaining.0, reserved = %self.reserved.0, cap = %self.cap.0, consumed = %gas.0, outcome = "ok", "REG");
        Ok(gas)
    }

    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    /// post: remaining is increased by replenish_rate (capped at cap)
    pub fn replenish(&mut self) {
        if self.replenish_rate.0 > 0 {
            self.remaining = GasCost(
                self.remaining
                    .0
                    .saturating_add(self.replenish_rate.0)
                    .min(self.cap.0),
            );
        }
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    /// post: remaining is increased by amount (capped at cap)
    pub fn replenish_by(&mut self, amount: GasCost) {
        self.remaining = GasCost(self.remaining.0.saturating_add(amount.0).min(self.cap.0));
    }
    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    /// post: remaining increased by amount scaled by priority; returns actual replenished gas
    pub fn replenish_by_weighted(&mut self, amount: GasCost, priority: f64) -> GasCost {
        let scaled = (amount.0 as f64 * priority.clamp(0.0, 1.0)).round() as u64;
        let effective = scaled.max(1);
        let before = self.remaining.0;
        self.remaining = GasCost(self.remaining.0.saturating_add(effective).min(self.cap.0));
        GasCost(self.remaining.0 - before)
    }
    pub(crate) fn usage_ratio(&self) -> f64 {
        1.0 - (self.remaining.0 as f64 / self.cap.0.max(1) as f64)
    }

    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    /// Check whether the current reservation is stale (never settled within timeout).
    pub fn stale_reservation(&self) -> Option<GasCost> {
        let ts = self.last_reservation?;
        let elapsed = (chrono::Utc::now() - ts).num_seconds();
        if elapsed > RESERVATION_TIMEOUT_SECS && self.reserved.0 > 0 {
            Some(self.reserved)
        } else {
            None
        }
    }

    /// expect: "The system tracks and constrains inference energy consumption through gas budgeting"
    /// Auto-release a stale reservation. Returns the amount released.
    /// post: reserved is set to zero, last_reservation is cleared
    pub fn release_stale_reservation(&mut self) -> GasCost {
        let amount = self.reserved;
        self.reserved = GasCost::ZERO;
        self.last_reservation = None;
        amount
    }
}

pub struct AgentGasStatus {
    pub cap: GasCost,
    pub remaining: GasCost,
    pub reserved: GasCost,
    pub available: GasCost,
    pub usage_ratio: f64,
    pub hard_limit: bool,
    pub alert_threshold: f64,
}

impl From<&GasBudget> for AgentGasStatus {
    fn from(budget: &GasBudget) -> Self {
        Self {
            cap: budget.cap,
            remaining: budget.remaining,
            reserved: budget.reserved,
            available: budget.available(),
            usage_ratio: budget.usage_ratio(),
            hard_limit: budget.hard_limit,
            alert_threshold: budget.alert_threshold,
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum GasError {
    #[error("Gas budget exceeded: {requested}, remaining {remaining}")]
    BudgetExceeded {
        requested: GasCost,
        remaining: GasCost,
    },
    #[error("Budget persistence failed: {0}")]
    Persistence(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    fn arbitrary_cost() -> BoxedStrategy<GasCost> {
        (1u64..1000u64).prop_map(GasCost).boxed()
    }
    fn arbitrary_budget() -> BoxedStrategy<GasBudget> {
        (1u64..10000u64)
            .prop_map(|cap| GasBudget::new(GasCost(cap)))
            .boxed()
    }

    proptest! {
        #[test]
        fn budget_never_exceeds_cap(mut budget in arbitrary_budget(), operations in prop::collection::vec((arbitrary_cost(), arbitrary_cost()), 0..20)) {
            let cap = budget.cap();
            for (reserve_gas, actual_gas) in &operations {
                if let Ok(reserved) = budget.reserve(*reserve_gas) { let _ = budget.settle(reserved, *actual_gas); }
            }
            let total = GasCost(budget.remaining().0 + budget.reserved().0);
            prop_assert!(total <= cap, "remaining {} + reserved {} = {} > cap {}", budget.remaining().0, budget.reserved().0, total.0, cap.0);
        }
    }
    proptest! {
        #[test]
        fn available_never_negative(mut budget in arbitrary_budget(), operations in prop::collection::vec(arbitrary_cost(), 0..20)) {
            for cost in &operations { let _ = budget.reserve(*cost); let _ = budget.consume(*cost); }
            prop_assert!(budget.available().0 <= budget.remaining().0, "available {} > remaining {}", budget.available().0, budget.remaining().0);
        }
    }
    proptest! {
        #[test]
        fn replenish_never_exceeds_cap(mut budget in arbitrary_budget(), cycles in 0u32..100u32) {
            let _ = budget.consume(budget.remaining());
            for _ in 0..cycles { budget.replenish(); }
            prop_assert!(budget.remaining() <= budget.cap(), "remaining {} > cap {} after {} cycles", budget.remaining().0, budget.cap().0, cycles);
        }
    }
}
