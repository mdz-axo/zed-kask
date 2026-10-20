#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Shared superforecasting computation engine (Tetlock GJP methodology).
//!
//! Canonical implementations used by both `hkask-mcp-scenarios` and
//! the domain MCP servers (`hkask-mcp-scenarios`). No MCP or server dependencies — pure math.
//!
//! Pipeline:
//! 1. Fermi decomposition — confidence-weighted sub-question averaging
//! 2. Outside view — base rate calibration with shrinkage estimator
//! 3. Bayesian updating — P(H|E) = P(E|H) × P(H) / P(E)
//! 4. Brier scoring — (prediction - outcome)²

use thiserror::Error;

// ── Posterior engine (promoted from hkask-graph-widget, PR-08) ──────────
// The shared evidence-posterior engine: the scenarios server's
// `scenario_recompute_posteriors` tool and the graph widget's interactive
// re-propagation both delegate here — one implementation, no drift.
pub mod posterior;
pub use posterior::{
    Evidence, PosteriorDependency, PosteriorNode, forward_marginals, is_polytree,
    recompute_posteriors,
};

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
///
/// If `evidence_base_rate` is zero or negative, the Bayesian ratio is undefined
/// (division by zero yields ±∞ or `NaN`; a negative base rate yields a
/// sign-flipped posterior that `clamp` would push to 0.01 — the wrong answer
/// for impossible evidence). This is treated as "evidence is impossible under
/// the model": the prior is returned unchanged (clamped) rather than letting
/// `NaN` or a nonsensical negative posterior escape. Callers should pass a
/// genuine positive base rate; the guard is a fail-safe, not a license to pass
/// zero or negative.
#[must_use = "posterior probability should be used"]
pub fn bayesian_update(prior: f64, evidence_likelihood: f64, evidence_base_rate: f64) -> f64 {
    if evidence_base_rate <= 0.0 {
        return prior.clamp(0.01, 0.99);
    }
    (evidence_likelihood * prior / evidence_base_rate).clamp(0.01, 0.99)
}

// ── Conditional-tree marginalization ────────────────────────────────────────

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

/// Noisy-OR combination of independent causal channels:
/// `P(E) = 1 − Π_g (1 − P_g(E))`.
///
/// For a single channel this is (approximately) the identity: `1−(1−p)`
/// differs from `p` by at most 1 ULP in IEEE-754. The scenarios server's
/// multi-group marginalization and the posterior engine's group
/// combination share this one implementation (PR-08) so the interactive
/// re-propagation cannot drift from `scenario_quantify`.
#[must_use = "combined probability should be used"]
pub fn combine_independent_channels(channel_probabilities: &[f64]) -> f64 {
    let survival = channel_probabilities
        .iter()
        .fold(1.0, |acc, &probability| acc * (1.0 - probability));
    1.0 - survival
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

/// Brier loss for a single binary forecast: (prediction - outcome)².
/// The caller must supply finite p in [0,1]; `brier_score_multi` validates it.
/// Raw loss alone does not establish calibration, skill or usefulness.
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
    for (index, probability) in probabilities.iter().copied().enumerate() {
        if !(0.0..=1.0).contains(&probability) {
            return Err(ForecastError::InvalidProbability(
                probability,
                format!("forecast {index}"),
            ));
        }
    }
    Ok(probabilities
        .iter()
        .zip(outcomes.iter())
        .map(|(p, o)| brier_score(*p, *o))
        .sum::<f64>()
        / probabilities.len() as f64)
}

// ── Hit-rate intervals ──────────────────────────────────────────────────────

/// Minimum resolved sample size before a bare hit-rate percentage may be
/// quoted as a headline. Below this the point estimate is unquotable without
/// its Wilson interval — "49% accurate" from three forecasts is noise
/// wearing a number (the Hodgson trap: a best result of 49% with a 95% CI of
/// [0, 100]).
pub const MIN_N_FOR_HEADLINE: usize = 30;

