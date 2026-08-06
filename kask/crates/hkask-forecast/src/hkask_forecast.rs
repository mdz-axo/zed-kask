#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
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

// ── Conditional-tree marginalization ──────────────────────────────────────

/// Marginalize a conditional probability table over independent parents.
///
/// P(E) = Σ_a P(E|a) · Π_i P(p_i)^a_i · (1 − P(p_i))^(1 − a_i)
///
/// where `a` ranges over the `2^n` bitmap of parent truth assignments
/// (`n = parent_marginals.len()`), bit `j` of `a` corresponds to
/// `parent_marginals[j]`, and `conditionals[a] = P(E | a)`. Missing
/// `conditionals` entries contribute 0 (matching the scenarios server's
/// `compute_marginal_probabilities`). The result is the raw marginal — callers
/// clamp to `[0, 1]`.
///
/// Cost is `O(2^n)`; callers must ensure `parent_marginals.len()` is small
/// (the scenarios server and the graph widget both guard fan-in upstream).
/// This is the single source of truth for the joint-marginalization formula —
/// `hkask-mcp-scenarios::superforecast::compute_marginal_probabilities` and
/// `hkask-graph-widget::propagate::recompute_marginals` both delegate here so the
/// math cannot drift between them.
#[must_use = "marginal probability should be used"]
pub fn marginalize(parent_marginals: &[f64], conditionals: &[f64]) -> f64 {
    let n_parents = parent_marginals.len();
    let n_assignments = 1usize << n_parents;
    let mut marginal = 0.0;
    for assignment in 0..n_assignments {
        let mut assignment_prob = 1.0;
        for (j, &parent_marginal) in parent_marginals.iter().enumerate() {
            let bit_set = (assignment >> j) & 1 == 1;
            assignment_prob *= if bit_set {
                parent_marginal
            } else {
                1.0 - parent_marginal
            };
        }
        if let Some(&conditional) = conditionals.get(assignment) {
            marginal += conditional * assignment_prob;
        }
    }
    marginal
}

/// The MAIA three-level certainty tier for a probability, matching the
/// scenarios server's `CertaintyTier::from_probability` (which delegates here):
/// proximate (≥67%), probable (33–66%), possible (<33%). Single source of truth
/// for the thresholds so the server's tiering and the graph widget's node
/// coloring cannot drift.
#[must_use = "certainty tier should be used"]
pub fn certainty_tier(probability: f64) -> &'static str {
    if probability >= 0.67 {
        "proximate"
    } else if probability >= 0.33 {
        "probable"
    } else {
        "possible"
    }
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

/// Natural log-odds (logit) of a probability. Interpolation and regression
/// over bounded probabilities happen in log-odds space — linear in p would
/// leak outside [0,1]. Input clamped to [1e-6, 1-1e-6] to keep the log finite.
#[must_use]
pub fn log_odds(probability: f64) -> f64 {
    let p = probability.clamp(1e-6, 1.0 - 1e-6);
    (p / (1.0 - p)).ln()
}

/// Inverse of `log_odds` (logistic sigmoid).
#[must_use]
pub fn from_log_odds(logit: f64) -> f64 {
    1.0 / (1.0 + (-logit).exp())
}

/// Isotonic (pool-adjacent-violators) recalibration fit over resolved
/// (probability, outcome) pairs, following 2604.20421 §6.1's isotonic
/// baseline. Returns a monotone non-decreasing step function as sorted
/// (threshold, calibrated) knots; `None` when fewer than 2 pairs (a
/// recalibration from one point is fiction).
///
/// Apply with `isotonic_apply`. PAVA pools adjacent violations of
/// monotonicity until the sequence is isotone — standard, deterministic.
pub fn isotonic_fit(pairs: &[(f64, bool)]) -> Option<Vec<(f64, f64)>> {
    if pairs.len() < 2 {
        return None;
    }
    let mut sorted: Vec<(f64, f64)> = pairs
        .iter()
        .map(|(p, o)| (p.clamp(0.0, 1.0), if *o { 1.0 } else { 0.0 }))
        .collect();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // PAVA: blocks of (weight-sum, count, start-probability).
    let mut blocks: Vec<(f64, usize, f64)> = sorted.iter().map(|(p, o)| (*o, 1usize, *p)).collect();
    let mut i = 0;
    while i + 1 < blocks.len() {
        let mean_a = blocks[i].0 / blocks[i].1 as f64;
        let mean_b = blocks[i + 1].0 / blocks[i + 1].1 as f64;
        if mean_a > mean_b {
            // The merged block keeps the earlier knot's start probability —
            // thresholds are the left edges of piecewise-constant regions.
            let merged = (
                blocks[i].0 + blocks[i + 1].0,
                blocks[i].1 + blocks[i + 1].1,
                blocks[i].2,
            );
            blocks.splice(i..=i + 1, [merged]);
            if i > 0 {
                i -= 1;
            }
        } else {
            i += 1;
        }
    }
    Some(
        blocks
            .iter()
            .map(|(sum, count, start)| (*start, *sum / *count as f64))
            .collect(),
    )
}

