//! hkask-trust — the expectation-ledger trust model (P13 substrate).
//!
//! The model, its parameter anchoring, and the operator rulings:
//! `kask/docs/research/trust-economics-and-model.md` (anchoring run
//! `55661b35ad904b63`). Proven invariants:
//! `kask/lean/trust_model_invariants.lean` (`TrustModelSpec`, pinned
//! toolchain v4.34.0, axiom audit `[propext, Quot.sound]` only). Principle:
//! P13 (`kask/docs/architecture/core/PRINCIPLES.md`).
//!
//! Anchored constants (operator ruling 2026-10-31: magnitudes anchor on
//! published reference models, not preferences):
//! - keep gain `+4·w`, violate loss `−9·w` trust points — the 9/4 = 2.25
//!   asymmetry is the Tversky–Kahneman (1992) loss-aversion coefficient,
//!   and the asymmetric-gain form is Guo & Yang 2021 (whose published
//!   assumption is that negative experiences impact trust more);
//! - the scale `600 points = T = 1.0` keeps the anchored asymmetry exact
//!   in fixed-point integers; a full-weight keep is 12/600 = 0.02 — the
//!   account's measurement resolution (definitional, not empirical);
//! - the prior `300 = 0.5` is the maximum-entropy Beta(1,1) prior
//!   (Guo & Yang 2021's conjugate form);
//! - the cliff: deception and omission zero the reliance factor — the cost
//!   of deception or omission is −100%, a certainty of failure for the
//!   goal (operator ruling 2026-10-31), benefit-independent;
//! - κ = 1 in the priced cost `Σw·S` is the unit definition: the exchange
//!   rate is carried by the stake's units, not hidden in κ.
//!
//! No floats: the account is integer trust points throughout, matching the
//! Lean pin. No storage here — the fold, pricing, cliff and steer
//! projection are pure; the storage-backed facade (events as curator
//! h_mems, entity `trust:ledger:<id>`) and the MCP tools live in the
//! curator server. [`TrustError::StoreUnavailable`] is defined here so
//! every consumer surfaces outages through one type that must never
//! serialize as an empty account.

use std::fmt;

/// Trust points: 600 = T 1.0 (definitional resolution).
pub const SCALE_POINTS: i32 = 600;
/// The maximum-entropy prior: Beta(1,1) → 0.5, in trust points.
pub const PRIOR_POINTS: i32 = 300;
/// Keep gain per unit weight (trust points): keep(w) = +4·w.
pub const KEEP_PER_WEIGHT: i32 = 4;
/// Violate loss per unit weight (trust points): violate(w) = −9·w.
/// The 9/4 ratio is the anchored 2.25 loss-aversion asymmetry.
pub const VIOLATE_PER_WEIGHT: i32 = 9;

/// How explicitly an expectation was stated — the violation's weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Weight {
    /// An implicit norm (weight 1).
    Implicit,
    /// A committed expectation (weight 2).
    Committed,
    /// An explicit instruction (weight 3) — integrity-class: the hardest
    /// violation class to repair (Kim et al. 2004, verified).
    Explicit,
}

impl Weight {
    /// The weight's integer value (1, 2, 3).
    pub fn points(self) -> i32 {
        match self {
            Weight::Implicit => 1,
            Weight::Committed => 2,
            Weight::Explicit => 3,
        }
    }
}

/// The verification outcome of an expectation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The expectation was honored (oracle-verified where an oracle exists).
    Kept,
    /// The expectation was violated.
    Violated,
    /// Not verified — itself a signal, never silence.
    Unverified,
}

/// One ledger event, the fold's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A kept expectation of the given weight.
    Keep(Weight),
    /// A violated expectation of the given weight.
    Violate(Weight),
}

impl Event {
    /// The event's signed trust-point delta.
    pub fn delta(self) -> i32 {
        match self {
            Event::Keep(w) => KEEP_PER_WEIGHT * w.points(),
            Event::Violate(w) => -VIOLATE_PER_WEIGHT * w.points(),
        }
    }
}

/// One fold step, clamped to the trust interval.
fn evolve(t: i32, event: Event) -> i32 {
    t.saturating_add(event.delta()).clamp(0, SCALE_POINTS)
}

/// Fold the window's events from the maximum-entropy prior (T₀ = 0.5).
pub fn trust_points(events: &[Event]) -> i32 {
    events.iter().fold(PRIOR_POINTS, |t, e| evolve(t, *e))
}

/// Fold from an explicit starting value (the running state between
/// windows); the start is itself clamped.
pub fn trust_points_from(start: i32, events: &[Event]) -> i32 {
    events
        .iter()
        .fold(start.clamp(0, SCALE_POINTS), |t, e| evolve(t, *e))
}

