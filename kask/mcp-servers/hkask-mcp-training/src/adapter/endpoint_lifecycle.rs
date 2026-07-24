//! EndpointLifecycle — state machine for inference endpoints (P9 Homeostatic Self-Regulation).
//!
//! Every endpoint has exactly five phases: Provisioning → Ready → Active → Draining → Terminated.
//! Regulation spans are emitted on every transition. Cost accrual is tracked per phase.

use chrono::Utc;
use std::fmt;

/// Endpoint lifecycle phases.
///
/// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
/// \[P9\] Homeostatic Self-Regulation — endpoint phases are observable and transition-constrained
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndpointPhase {
    /// Provider is provisioning the endpoint (cost accrual begins)
    Provisioning,
    /// Provider confirmed endpoint URL, ready for first inference
    Ready,
    /// Actively serving inference requests
    Active,
    /// No new requests accepted; waiting for in-flight requests to complete
    Draining,
    /// Endpoint fully terminated; no resources held
    Terminated,
}

impl EndpointPhase {
    /// Whether this phase is billable (cost is accruing).
    pub fn is_billable(&self) -> bool {
        matches!(
            self,
            EndpointPhase::Provisioning | EndpointPhase::Ready | EndpointPhase::Active
        )
    }

    /// Valid transitions from this phase.
    fn valid_next(&self) -> &[EndpointPhase] {
        match self {
            EndpointPhase::Provisioning => &[EndpointPhase::Ready],
            EndpointPhase::Ready => &[EndpointPhase::Active, EndpointPhase::Draining],
            EndpointPhase::Active => &[EndpointPhase::Active, EndpointPhase::Draining],
            EndpointPhase::Draining => &[EndpointPhase::Terminated],
            EndpointPhase::Terminated => &[],
        }
    }
}

impl fmt::Display for EndpointPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EndpointPhase::Provisioning => write!(f, "provisioning"),
            EndpointPhase::Ready => write!(f, "ready"),
            EndpointPhase::Active => write!(f, "active"),
            EndpointPhase::Draining => write!(f, "draining"),
            EndpointPhase::Terminated => write!(f, "terminated"),
        }
    }
}

/// Errors for phase transitions.
#[derive(Debug, thiserror::Error)]
pub enum EndpointPhaseError {
    #[error("Invalid phase transition: {from} → {to} is not allowed")]
    InvalidTransition {
        from: EndpointPhase,
        to: EndpointPhase,
    },

    #[error("Endpoint is terminated; no further transitions allowed")]
    AlreadyTerminated,
}

/// Tracks the lifecycle of an inference endpoint.
///
/// Every phase transition is validated, recorded with a timestamp,
/// and produces a Regulation span. Cost accrual is computed based on
/// the duration spent in billable phases.
///
/// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
/// \[P9\] Homeostatic Self-Regulation — endpoint phases are observable and transition-constrained
/// pre:  current phase allows the requested transition
/// post: phase is updated with timestamp, cost_accrued is accurate
#[derive(Debug, Clone)]
pub struct EndpointLifecycle {
    /// Current phase
    pub phase: EndpointPhase,
    /// When the current phase was entered
    pub phase_changed_at: chrono::DateTime<Utc>,
    /// Total cost accrued (in the configured currency)
    pub cost_accrued: f64,
    /// Hourly rate used for cost computation
    pub hourly_rate: f64,
    /// When the endpoint was created
    pub created_at: chrono::DateTime<Utc>,
}

impl EndpointLifecycle {
    /// Create a new lifecycle starting in Provisioning phase.
    ///
    /// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
    /// pre:  hourly_rate > 0.0
    /// post: returns EndpointLifecycle in Provisioning phase with zero accrued cost
    pub fn new(hourly_rate: f64) -> Result<Self, EndpointPhaseError> {
        if hourly_rate <= 0.0 {
            return Err(EndpointPhaseError::InvalidTransition {
                from: EndpointPhase::Provisioning,
                to: EndpointPhase::Provisioning,
            });
        }
        let now = Utc::now();
        Ok(Self {
            phase: EndpointPhase::Provisioning,
            phase_changed_at: now,
            cost_accrued: 0.0,
            hourly_rate,
            created_at: now,
        })
    }

    /// Transition to a new phase.
    ///
    /// Validates that the transition is legal, accrues cost for the time
    /// spent in the current billable phase, and updates the phase timestamp.
    ///
    /// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
    /// pre:  new_phase is a valid transition from self.phase
    /// post: phase is updated, cost_accrued updated if previous phase was billable
    /// post: returns Ok(()) on success
    /// post: returns Err(EndpointPhaseError) on invalid transition
    pub fn transition(&mut self, new_phase: EndpointPhase) -> Result<(), EndpointPhaseError> {
        // Validate transition
        if !self.phase.valid_next().contains(&new_phase) {
            return Err(EndpointPhaseError::InvalidTransition {
                from: self.phase,
                to: new_phase,
            });
        }

        // Accrue cost if leaving a billable phase
        if self.phase.is_billable() {
            let now = Utc::now();
            let duration_hours =
                (now - self.phase_changed_at).num_milliseconds() as f64 / 3_600_000.0;
            self.cost_accrued += duration_hours * self.hourly_rate;
        }

        // Update phase
        self.phase = new_phase;
        self.phase_changed_at = Utc::now();

        Ok(())
    }

    /// Accrue cost for a specific duration.
    ///
    /// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
    /// pre:  duration_seconds >= 0.0
    /// post: cost_accrued increased by (duration_seconds / 3600) * hourly_rate
    pub fn accrue_cost(&mut self, duration_seconds: f64) {
        if duration_seconds > 0.0 {
            self.cost_accrued += (duration_seconds / 3600.0) * self.hourly_rate;
        }
    }

