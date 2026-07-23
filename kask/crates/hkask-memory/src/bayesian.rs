//! Bayesian confidence operations — decay and evidence pooling.
//!
//! Memory decay follows Wozniak & Gorzelanczyk (1995) two-component model
//! of long-term memory, equation (3): R(t) = exp(-t/S).
//!
//! Where S is memory life in days (configurable, default 180 = 6×30 days)
//! and t is days since most recent recall. At t = S, R = exp(-1) ≈ 0.368.
//!
//! Bayesian evidence pooling (log-odds) combines independent confidence
//! estimates for the same proposition.

use chrono::{DateTime, Utc};
use hkask_types::Confidence;

// ── Time utilities ───────────────────────────────────────────────────────────

/// Seconds in one day — used for converting UTC timestamps to elapsed days.
const SECONDS_PER_DAY: f64 = 86400.0;

/// Compute elapsed days since a given DateTime, for use in the
/// Wozniak-Gorzelanczyk forgetting curve R(t) = exp(-t/S).
///
/// Returns 0.0 for timestamps in the future (clock skew safety).
///
/// expect: "The system combines independent confidence estimates using Bayesian evidence pooling"
/// pre:  dt is any valid DateTime<Utc>
/// post: returns days elapsed as f64, always ≥ 0.0
pub fn days_since(dt: DateTime<Utc>) -> f64 {
    let now = Utc::now();
    let seconds = (now - dt).num_seconds() as f64;
    if seconds < 0.0 {
        0.0 // clock skew: treat future timestamps as fresh
    } else {
        seconds / SECONDS_PER_DAY
    }
}

// ── Memory life constants ───────────────────────────────────────────────────

/// Default memory life in days: 6 months × 30 days = 180 days.
///
/// After 180 days without recall, confidence decays to exp(-1) ≈ 36.8%.
/// Configurable via ServiceConfig.memory_life_days (admin setting).
///
/// Based on Wozniak & Gorzelanczyk (1995): R(t) = exp(-t/S) where S is
/// memory life in days.
pub const DEFAULT_MEMORY_LIFE_DAYS: f64 = 6.0 * 30.0; // 180 days

// ── Bayesian evidence pooling (log-odds) ───────────────────────────────────

/// Minimum confidence value used in log-odds computation to avoid ln(0).
const LOG_ODDS_EPSILON: f64 = 1e-6;

/// Combine two independent confidence estimates using log-odds (Bayesian) pooling.
///
/// Given two independent probability estimates `c₁` and `c₂` for the same
/// proposition, computes the combined posterior via log-odds addition:
///
/// ```text
/// L₁  = ln(c₁ / (1 − c₁))       // log-odds of source 1
/// L₂  = ln(c₂ / (1 − c₂))       // log-odds of source 2
/// L   = L₁ + L₂                 // additive: independent evidence
/// c   = 1 / (1 + exp(−L))       // convert back to probability
/// ```rust,no_run
///
/// # Properties
///
/// - **Consensus-strengthening:** c₁ = 0.8, c₂ = 0.8 → combined ≈ 0.941
/// - **Conflict-dampening:** c₁ = 0.8, c₂ = 0.2 → combined = 0.5
/// - **Low-evidence discounting:** c₁ = 0.51, c₂ = 0.51 → combined ≈ 0.520
/// - **High-certitude convergence:** c₁ = 0.99, c₂ = 0.99 → combined ≈ 0.9999
///
/// # Edge cases
///
/// Values of 0 and 1 are clamped to [ε, 1−ε] where ε = 10⁻⁶ to avoid
/// ln(0) and division-by-zero.
///
/// expect: "The system combines independent confidence estimates using Bayesian evidence pooling"
/// \[P3\] Motivating: Generative Space — fuses episodic and semantic evidence
/// \[P8\] Constraining: Semantic Grounding — log-odds pooling is the
///         mathematically correct method for combining independent probabilities
/// pre:  c₁, c₂ are in [0, 1]
/// post: returns Confidence in [0, 1]
/// post: combined ≥ max(c₁, c₂) when both > 0.5 (consensus strengthens)
/// post: combined = 0.5 when c₁ = 0.5 and c₂ = 0.5 (neutral evidence)
pub fn combine_confidences(c1: Confidence, c2: Confidence) -> Confidence {
    let v1 = c1.value().clamp(LOG_ODDS_EPSILON, 1.0 - LOG_ODDS_EPSILON);
    let v2 = c2.value().clamp(LOG_ODDS_EPSILON, 1.0 - LOG_ODDS_EPSILON);

    // Log-odds: logit(p) = ln(p / (1 − p))
    let log_odds_1 = (v1 / (1.0 - v1)).ln();
    let log_odds_2 = (v2 / (1.0 - v2)).ln();

    // Additive for independent evidence
    let combined_log_odds = log_odds_1 + log_odds_2;

    // Inverse logit: σ(x) = 1 / (1 + e⁻ˣ)
    let combined = 1.0 / (1.0 + (-combined_log_odds).exp());

    Confidence::new(combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consensus_strengthening() {
        let c = combine_confidences(Confidence::new(0.8), Confidence::new(0.8));
        assert!(c.value() > 0.93 && c.value() < 0.95);
    }

    #[test]
    fn conflict_dampening() {
        let c = combine_confidences(Confidence::new(0.8), Confidence::new(0.2));
        assert!((c.value() - 0.5).abs() < 0.001);
    }

    #[test]
    fn neutral_evidence_does_not_shift() {
        let c = combine_confidences(Confidence::new(0.5), Confidence::new(0.9));
        assert!((c.value() - 0.9).abs() < 0.001);
    }

    #[test]
    fn low_evidence_stays_low() {
        let c = combine_confidences(Confidence::new(0.51), Confidence::new(0.51));
        assert!(c.value() > 0.5 && c.value() < 0.53);
    }

    #[test]
    fn edge_case_zero_clamped() {
        let c = combine_confidences(Confidence::new(0.0), Confidence::new(0.8));
        assert!(c.value() < 1e-5);
    }

    #[test]
    fn edge_case_one_clamped() {
        let c = combine_confidences(Confidence::new(1.0), Confidence::new(0.8));
        assert!(c.value() > 0.9999);
    }

    #[test]
    fn high_both_saturates() {
        let c = combine_confidences(Confidence::new(0.99), Confidence::new(0.99));
        assert!(c.value() > 0.9998);
    }

    #[test]
    fn monotonic_both_increase() {
        let c = combine_confidences(Confidence::new(0.6), Confidence::new(0.7));
        assert!(c.value() >= 0.6);
        assert!(c.value() >= 0.7);
    }
}