/// The goal's reliance factor: the probability the user can rely on the
/// work. Honest paths draw it from the trust account; deception and
/// omission set it to zero (operator ruling 2026-10-31: the cost of
/// deception or omission is −100% — certainty of failure for the goal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reliance {
    /// An honest action: reliance in percent (0..=100), drawn from T.
    Honest(i32),
    /// Deception or omission: the cliff — reliance collapses to zero.
    Deceptive,
}

impl Reliance {
    /// Reliance in percent; the cliff is zero, benefit-independent.
    pub fn percent(self) -> i32 {
        match self {
            Reliance::Honest(p) => p.clamp(0, 100),
            Reliance::Deceptive => 0,
        }
    }

    /// Whether this action is on the cliff (the goal fails with certainty).
    pub fn is_cliff(self) -> bool {
        matches!(self, Reliance::Deceptive)
    }
}

/// Goal achievement probability in percent: reliance × completion.
/// The cliff zeroes it for ANY completion probability.
pub fn goal_probability_percent(reliance: Reliance, completion_percent: i32) -> i32 {
    reliance
        .percent()
        .saturating_mul(completion_percent.clamp(0, 100))
        / 100
}

/// Delivered value: reliance × benefit. Zero on the cliff for ANY benefit —
/// no amount of task progress survives a zero reliance multiplier.
pub fn delivered_value(reliance_percent: i32, benefit: i64) -> i64 {
    (reliance_percent.clamp(0, 100) as i64).saturating_mul(benefit.max(0)) / 100
}

/// The priced trust cost of violating the given weights at stake `S`:
/// `C_trust = Σw·S` with κ = 1 — the unit definition (the exchange rate is
/// carried by the stake's units, not hidden in κ).
pub fn priced_cost(weights: &[Weight], stake: i64) -> i64 {
    let total_weight: i64 = weights.iter().map(|w| i64::from(w.points())).sum();
    total_weight.saturating_mul(stake.max(0))
}

/// One `steer` projection line — the LLM-legible form of the goal
/// gradient, one compact line per candidate action. A trust-killing
/// action reads as the path terminating (`path: cliff, P_goal: 0%`),
/// never as a mild cost.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteerProjection {
    /// The candidate action's name.
    pub action: String,
    /// Whether this action terminates the goal (the deception/omission
    /// cliff) — distinct from "merely expensive".
    pub cliff: bool,
    /// Projected goal achievement probability in percent (0 on the cliff).
    pub p_goal_percent: i32,
    /// Projected trust-point change.
    pub delta_trust_points: i32,
    /// Why: names the violated expectations (with weights), or the
    /// concealment the action would require.
    pub note: String,
}

impl fmt::Display for SteerProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = if self.cliff { "cliff" } else { "open" };
        write!(
            f,
            "steer: {} | path: {} | P_goal: {}% | ΔT: {:+} pts | {}",
            self.action, path, self.p_goal_percent, self.delta_trust_points, self.note
        )
    }
}

/// The trust module's error. [`TrustError::StoreUnavailable`] must surface
/// as sensor-unavailable — never serialized as an empty account (a broken
/// feedback loop is worse than a surfaced outage; the regulation `Sensor`
/// contract "None is not evidence of recovery" carries the same rule).
#[derive(Debug)]
pub enum TrustError {
    /// The ledger store is unavailable — the account is unknown, not zero.
    StoreUnavailable,
    /// A caller-named ledger event was not found (names the id).
    EventNotFound(String),
}

impl fmt::Display for TrustError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrustError::StoreUnavailable => {
                write!(
                    f,
                    "the trust ledger store is unavailable — account unknown, not zero"
                )
            }
            TrustError::EventNotFound(id) => {
                write!(f, "trust ledger event '{id}' not found")
            }
        }
    }
}