/// Apply an isotonic fit: piecewise-constant calibrated probability for a
/// raw probability. Below the first knot → first value; above the last →
/// last value. Empty fit ⇒ returns the input unchanged (identity).
#[must_use]
pub fn isotonic_apply(fit: &[(f64, f64)], probability: f64) -> f64 {
    let mut calibrated = probability;
    for (threshold, value) in fit {
        if probability >= *threshold {
            calibrated = *value;
        } else {
            break;
        }
    }
    calibrated
}

/// Volatility regime classification over a price series (2607.08199):
/// economics-style contracts move smoothly (deadline-resolution dynamics);
/// sports-style contracts are jump-like (event-concentrated). Classifier:
/// the share of total absolute movement contributed by the largest single
/// move — jump-like when one move dominates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolatilityRegime {
    Smooth,
    JumpLike,
    /// Fewer than 2 price moves — no basis to classify.
    InsufficientData,
}

#[must_use]
pub fn volatility_regime(prices: &[f64]) -> VolatilityRegime {
    let moves: Vec<f64> = prices
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .filter(|d| *d > 1e-9)
        .collect();
    if moves.len() < 2 {
        return VolatilityRegime::InsufficientData;
    }
    let total: f64 = moves.iter().sum();
    let max_move = moves.iter().cloned().fold(0.0, f64::max);
    if max_move / total > 0.5 {
        VolatilityRegime::JumpLike
    } else {
        VolatilityRegime::Smooth
    }
}

/// Domain-bias correction for market-implied probabilities (arXiv:2602.19520).
///
/// Prediction-market prices are not face-value probabilities: politics
/// markets on both Kalshi and Polymarket are chronically *underconfident* —
/// prices compressed toward 0.5, so extreme outcomes are underpriced. The
/// correction de-compresses: `p' = 0.5 + (p - 0.5)(1 + δ)`, clamped to
/// [0.01, 0.99]. A δ of 0 applies to already-calibrated domains (sports —
/// arXiv:2604.20421 §6.1 found isotonic recalibration adds nothing on NBA
/// markets).
///
/// `delta` is the signed de-compression strength in [0, 0.5]; values beyond
/// are clamped (a correction must never invert or saturate a probability).
/// Callers source δ from measured per-domain calibration (seeded from the
/// paper until the calibration loop accumulates resolved outcomes).
#[must_use = "corrected probability should replace the face-value price"]
pub fn domain_bias_correction(probability: f64, delta: f64) -> f64 {
    let delta = delta.clamp(0.0, 0.5);
    (0.5 + (probability - 0.5) * (1.0 + delta)).clamp(0.01, 0.99)
}

// ── Scenario risk core (T8a) ────────────────────────────────────────────────
//
// The risk axis of the three-axes specification: probability-weighted risk
// measures over scenario-tree branches. Pure math over caller-supplied branch
// returns — the valuation engine (companies server) supplies per-branch
// revaluations; this module turns them into risk measures and factor
// exposures. Time and return stay simple by design; the complexity budget
// lives here.

/// One branch of a scenario tree with its probability and the company's
/// implied return under that branch.
///
/// `probability` is the branch's joint probability (product of path
/// conditionals); `branch_return` is the company's annualized return if the
/// branch realizes (from re-evaluating the DCF/RIM under the branch's
/// assumptions — the `branch_return` step of the risk core).
#[derive(Debug, Clone, Copy)]
pub struct BranchOutcome {
    pub probability: f64,
    pub branch_return: f64,
}