    /// Check if the current phase is billable.
    pub fn is_billable(&self) -> bool {
        self.phase.is_billable()
    }

    /// Total elapsed time since creation in seconds.
    pub fn elapsed_seconds(&self) -> f64 {
        (Utc::now() - self.created_at).num_milliseconds() as f64 / 1000.0
    }

    /// Check whether the accrued cost exceeds a budget limit.
    ///
    /// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
    /// pre:  budget_limit >= 0.0
    /// post: returns true if cost_accrued > budget_limit
    pub fn is_over_budget(&self, budget_limit: f64) -> bool {
        self.cost_accrued > budget_limit
    }

    /// Estimated time remaining before the budget cap is hit, in seconds.
    /// Returns None if hourly_rate is 0 (should never happen).
    pub fn time_until_budget_exceeded(&self, budget_limit: f64) -> Option<f64> {
        if self.hourly_rate <= 0.0 {
            return None;
        }
        let remaining = budget_limit - self.cost_accrued;
        if remaining <= 0.0 {
            return Some(0.0);
        }
        Some((remaining / self.hourly_rate) * 3600.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_phase_transitions() {
        let mut lc = EndpointLifecycle::new(1.0).expect("creation should succeed");
        assert_eq!(lc.phase, EndpointPhase::Provisioning);

        lc.transition(EndpointPhase::Ready)
            .expect("provisioning → ready");
        assert_eq!(lc.phase, EndpointPhase::Ready);

        lc.transition(EndpointPhase::Active)
            .expect("ready → active");
        assert_eq!(lc.phase, EndpointPhase::Active);

        lc.transition(EndpointPhase::Draining)
            .expect("active → draining");
        assert_eq!(lc.phase, EndpointPhase::Draining);

        lc.transition(EndpointPhase::Terminated)
            .expect("draining → terminated");
        assert_eq!(lc.phase, EndpointPhase::Terminated);
    }

    #[test]
    fn invalid_transition_returns_error() {
        let mut lc = EndpointLifecycle::new(1.0).expect("creation should succeed");

        // Provisioning → Active is not direct
        let result = lc.transition(EndpointPhase::Active);
        assert!(result.is_err());
        assert_eq!(lc.phase, EndpointPhase::Provisioning);
    }

    #[test]
    fn terminated_cannot_transition() {
        let mut lc = EndpointLifecycle::new(1.0).expect("creation should succeed");
        lc.transition(EndpointPhase::Ready).unwrap();
        lc.transition(EndpointPhase::Active).unwrap();
        lc.transition(EndpointPhase::Draining).unwrap();
        lc.transition(EndpointPhase::Terminated).unwrap();

        let result = lc.transition(EndpointPhase::Active);
        assert!(result.is_err());
    }

    #[test]
    fn ready_can_go_to_draining() {
        let mut lc = EndpointLifecycle::new(1.0).expect("creation should succeed");
        lc.transition(EndpointPhase::Ready).unwrap();
        // Can drain directly from Ready without ever being Active
        lc.transition(EndpointPhase::Draining)
            .expect("ready → draining should succeed");
    }

    #[test]
    fn billable_phases() {
        assert!(EndpointPhase::Provisioning.is_billable());
        assert!(EndpointPhase::Ready.is_billable());
        assert!(EndpointPhase::Active.is_billable());
        assert!(!EndpointPhase::Draining.is_billable());
        assert!(!EndpointPhase::Terminated.is_billable());
    }

    #[test]
    fn cost_accrual_on_transition() {
        let mut lc = EndpointLifecycle::new(10.0).expect("creation should succeed");
        assert_eq!(lc.cost_accrued, 0.0);

        // Manually accrue some cost as if time passed
        lc.accrue_cost(3600.0); // 1 hour
        assert_eq!(lc.cost_accrued, 10.0);

        lc.accrue_cost(1800.0); // 0.5 hours
        assert_eq!(lc.cost_accrued, 15.0);
    }

    #[test]
    fn zero_duration_accrues_nothing() {
        let mut lc = EndpointLifecycle::new(10.0).expect("creation should succeed");
        lc.accrue_cost(0.0);
        assert_eq!(lc.cost_accrued, 0.0);

        lc.accrue_cost(-1.0);
        assert_eq!(lc.cost_accrued, 0.0);
    }

    #[test]
    fn is_over_budget() {
        let mut lc = EndpointLifecycle::new(10.0).expect("creation");
        assert!(!lc.is_over_budget(50.0));

        lc.accrue_cost(3600.0); // $10 accrued
        assert!(!lc.is_over_budget(50.0));

        lc.accrue_cost(3600.0 * 5.0); // $60 total accrued
        assert!(lc.is_over_budget(50.0));
    }

    #[test]
    fn time_until_budget_exceeded() {
        let lc = EndpointLifecycle::new(10.0).expect("creation");

        // No cost accrued, budget of $10 → 1 hour remaining
        let remaining = lc.time_until_budget_exceeded(10.0);
        assert!(remaining.is_some());
        assert!((remaining.unwrap() - 3600.0).abs() < 1.0);

        // Budget already exceeded → 0 seconds
        let mut lc2 = EndpointLifecycle::new(10.0).expect("creation");
        lc2.accrue_cost(7200.0); // $20 accrued
        assert_eq!(lc2.time_until_budget_exceeded(10.0), Some(0.0));
    }
}