impl std::error::Error for TrustError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// I0: the empty window is the maximum-entropy prior.
    #[test]
    fn prior_is_maximum_entropy() {
        assert_eq!(trust_points(&[]), PRIOR_POINTS);
        assert_eq!(PRIOR_POINTS, SCALE_POINTS / 2);
    }

    /// I1: bounds hold at the floor and the ceiling (anchored arithmetic:
    /// one explicit violation is 27 points, so the floor needs 25 from 600).
    #[test]
    fn bounds_hold_floor_and_ceiling() {
        let violations = vec![Event::Violate(Weight::Explicit); 25];
        assert_eq!(trust_points_from(SCALE_POINTS, &violations), 0);
        let keeps = vec![Event::Keep(Weight::Explicit); 60];
        assert_eq!(trust_points_from(0, &keeps), SCALE_POINTS);
    }

    /// I2: the asymmetry is exactly the anchored 2.25 (9/4), and one
    /// violation costs more than two full keeps restore.
    #[test]
    fn asymmetry_is_anchored_at_2_25() {
        let keep = Event::Keep(Weight::Explicit).delta();
        let violate = Event::Violate(Weight::Explicit).delta();
        assert_eq!(keep, 12);
        assert_eq!(violate, -27);
        assert_eq!(4 * -violate, 9 * keep, "exactly nine-fourths");
        assert!(-violate > 2 * keep, "one violation > two full keeps");
    }

    /// I3: the loss scales with the weight of the violated expectation.
    #[test]
    fn loss_scales_with_weight() {
        let explicit = Event::Violate(Weight::Explicit).delta().abs();
        let implicit = Event::Violate(Weight::Implicit).delta().abs();
        assert!(explicit > implicit);
    }

    /// I4: recovery needs more evidence than the violation destroyed —
    /// one violation plus two keeps leaves T strictly below the prior.
    #[test]
    fn recovery_needs_more_than_two_keeps() {
        let events = [
            Event::Violate(Weight::Explicit),
            Event::Keep(Weight::Explicit),
            Event::Keep(Weight::Explicit),
        ];
        let t = trust_points(&events);
        assert!(
            t < PRIOR_POINTS,
            "two keeps must not undo one violation: {t}"
        );
        assert_eq!(t, 297, "300 - 27 + 12 + 12");
    }

    /// The gradient's sign: a violation never raises T, anywhere in bounds.
    #[test]
    fn violation_never_raises_trust() {
        for start in [0, 1, 27, 300, 599, 600] {
            let t = evolve(start, Event::Violate(Weight::Explicit));
            assert!(t <= start, "violation raised {start} to {t}");
        }
    }

    /// The cliff: deception zeroes goal probability and delivered value
    /// for ANY benefit and ANY completion probability (benefit-
    /// independence), and any honest action with positive reliance
    /// strictly dominates a deceptive one.
    #[test]
    fn the_cliff_zeroes_goal_and_value_for_any_benefit() {
        assert_eq!(goal_probability_percent(Reliance::Deceptive, 99), 0);
        assert_eq!(delivered_value(Reliance::Deceptive.percent(), 1_000_000), 0);
        assert_eq!(
            delivered_value(Reliance::Deceptive.percent(), 1),
            delivered_value(Reliance::Deceptive.percent(), 1_000_000),
            "benefit-independence"
        );
        assert!(
            delivered_value(Reliance::Honest(1).percent(), 1_000_000)
                > delivered_value(Reliance::Deceptive.percent(), 1_000_000),
            "same-benefit dominance (the pinned `honest_action_dominates`): \
             any honest reliance beats deception at the same benefit"
        );
        assert!(Reliance::Deceptive.is_cliff());
        assert!(!Reliance::Honest(50).is_cliff());
    }

    /// Honest goal probability is positive when both factors are.
    #[test]
    fn honest_goal_probability_is_positive() {
        assert!(goal_probability_percent(Reliance::Honest(50), 80) > 0);
    }

    /// Priced cost: Σw·S with κ = 1 (unit definition).
    #[test]
    fn priced_cost_is_weight_times_stake() {
        let weights = [Weight::Explicit, Weight::Explicit, Weight::Committed];
        assert_eq!(priced_cost(&weights, 1000), 8000);
    }

    /// The steer projection renders the cliff as the path terminating —
    /// never as a mild cost.
    #[test]
    fn steer_projection_renders_the_cliff_visibly() {
        let cliff = SteerProjection {
            action: "skip verification, present from memory".into(),
            cliff: true,
            p_goal_percent: 0,
            delta_trust_points: -27,
            note: "conceals e_17 (explicit): reliance collapses — goal fails with certainty".into(),
        };
        let line = cliff.to_string();
        assert!(line.contains("path: cliff"), "cliff must be named: {line}");
        assert!(
            line.contains("P_goal: 0%"),
            "the zero must be visible: {line}"
        );
        assert!(line.contains("goal fails"), "the note must carry: {line}");

        let honest = SteerProjection {
            action: "verify each quote before including it".into(),
            cliff: false,
            p_goal_percent: 80,
            delta_trust_points: 0,
            note: "no violation: honors e_17".into(),
        };
        let open = honest.to_string();
        assert!(
            open.contains("path: open"),
            "the open path must be named: {open}"
        );
        assert!(
            open.contains("P_goal: 80%"),
            "open path keeps a live probability: {open}"
        );
    }

    /// StoreUnavailable must exist as a distinct variant and name the
    /// unknown-not-zero rule in its message (the facade pins the rest).
    #[test]
    fn store_unavailable_names_unknown_not_zero() {
        let msg = TrustError::StoreUnavailable.to_string();
        assert!(msg.contains("unknown"));
        assert!(msg.contains("not zero"));
    }
}