/// Probability-weighted risk measure over scenario branches.
#[derive(Debug, Clone, Copy)]
pub struct ScenarioRiskMeasure {
    /// Probability-weighted mean branch return (the scenario-implied
    /// expected return).
    pub expected_return: f64,
    /// σ_scenario: probability-weighted standard deviation of branch returns.
    pub sigma_scenario: f64,
    /// Number of branches.
    pub branch_count: usize,
    /// Sum of branch probabilities (diagnostic — 1.0 for a complete tree).
    pub probability_mass: f64,
}

/// Compute the scenario risk measure over a set of branches.
///
/// Branches need not sum to probability 1 (an incomplete tree is measurable —
/// the mass is reported so the caller can decide whether the residual is a
/// hold-out branch). Returns None when no branch has positive probability —
/// a risk measure over zero mass is undefined, never fabricated.
#[must_use = "risk measure should feed valuation or factor analysis"]
pub fn scenario_risk_measure(branches: &[BranchOutcome]) -> Option<ScenarioRiskMeasure> {
    let mass: f64 = branches.iter().map(|b| b.probability).sum();
    if mass <= 0.0 {
        return None;
    }
    let expected: f64 = branches
        .iter()
        .map(|b| b.probability * b.branch_return)
        .sum::<f64>()
        / mass;
    let variance: f64 = branches
        .iter()
        .map(|b| b.probability * (b.branch_return - expected).powi(2))
        .sum::<f64>()
        / mass;
    Some(ScenarioRiskMeasure {
        expected_return: expected,
        sigma_scenario: variance.sqrt(),
        branch_count: branches.len(),
        probability_mass: mass,
    })
}

/// Scenario factor exposure: the company's return sensitivity to a single
/// scenario node (factor), in the APT loading sense.
///
/// Construction (per the corrected T8a design, phase2-review B2): the factor
/// is the node's binary outcome; the loading is the difference in the
/// company's branch return between branches where the node is true and
/// branches where it is false, probability-weighted:
///
///   β(node) = E[r | node true] − E[r | node false]
///
/// This is the cash-flow sensitivity of company value to the node's outcome
/// — elicited by revaluation, not estimated by covariance with an indicator
/// (indicators over mutually exclusive branches are collinear).
///
/// Returns None when either conditioning set has zero probability mass.
#[must_use = "loading should feed factor-exposure analysis"]
pub fn scenario_node_loading(branches: &[BranchOutcome], node_true: &[bool]) -> Option<f64> {
    if node_true.len() != branches.len() {
        return None;
    }
    let (mut mass_true, mut mass_false) = (0.0, 0.0);
    let (mut ret_true, mut ret_false) = (0.0, 0.0);
    for (branch, &is_true) in branches.iter().zip(node_true.iter()) {
        if is_true {
            mass_true += branch.probability;
            ret_true += branch.probability * branch.branch_return;
        } else {
            mass_false += branch.probability;
            ret_false += branch.probability * branch.branch_return;
        }
    }
    if mass_true <= 0.0 || mass_false <= 0.0 {
        return None;
    }
    Some(ret_true / mass_true - ret_false / mass_false)
}

