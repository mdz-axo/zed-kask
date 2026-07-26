#![forbid(unsafe_code)]
//! Shared superforecasting computation engine (Tetlock GJP methodology).
//!
//! Canonical implementations used by both `hkask-mcp-scenarios` and
//! `hkask-mcp-companies`. No MCP or server dependencies — pure math.
//!
//! Pipeline:
//! 1. Fermi decomposition — confidence-weighted sub-question averaging
//! 2. Outside view — base rate calibration with shrinkage estimator
//! 3. Bayesian updating — P(H|E) = P(E|H) × P(H) / P(E)
//! 4. Brier scoring — (prediction - outcome)²

use thiserror::Error;

// ── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ForecastError {
    #[error("probability {0} not in [0, 1] for '{1}'")]
    InvalidProbability(f64, String),

    #[error("brier: probabilities and outcomes have different lengths ({0} vs {1})")]
    BrierLengthMismatch(usize, usize),

    #[error("brier: no data provided")]
    BrierNoData,
}

// ── Sub-question type (minimal — no serde dependency needed here) ───────────

/// A Fermi sub-question with an estimate and confidence weight.
#[derive(Debug, Clone)]
pub struct FermiQuestion {
    pub question: String,
    pub estimate: f64,
    pub confidence: f64,
}

impl FermiQuestion {
    pub fn new(question: String, estimate: f64, confidence: f64) -> Self {
        Self {
            question,
            estimate,
            confidence,
        }
    }
}

// ── Fermi decomposition ────────────────────────────────────────────────────

/// Fermi decomposition calibration. Weighted average of sub-question
/// estimates by confidence. Returns Err if any sub-question has a non-finite
/// estimate or confidence, or if all confidence weights are zero.
/// Returns Ok(0.5) if sub_questions is empty (neutral prior).
#[must_use = "calibration result should be used or the error handled"]
pub fn calibrate_from_fermi(questions: &[FermiQuestion]) -> Result<f64, ForecastError> {
    if questions.is_empty() {
        return Ok(0.5);
    }
    for q in questions {
        if !q.estimate.is_finite() || !(0.0..=1.0).contains(&q.estimate) {
            return Err(ForecastError::InvalidProbability(
                q.estimate,
                q.question.clone(),
            ));
        }
        if !q.confidence.is_finite() || !(0.0..=1.0).contains(&q.confidence) {
            return Err(ForecastError::InvalidProbability(
                q.confidence,
                format!("confidence for '{}'", q.question),
            ));
        }
    }
    let total_weight: f64 = questions.iter().map(|q| q.confidence).sum();
    if total_weight == 0.0 {
        return Ok(0.5);
    }
    let weighted_sum: f64 = questions.iter().map(|q| q.estimate * q.confidence).sum();
    Ok(weighted_sum / total_weight)
}

// ── Outside view (base rate calibration) ───────────────────────────────────

/// Blend inside-view estimate with outside-view base rate using a shrinkage
/// estimator. Higher reference_count → more weight on the outside view.
/// Returns (calibrated_probability, confidence).
#[must_use = "adjustment result should be used"]
pub fn outside_view_adjustment(
    base_rate: f64,
    inside_estimate: f64,
    reference_count: u64,
) -> (f64, f64) {
    // Regression toward the mean: the less reference data, the more we
    // regress toward 0.5 (the uninformative prior).
    let shrinkage = 1.0 / (1.0 + reference_count as f64);
    let regressed_base = 0.5 + (1.0 - shrinkage) * (base_rate - 0.5);

    // Blend outside and inside view. Outside view gets more weight
    // when the reference count is high.
    let outside_weight = (reference_count as f64 / (reference_count as f64 + 3.0)).min(0.8);
    let calibrated = regressed_base * outside_weight + inside_estimate * (1.0 - outside_weight);

    let confidence = 0.5 + 0.3 * outside_weight;

    (calibrated, confidence)
}

// ── Bayesian updating ──────────────────────────────────────────────────────

/// Standard Bayesian update: posterior = prior × likelihood / evidence_rate.
#[must_use = "posterior probability should be used"]
pub fn bayesian_update(prior: f64, evidence_likelihood: f64, evidence_base_rate: f64) -> f64 {
    (evidence_likelihood * prior / evidence_base_rate).clamp(0.01, 0.99)
}

// ── Brier scoring ──────────────────────────────────────────────────────────

/// Brier score for a single binary forecast: (prediction - outcome)².
#[must_use = "score should be used or recorded"]
pub fn brier_score(probability: f64, outcome_occurred: bool) -> f64 {
    (probability - if outcome_occurred { 1.0 } else { 0.0 }).powi(2)
}

/// Average Brier score across multiple binary forecasts.
#[must_use = "score should be used or recorded"]
pub fn brier_score_multi(probabilities: &[f64], outcomes: &[bool]) -> Result<f64, ForecastError> {
    if probabilities.is_empty() {
        return Err(ForecastError::BrierNoData);
    }
    if probabilities.len() != outcomes.len() {
        return Err(ForecastError::BrierLengthMismatch(
            probabilities.len(),
            outcomes.len(),
        ));
    }
    Ok(probabilities
        .iter()
        .zip(outcomes.iter())
        .map(|(p, o)| brier_score(*p, *o))
        .sum::<f64>()
        / probabilities.len() as f64)
}