/// Wilson score interval (95%) for a binomial hit rate: `hits` successes in
/// `trials` attempts. Returns `None` when `trials == 0` — an absent sample is
/// not a zero rate. Served alongside every hit-rate percentage so the
/// estimate is never quoted without its uncertainty.
#[must_use = "an interval without its rate is as unquotable as a rate without its interval"]
pub fn wilson_bounds(hits: usize, trials: usize) -> Option<(f64, f64)> {
    if trials == 0 {
        return None;
    }
    // z = 1.95996 for a 95% two-sided interval.
    let z = 1.95996;
    let z_squared = z * z;
    let n = trials as f64;
    let p_hat = hits as f64 / n;
    let denominator = 1.0 + z_squared / n;
    let centre = p_hat + z_squared / (2.0 * n);
    let spread = z * (p_hat * (1.0 - p_hat) / n + z_squared / (4.0 * n * n)).sqrt();
    Some((
        ((centre - spread) / denominator).max(0.0),
        ((centre + spread) / denominator).min(1.0),
    ))
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
    // Below the first knot the step function extends leftward at the first
    // region's value (documented contract) — not the raw input, which would
    // leave sub-threshold probabilities uncalibrated.
    let Some(&(_, first_value)) = fit.first() else {
        return probability;
    };
    let mut calibrated = first_value;
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

// ── Scenario risk core ────────────────────────────────────────────────
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
/// Construction: the factor is the node's binary outcome; the loading is the difference in the
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
mod property_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    // ── Hit-rate intervals ─────────────────────────────────────────

    #[test]
    fn an_absent_sample_is_not_a_zero_rate() {
        assert_eq!(wilson_bounds(0, 0), None);
    }

    #[test]
    fn a_single_hit_is_not_a_hundred_percent_headline() {
        // 1 of 1: the Wilson interval is [0.21, 1.00] — quoting "100%"
        // from one trial is the Hodgson trap this function exists to end.
        let (low, high) = wilson_bounds(1, 1).expect("n=1 has bounds");
        assert!(close(low, 0.206549981));
        assert!(close(high, 1.0));
    }

    #[test]
    fn wilson_bounds_bracket_a_small_sample() {
        // 5 of 10: [0.237, 0.763] — the interval is what makes a small
        // bin quotable, not the 50% point estimate.
        let (low, high) = wilson_bounds(5, 10).expect("n=10 has bounds");
        assert!(close(low, 0.236593477));
        assert!(close(high, 0.763406523));
    }

    #[test]
    fn wilson_bounds_narrow_as_the_sample_grows() {
        let (small_low, small_high) = wilson_bounds(15, 30).expect("n=30 has bounds");
        let (large_low, large_high) = wilson_bounds(150, 300).expect("n=300 has bounds");
        assert!(large_high - large_low < small_high - small_low);
        assert!(large_low > small_low);
        assert!(large_high < small_high);
    }

    #[test]
    fn the_headline_gate_refuses_small_samples() {
        // The gate is the const itself: a headline percentage below this
        // count is unquotable without its interval.
        assert_eq!(MIN_N_FOR_HEADLINE, 30);
    }

    // ── Brier scoring ───────────────────────────────────────────────

    #[test]
    fn brier_multi_rejects_nonfinite_and_out_of_range_probabilities() {
        for probability in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            assert!(
                brier_score_multi(&[probability], &[true]).is_err(),
                "invalid p: {probability}"
            );
        }
    }

    #[test]
    fn brier_perfect_and_worst() {
        assert_eq!(brier_score(1.0, true), 0.0);
        assert_eq!(brier_score(0.0, false), 0.0);
        assert_eq!(brier_score(0.0, true), 1.0);
        assert_eq!(brier_score(1.0, false), 1.0);
    }

    #[test]
    fn brier_is_quadratic_penalty() {
        // 0.8 predicted, occurred → (0.2)² = 0.04.
        assert!(close(brier_score(0.8, true), 0.04));
        // 0.5 is the no-skill anchor regardless of outcome.
        assert!(close(brier_score(0.5, true), 0.25));
        assert!(close(brier_score(0.5, false), 0.25));
    }

    #[test]
    fn brier_multi_mean_is_exact() {
        // (0.8,true) → (0.2)² = 0.04; (0.4,false) → (0.4)² = 0.16.
        // Mean = (0.04 + 0.16) / 2 = 0.10.
        let score = brier_score_multi(&[0.8, 0.4], &[true, false]).unwrap();
        assert!(close(score, 0.10));
    }

    #[test]
    fn brier_multi_rejects_empty_and_mismatched() {
        assert!(matches!(
            brier_score_multi(&[], &[]),
            Err(ForecastError::BrierNoData)
        ));
        assert!(matches!(
            brier_score_multi(&[0.5], &[]),
            Err(ForecastError::BrierLengthMismatch(1, 0))
        ));
    }

    // ── Bayesian update ─────────────────────────────────────────────

    #[test]
    fn bayesian_update_computes_odds_ratio() {
        // prior 0.5, likelihood 0.8, base rate 0.4 → 0.8·0.5/0.4 = 1.0.
        assert!(close(bayesian_update(0.5, 0.8, 0.4), 1.0_f64.min(0.99)));
        // prior 0.3, likelihood 0.2, base rate 0.5 → 0.12.
        assert!(close(bayesian_update(0.3, 0.2, 0.5), 0.12));
    }

    #[test]
    fn bayesian_update_clamps_posterior_to_open_interval() {
        // Extreme inputs must not produce exactly 0 or 1.
        let posterior = bayesian_update(0.99, 1.0, 0.01);
        assert!((0.01..=0.99).contains(&posterior));
    }

    #[test]
    fn bayesian_update_zero_base_rate_returns_clamped_prior_not_nan() {
        // The fail-safe guard: impossible evidence returns the clamped prior,
        // never NaN or a sign-flipped value.
        let posterior = bayesian_update(0.7, 0.9, 0.0);
        assert!(close(posterior, 0.7));
        assert!(!posterior.is_nan());
        let negative = bayesian_update(0.7, 0.9, -0.5);
        assert!(close(negative, 0.7));
    }

    // ── Fermi decomposition ─────────────────────────────────────────

    #[test]
    fn fermi_empty_set_yields_neutral_prior() {
        assert!(close(calibrate_from_fermi(&[]).unwrap(), 0.5));
    }

    #[test]
    fn fermi_weights_by_confidence() {
        let questions = vec![
            FermiQuestion::new("a".into(), 0.9, 0.9),
            FermiQuestion::new("b".into(), 0.1, 0.1),
        ];
        // Weighted mean: (0.81 + 0.01) / 1.0 = 0.82 — dominated by the
        // high-confidence sub-question.
        let calibrated = calibrate_from_fermi(&questions).unwrap();
        assert!(close(calibrated, 0.82));
    }

    #[test]
    fn fermi_all_zero_confidence_yields_neutral_prior() {
        let questions = vec![
            FermiQuestion::new("a".into(), 0.9, 0.0),
            FermiQuestion::new("b".into(), 0.1, 0.0),
        ];
        assert!(close(calibrate_from_fermi(&questions).unwrap(), 0.5));
    }

    #[test]
    fn fermi_rejects_out_of_range_inputs() {
        let bad_estimate = vec![FermiQuestion::new("x".into(), 1.5, 0.5)];
        assert!(matches!(
            calibrate_from_fermi(&bad_estimate),
            Err(ForecastError::InvalidProbability(..))
        ));
        let bad_confidence = vec![FermiQuestion::new("x".into(), 0.5, -0.1)];
        assert!(calibrate_from_fermi(&bad_confidence).is_err());
    }

    // ── Outside view ────────────────────────────────────────────────

    #[test]
    fn outside_view_more_references_pull_toward_base_rate() {
        let few = outside_view_adjustment(0.9, 0.2, 1);
        let many = outside_view_adjustment(0.9, 0.2, 100);
        // With many references the estimate converges toward the base rate.
        assert!(
            many.0 > few.0,
            "more references should raise toward base rate"
        );
        assert!(many.0 <= 0.9 + 1e-9);
    }

    // ── Marginalization ─────────────────────────────────────────────

    #[test]
    fn marginalize_single_parent_matches_definition() {
        // Bitmap convention: bit j set = parent j TRUE. With one parent at
        // p=0.4: assignment 0 (false) has weight 0.6 and reads conditionals[0];
        // assignment 1 (true) has weight 0.4 and reads conditionals[1].
        let marginal = marginalize(&[0.4], &[0.9, 0.1]);
        assert!(close(marginal, 0.9 * 0.6 + 0.1 * 0.4));
    }

    #[test]
    fn marginalize_two_parents_enumerates_full_joint() {
        // AND-gate: conditional is 1 only when both parents true (bitmap 11 = 3).
        let p = marginalize(&[0.5, 0.5], &[0.0, 0.0, 0.0, 1.0]);
        assert!(close(p, 0.25));

        // OR-gate: 1 unless both parents false (bitmap 00).
        let q = marginalize(&[0.5, 0.5], &[0.0, 1.0, 1.0, 1.0]);
        assert!(close(q, 0.75));
    }

    #[test]
    fn marginalize_short_table_contributes_zero_for_missing_entries() {
        // Only bitmap-1 entry present: the bitmap-0 contribution is dropped.
        let with_both = marginalize(&[0.4], &[0.1, 0.9]);
        let first_only = marginalize(&[0.4], &[0.1]);
        // With parent at 0.4: full = 0.1·0.6 + 0.9·0.4; truncated drops the
        // true-parent branch.
        assert!(first_only < with_both);
        assert!(close(first_only, 0.1 * 0.6));
    }

    // ── Certainty tiers ─────────────────────────────────────────────

    #[test]
    fn certainty_tier_boundaries() {
        assert_eq!(certainty_tier(0.67), "proximate");
        assert_eq!(certainty_tier(0.66), "probable");
        assert_eq!(certainty_tier(0.33), "probable");
        assert_eq!(certainty_tier(0.32), "possible");
    }

    // ── Isotonic recalibration (PAVA) ─────────────────────────────

    #[test]
    fn isotonic_fit_rejects_fewer_than_two_pairs() {
        assert!(isotonic_fit(&[]).is_none());
        assert!(isotonic_fit(&[(0.7, true)]).is_none());
    }

    #[test]
    fn isotonic_fit_pools_adjacent_violations() {
        // A non-monotone sequence: higher raw probability with lower outcome
        // rate must be pooled so the fit is non-decreasing.
        let pairs = [(0.2, true), (0.4, false), (0.6, true), (0.8, true)];
        let fit = isotonic_fit(&pairs).expect("two pairs yield a fit");
        for window in fit.windows(2) {
            assert!(
                window[0].1 <= window[1].1 + 1e-9,
                "fit must be non-decreasing: {fit:?}"
            );
        }
    }

    #[test]
    fn isotonic_apply_is_piecewise_constant_with_endpoints() {
        let fit = [(0.3, 0.25), (0.7, 0.75)];
        // Below the first knot → first value.
        assert!(close(isotonic_apply(&fit, 0.1), 0.25));
        // Inside a region → that region's value.
        assert!(close(isotonic_apply(&fit, 0.5), 0.25));
        // At a knot threshold → the knot's value (>= comparison).
        assert!(close(isotonic_apply(&fit, 0.7), 0.75));
        // Above the last knot → last value.
        assert!(close(isotonic_apply(&fit, 0.95), 0.75));
    }

    #[test]
    fn isotonic_apply_empty_fit_is_identity() {
        assert!(close(isotonic_apply(&[], 0.42), 0.42));
    }

    // ── Scenario risk measure ─────────────────────────────

    #[test]
    fn scenario_risk_measure_weighted_mean_and_sigma() {
        // Two equally-weighted branches: +100% and 0% → mean 0.5, σ = 0.5.
        let branches = [
            BranchOutcome {
                probability: 0.5,
                branch_return: 1.0,
            },
            BranchOutcome {
                probability: 0.5,
                branch_return: 0.0,
            },
        ];
        let rm = scenario_risk_measure(&branches).expect("positive mass");
        assert!(close(rm.expected_return, 0.5));
        assert!(close(rm.sigma_scenario, 0.5));
        assert_eq!(rm.branch_count, 2);
        assert!(close(rm.probability_mass, 1.0));
    }

    #[test]
    fn scenario_risk_measure_incomplete_tree_reports_mass() {
        // Branches need not sum to 1 — the mass is reported, not renormalized
        // silently.
        let branches = [BranchOutcome {
            probability: 0.25,
            branch_return: 0.2,
        }];
        let rm = scenario_risk_measure(&branches).expect("positive mass");
        assert!(close(rm.probability_mass, 0.25));
        assert!(close(rm.expected_return, 0.2));
        assert!(close(rm.sigma_scenario, 0.0));
    }

    #[test]
    fn scenario_risk_measure_none_on_zero_mass() {
        assert!(scenario_risk_measure(&[]).is_none());
        assert!(
            scenario_risk_measure(&[BranchOutcome {
                probability: 0.0,
                branch_return: 1.0,
            }])
            .is_none()
        );
    }

    // ── Scenario factor loading ────────────────────────────

    #[test]
    fn scenario_node_loading_is_conditional_return_difference() {
        // β = E[r | true] − E[r | false] = 0.8 − 0.2 = 0.6.
        let branches = [
            BranchOutcome {
                probability: 0.5,
                branch_return: 0.8,
            },
            BranchOutcome {
                probability: 0.5,
                branch_return: 0.2,
            },
        ];
        let loading =
            scenario_node_loading(&branches, &[true, false]).expect("both sets have mass");
        assert!(close(loading, 0.6));
    }

    #[test]
    fn scenario_node_loading_rejects_length_mismatch() {
        let branches = [BranchOutcome {
            probability: 1.0,
            branch_return: 0.5,
        }];
        assert!(scenario_node_loading(&branches, &[true, false]).is_none());
    }

    #[test]
    fn scenario_node_loading_none_when_one_side_has_no_mass() {
        // All branches true → the false conditioning set is empty.
        let branches = [
            BranchOutcome {
                probability: 0.5,
                branch_return: 0.8,
            },
            BranchOutcome {
                probability: 0.5,
                branch_return: 0.2,
            },
        ];
        assert!(scenario_node_loading(&branches, &[true, true]).is_none());
    }

    // ── Volatility fusion ──────────────────────────────────────

    #[test]
    fn fuse_volatility_degrades_to_realized_without_scenario() {
        assert!(close(fuse_volatility(0.3, None, 1.0), 0.3));
    }

    #[test]
    fn fuse_volatility_root_sum_square() {
        // 0.3² + (0.5·0.4)² = 0.09 + 0.04 → √0.13.
        assert!(close(fuse_volatility(0.3, Some(0.4), 0.5), 0.13_f64.sqrt()));
    }

    #[test]
    fn fuse_volatility_clamps_scenario_weight() {
        // A weight beyond [0,1] clamps to 1, not extrapolates.
        assert!(close(
            fuse_volatility(0.3, Some(0.4), 2.0),
            (0.09f64 + 0.16).sqrt()
        ));
    }
}