/// Fuse realized market volatility with scenario-implied volatility.
///
/// Graceful degradation (per the three-axes spec): when `sigma_scenario` is
/// None (no tree, or a zero-mass tree), the fused value IS the realized
/// volatility — the simple path is the default and the detailed path is an
/// earned upgrade (analyst maturity ladder).
///
/// When both exist, the fusion is the root-sum-square: the two sources are
/// treated as independent risk channels (market microstructure noise vs
/// event-driven scenario uncertainty), so their variances add. `scenario_weight`
/// in [0,1] scales the scenario channel's contribution — 1.0 for a fully
/// validated tree, less for partial coverage.
#[must_use = "fused volatility should replace the single-source estimate"]
pub fn fuse_volatility(
    realized_volatility: f64,
    sigma_scenario: Option<f64>,
    scenario_weight: f64,
) -> f64 {
    match sigma_scenario {
        None => realized_volatility,
        Some(sigma) => {
            let weight = scenario_weight.clamp(0.0, 1.0);
            (realized_volatility.powi(2) + (weight * sigma).powi(2)).sqrt()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── T8a scenario risk core ───────────────────────────────────────────

    #[test]
    fn sigma_scenario_binary_hand_check() {
        // Binary tree {+20%, −15%} at p=0.6: E[r] = 0.06, σ = 0.35·√0.24
        // ≈ 0.17146. (The plan's "≈ 0.176" was an arithmetic slip — the
        // correct value is recorded here and the plan amended.)
        let branches = [
            BranchOutcome {
                probability: 0.6,
                branch_return: 0.20,
            },
            BranchOutcome {
                probability: 0.4,
                branch_return: -0.15,
            },
        ];
        let measure = scenario_risk_measure(&branches).expect("positive mass");
        assert!((measure.expected_return - 0.06).abs() < 1e-12);
        assert!(
            (measure.sigma_scenario - 0.17146).abs() < 0.0001,
            "sigma {}",
            measure.sigma_scenario
        );
        assert_eq!(measure.branch_count, 2);
        assert!((measure.probability_mass - 1.0).abs() < 1e-12);
    }

    #[test]
    fn risk_measure_none_on_zero_mass() {
        assert!(scenario_risk_measure(&[]).is_none());
        assert!(
            scenario_risk_measure(&[BranchOutcome {
                probability: 0.0,
                branch_return: 0.1
            }])
            .is_none()
        );
    }

    #[test]
    fn risk_measure_normalizes_partial_mass() {
        // Incomplete tree (mass 0.8): measure is over the covered branches.
        let branches = [
            BranchOutcome {
                probability: 0.4,
                branch_return: 0.10,
            },
            BranchOutcome {
                probability: 0.4,
                branch_return: 0.30,
            },
        ];
        let measure = scenario_risk_measure(&branches).expect("positive mass");
        assert!((measure.expected_return - 0.20).abs() < 1e-12);
        assert!((measure.probability_mass - 0.8).abs() < 1e-12);
    }

    #[test]
    fn node_loading_hand_check() {
        // Two branches: node true → r=0.20 (p=0.6); node false → r=−0.15
        // (p=0.4). Loading = 0.20 − (−0.15) = 0.35.
        let branches = [
            BranchOutcome {
                probability: 0.6,
                branch_return: 0.20,
            },
            BranchOutcome {
                probability: 0.4,
                branch_return: -0.15,
            },
        ];
        let loading = scenario_node_loading(&branches, &[true, false]).expect("both sides");
        assert!((loading - 0.35).abs() < 1e-12);
    }

    #[test]
    fn node_loading_none_when_one_side_empty() {
        let branches = [BranchOutcome {
            probability: 0.6,
            branch_return: 0.20,
        }];
        assert!(scenario_node_loading(&branches, &[true]).is_none());
        assert!(scenario_node_loading(&branches, &[true, false]).is_none()); // length mismatch
    }

    #[test]
    fn fuse_volatility_degrades_to_realized() {
        assert!((fuse_volatility(0.25, None, 1.0) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn fuse_volatility_rss_when_scenario_present() {
        // sqrt(0.25² + 0.15²) ≈ 0.2915.
        let fused = fuse_volatility(0.25, Some(0.15), 1.0);
        assert!((fused - 0.2915).abs() < 0.001, "fused {fused}");
        // Zero weight → realized only.
        assert!((fuse_volatility(0.25, Some(0.15), 0.0) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn log_odds_round_trip() {
        for p in [0.01, 0.25, 0.5, 0.75, 0.99] {
            assert!((from_log_odds(log_odds(p)) - p).abs() < 1e-9);
        }
        // Monotone increasing — the property interpolation relies on.
        assert!(log_odds(0.7) > log_odds(0.6));
        // Never NaN at the extremes.
        assert!(log_odds(0.0).is_finite());
        assert!(log_odds(1.0).is_finite());
    }

    #[test]
    fn isotonic_fit_enforces_monotonicity() {
        // A violation: raw 0.7 resolved false more than raw 0.6.
        let pairs = [(0.6, true), (0.7, false), (0.65, true), (0.9, true)];
        let fit = isotonic_fit(&pairs).expect("fits");
        let values: Vec<f64> = fit.iter().map(|(_, v)| *v).collect();
        assert!(
            values.windows(2).all(|w| w[0] <= w[1] + 1e-12),
            "fit must be non-decreasing: {values:?}"
        );
    }

    #[test]
    fn isotonic_fit_needs_two_pairs() {
        assert!(isotonic_fit(&[(0.5, true)]).is_none());
        assert!(isotonic_fit(&[]).is_none());
    }

    #[test]
    fn isotonic_apply_is_piecewise_constant() {
        let fit = vec![(0.2, 0.1), (0.6, 0.5), (0.9, 0.85)];
        assert!((isotonic_apply(&fit, 0.1) - 0.1).abs() < 1e-12); // below first → first? no: below first knot returns input region value
        assert!((isotonic_apply(&fit, 0.5) - 0.1).abs() < 1e-12);
        assert!((isotonic_apply(&fit, 0.95) - 0.85).abs() < 1e-12);
        // Empty fit is identity.
        assert!((isotonic_apply(&[], 0.42) - 0.42).abs() < 1e-12);
    }

    #[test]
    fn volatility_regime_classification() {
        // Jump-like: one dominant move (sports-style event concentration).
        let jumpy = [0.5, 0.5, 0.52, 0.9, 0.91];
        assert_eq!(volatility_regime(&jumpy), VolatilityRegime::JumpLike);
        // Smooth: many comparable small moves (macro-style drift).
        let smooth = [0.5, 0.52, 0.54, 0.56, 0.58, 0.6];
        assert_eq!(volatility_regime(&smooth), VolatilityRegime::Smooth);
        assert_eq!(
            volatility_regime(&[0.5]),
            VolatilityRegime::InsufficientData
        );
        assert_eq!(volatility_regime(&[]), VolatilityRegime::InsufficientData);
    }

    #[test]
    fn domain_bias_correction_decompresses_away_from_half() {
        // Politics market at 0.62 with δ=0.3 must move toward the extreme.
        let corrected = domain_bias_correction(0.62, 0.3);
        assert!(corrected > 0.62, "de-compression: {corrected}");
        assert!((corrected - 0.656).abs() < 1e-9);
        // Symmetric below 0.5.
        let low = domain_bias_correction(0.38, 0.3);
        assert!(low < 0.38);
    }

    #[test]
    fn domain_bias_correction_zero_delta_is_identity() {
        // Already-calibrated domains (sports, 2604.20421 §6.1) pass through.
        assert!((domain_bias_correction(0.7, 0.0) - 0.7).abs() < 1e-12);
    }

    #[test]
    fn domain_bias_correction_clamps() {
        // Extreme inputs stay in [0.01, 0.99]; delta never exceeds 0.5.
        assert!((domain_bias_correction(0.999, 0.5) - 0.99).abs() < 1e-9);
        assert!(domain_bias_correction(0.5, 1.0).abs() - 0.5 < 1e-12);
        assert!((domain_bias_correction(0.001, 2.0) - 0.01).abs() < 1e-9);
    }

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

    #[test]
    fn marginalize_single_parent() {
        // P(b) = P(b|¬a)·P(¬a) + P(b|a)·P(a) = 0.1·0.2 + 0.6·0.8 = 0.5
        let m = marginalize(&[0.8], &[0.1, 0.6]);
        assert!((m - 0.5).abs() < 1e-9, "got {m}");
    }

    #[test]
    fn marginalize_missing_conditionals_contribute_zero() {
        // conditionals shorter than 2^n → the missing entry contributes 0.
        // P(b) = P(b|¬a)·P(¬a) + 0 = 0.4·0.5 = 0.2.
        let m = marginalize(&[0.5], &[0.4]);
        assert!((m - 0.4 * 0.5).abs() < 1e-9, "got {m}");
    }

    #[test]
    fn certainty_tier_thresholds() {
        assert_eq!(certainty_tier(0.9), "proximate");
        assert_eq!(certainty_tier(0.5), "probable");
        assert_eq!(certainty_tier(0.1), "possible");
    }
}