/// Human-readable Brier score interpretation.
#[must_use]
pub fn brier_interpretation(score: f64) -> &'static str {
    if score < 0.05 {
        "excellent"
    } else if score < 0.10 {
        "good"
    } else if score < 0.20 {
        "fair"
    } else if score < 0.33 {
        "poor"
    } else {
        "worse_than_climatology"
    }
}

// ── Calibration feedback ─────────────────────────────────────────────────────

/// Adjust a prior probability using a calibration bias signal from a
/// historical Brier-scored calibration curve. Closes the Tetlock feedback
/// loop: record → Brier score → calibration curve → adjust next prior.
///
/// `overconfidence_bias` is the signed mean (expected_rate − hit_rate) across
/// calibration bins (positive = systematically overconfident, negative =
/// underconfident), typically from
/// `hkask-mcp-scenarios::compute_calibration_curve.overconfidence_score`.
///
/// The adjustment regresses the prior toward the uninformative 0.5 anchor
/// proportionally to the bias: an overconfident forecaster's extreme
/// predictions are pulled toward 0.5; an underconfident forecaster's are
/// pushed slightly away. The bias influence is clamped to ±0.5 so a single
/// unreliable curve cannot invert a forecast.
#[must_use = "calibrated prior should be used for the next forecast"]
pub fn apply_calibration_adjustment(prior: f64, overconfidence_bias: f64) -> f64 {
    let influence = overconfidence_bias.clamp(-0.5, 0.5);
    let adjusted = prior - influence * (prior - 0.5);
    adjusted.clamp(0.01, 0.99)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fermi_simple() {
        let qs = vec![
            FermiQuestion::new("a".into(), 0.8, 0.9),
            FermiQuestion::new("b".into(), 0.2, 0.1),
        ];
        let r = calibrate_from_fermi(&qs).unwrap();
        assert!((r - 0.74).abs() < 0.001);
    }

    #[test]
    fn fermi_empty() {
        assert_eq!(calibrate_from_fermi(&[]).unwrap(), 0.5);
    }

    #[test]
    fn fermi_nan_rejected() {
        let qs = vec![FermiQuestion::new("nan".into(), f64::NAN, 0.5)];
        assert!(calibrate_from_fermi(&qs).is_err());
    }

    #[test]
    fn fermi_out_of_range_values_are_rejected() {
        let invalid_estimate = vec![FermiQuestion::new("estimate".into(), 1.1, 0.5)];
        let invalid_confidence = vec![FermiQuestion::new("confidence".into(), 0.5, -0.1)];

        assert!(calibrate_from_fermi(&invalid_estimate).is_err());
        assert!(calibrate_from_fermi(&invalid_confidence).is_err());
    }

    #[test]
    fn outside_view_high_ref() {
        let (p, c) = outside_view_adjustment(0.7, 0.3, 1000);
        assert!(p > 0.6);
        assert!(c > 0.7);
    }

    #[test]
    fn bayesian_positive() {
        let p = bayesian_update(0.3, 0.9, 0.3);
        assert!((p - 0.9).abs() < 0.01);
    }

    #[test]
    fn brier_perfect() {
        assert_eq!(brier_score(1.0, true), 0.0);
    }

    #[test]
    fn brier_interpretation_excellent() {
        assert_eq!(brier_interpretation(0.03), "excellent");
    }

    #[test]
    fn calibration_adjustment_regresses_overconfident() {
        // Overconfident (bias 0.3): an 0.9 prior should regress toward 0.5.
        let adjusted = apply_calibration_adjustment(0.9, 0.3);
        assert!(
            adjusted < 0.9 && adjusted > 0.5,
            "overconfident extreme regresses toward 0.5"
        );
    }

    #[test]
    fn calibration_adjustment_neutral_bias_unchanged() {
        let adjusted = apply_calibration_adjustment(0.7, 0.0);
        assert!(
            (adjusted - 0.7).abs() < 1e-9,
            "zero bias leaves prior unchanged"
        );
    }

    #[test]
    fn calibration_adjustment_underconfident_pushes_outward() {
        // Underconfident (bias -0.2): a 0.6 prior should push slightly above 0.6.
        let adjusted = apply_calibration_adjustment(0.6, -0.2);
        assert!(
            adjusted > 0.6,
            "underconfident bias pushes prediction outward"
        );
    }

    #[test]
    fn calibration_adjustment_clamps_influence() {
        // An extreme bias (2.0) is clamped to 0.5; an 0.8 prior regresses to
        // 0.8 - 0.5*(0.8-0.5) = 0.65, not inverted.
        let adjusted = apply_calibration_adjustment(0.8, 2.0);
        assert!(
            (adjusted - 0.65).abs() < 1e-9,
            "extreme bias clamped to 0.5 influence"
        );
    }
}
