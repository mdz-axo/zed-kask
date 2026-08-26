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

    #[error("tree: node '{0}' in topological order not found in nodes")]
    TreeMissingNode(String),

    #[error("tree: outcome node '{0}' not computed (missing from topological order or nodes)")]
    TreeMissingOutcome(String),

    #[error("tree: node '{0}' has neither marginal_probability nor depends_on")]
    TreeUndefinedNode(String),

    #[error(
        "tree: node '{0}' depends on '{1}' which is not yet computed (cycle or bad topological order)"
    )]
    TreeUnresolvedParent(String, String),

    #[error("tree: node '{0}' dependency {1} has {2} conditionals, expected 2^parents")]
    TreeConditionalLength(String, usize, usize),
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

/// A dependency of a tree node on a set of parents, with a conditional
/// probability table. Mirrors `hkask-graph-widget::DependencyBody` so the
/// tree-walk math stays identical to the interactive widget's
/// re-propagation.
///
/// `conditionals` is indexed by the bitmap of parent truth assignments (bit j
/// = parent j's truth), matching `marginalize`'s convention. Length must be
/// `2^parent_ids.len()`; short tables are an error (unlike the silent
/// zero-fill in `marginalize`, the tree walk rejects incomplete conditionals
/// so the LLM cannot silently emit a near-zero marginal).
#[derive(Debug, Clone)]
pub struct TreeDependency {
    pub parent_ids: Vec<String>,
    pub conditionals: Vec<f64>,
}

/// A node in a conditional probability tree. Roots carry a `marginal_probability`;
/// dependents carry `depends_on` entries. A node must have exactly one of the
/// two — a root with no parents has `marginal_probability = Some(p)` and an
/// empty `depends_on`; a dependent has `marginal_probability = None` and a
/// non-empty `depends_on`.
///
/// The combinator (AND-gate / OR-gate / mixture) is encoded structurally in
/// the conditional table values, not as a separate field — an AND-gate has
/// conditionals equal to 1 only when all parents are true, an OR-gate has
/// conditionals equal to 1 when any parent is true. This avoids a second
/// heuristic layer on top of the tree.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: String,
    pub marginal_probability: Option<f64>,
    /// Each entry marginalizes over its own parents via `marginalize`; multiple
    /// entries on the same node combine by independence (product), matching
    /// `recompute_marginals`'s `multi_dep_combines_by_independence` semantics.
    pub depends_on: Vec<TreeDependency>,
}

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

/// Walk a conditional probability tree in topological order and compute the
/// marginal probability of the outcome node. Roots contribute their stored
/// `marginal_probability`; each dependent marginalizes its `depends_on` entries
/// via `marginalize` (the single source of truth for the joint marginalization
/// formula) and combines multiple entries by independence (product), matching
/// `hkask-graph-widget::propagate::recompute_marginals`.
///
/// This is the exact chain-rule computation — no independence heuristic is
/// applied across a node's parents; the conditioning is encoded in each
/// conditional table. The LLM's job is the structural reasoning (tree shape,
/// per-node conditional tables); this function owns the numeric aggregation the
/// LLM cannot do reliably. Replaces the former stage_3 "Aggregate hypothesis
/// probabilities into a single combined_probability" heuristic.
///
/// This is the pure-math factorization of `recompute_marginals` (which walks
/// the GPUI block body). The skill's stage_3 emits the tree; the compute
/// dispatcher calls this; stage_4 consumes the resulting
/// `tree_combined_probability` as its prior.
#[must_use = "tree-combined probability should be used as the stage-4 prior"]
pub fn combine_tree_probabilities(
    nodes: &[TreeNode],
    topological_order: &[&str],
    outcome_id: &str,
) -> Result<f64, ForecastError> {
    if topological_order.is_empty() {
        return Err(ForecastError::TreeMissingOutcome(outcome_id.to_string()));
    }

    // Index nodes by id for O(1) lookup during the topological walk.
    let mut node_map: std::collections::HashMap<&str, &TreeNode> =
        std::collections::HashMap::with_capacity(nodes.len());
    for node in nodes {
        if node_map.insert(node.id.as_str(), node).is_some() {
            return Err(ForecastError::TreeMissingNode(format!(
                "duplicate node id '{}' in tree",
                node.id
            )));
        }
    }

    // Computed marginals, filled as the walk progresses.
    let mut computed: std::collections::HashMap<&str, f64> =
        std::collections::HashMap::with_capacity(topological_order.len());

    for id in topological_order {
        let node = node_map
            .get(id)
            .ok_or_else(|| ForecastError::TreeMissingNode(id.to_string()))?;

        let marginal = match (node.marginal_probability, node.depends_on.is_empty()) {
            (Some(p), true) => {
                if !p.is_finite() || !(0.0..=1.0).contains(&p) {
                    return Err(ForecastError::InvalidProbability(
                        p,
                        format!("marginal_probability for node '{}'", node.id),
                    ));
                }
                p
            }
            (None, false) => {
                // Dependent node: marginalize each depends_on entry over its
                // parents and combine entries by independence (product).
                let mut combined = 1.0_f64;
                for (entry_idx, dep) in node.depends_on.iter().enumerate() {
                    let expected = 1usize << dep.parent_ids.len();
                    if dep.conditionals.len() != expected {
                        return Err(ForecastError::TreeConditionalLength(
                            node.id.clone(),
                            entry_idx,
                            expected,
                        ));
                    }
                    let parent_marginals: Vec<f64> = dep
                        .parent_ids
                        .iter()
                        .map(|pid| {
                            computed.get(pid.as_str()).copied().ok_or_else(|| {
                                ForecastError::TreeUnresolvedParent(node.id.clone(), pid.clone())
                            })
                        })
                        .collect::<Result<_, _>>()?;
                    let entry_marginal = marginalize(&parent_marginals, &dep.conditionals);
                    combined *= entry_marginal.clamp(0.0, 1.0);
                }
                combined.clamp(0.0, 1.0)
            }
            (Some(_), false) => {
                // Both set: ambiguous — reject so the LLM can't silently
                // override the tree math with a free-floating marginal.
                return Err(ForecastError::TreeUndefinedNode(format!(
                    "node '{}' has both marginal_probability and depends_on",
                    node.id
                )));
            }
            (None, true) => {
                return Err(ForecastError::TreeUndefinedNode(node.id.clone()));
            }
        };

        computed.insert(id, marginal);
    }

    computed
        .get(outcome_id)
        .copied()
        .ok_or_else(|| ForecastError::TreeMissingOutcome(outcome_id.to_string()))
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
/// Callers source δ from measured per-domain calibration (the
/// `superforecast::domain_bias_delta` function reads the calibration loop's
/// resolved forecasts for the domain; when there is insufficient data, δ=0.0
/// — no correction — the honest default per Tetlock's discipline: corrections
/// come from measured calibration, not hardcoded magic numbers).
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

// ── R4: σ_scenario over CMP-driven branches ────────────────────────────────
//
// Re-points the scenario risk measure at CMP-controlled branch probabilities.
// A CMP branch is a `BranchOutcome` whose probability comes from a CMP index
// (constant-maturity, constant-orientation), not a raw decaying contract. The
// risk measure carries CMP provenance so downstream consumers can distinguish
// CMP-controlled risk from raw-contract risk.

/// A scenario branch whose probability comes from a CMP index.
///
/// `cmp_source` identifies the CMP index that supplied the probability
/// (e.g. "cmp:policy_interest_rate:3m:increase"). When `None`, the branch
/// probability is from a raw contract (pre-R4 behavior) — the risk measure
/// degrades to the uncontrolled form. Owned `String` so dynamically-generated
/// CMP source identifiers (from `compose_cmp_tree`) can be used without
/// leaking allocations or forcing `'static`.
#[derive(Debug, Clone)]
pub struct CmpBranchOutcome {
    /// The branch's joint probability (from a CMP index or a raw contract).
    pub probability: f64,
    /// The company's annualized return if this branch realizes.
    pub branch_return: f64,
    /// CMP index identity when the probability is CMP-controlled, else None.
    /// Carries the (family, tenor, orientation) of the source index.
    pub cmp_source: Option<String>,
}

/// Probability-weighted risk measure with CMP provenance.
///
/// When `cmp_controlled` is true, all branch probabilities came from CMP
/// indices — the risk measure is maturity-controlled. When false, at least
/// one branch used a raw-contract probability — the risk measure carries
/// the maturity-transformation confound.
#[derive(Debug, Clone, Copy)]
pub struct CmpScenarioRiskMeasure {
    /// The underlying scenario risk measure (expected return, σ_scenario).
    pub inner: ScenarioRiskMeasure,
    /// True when all branch probabilities came from CMP indices.
    pub cmp_controlled: bool,
    /// Number of CMP-controlled branches.
    pub cmp_branch_count: usize,
}

/// Compute the scenario risk measure over CMP-controlled branches.
///
/// Each branch's probability comes from either a CMP index (`cmp_source`
/// present) or a raw contract (`cmp_source` absent). The risk measure is
/// `cmp_controlled` only when ALL branches are CMP-controlled — a single
/// raw-contract branch contaminates the measure with the maturity-
/// transformation confound.
///
/// Returns None when no branch has positive probability (same contract as
/// `scenario_risk_measure`). Never fabricates.
#[must_use = "CMP risk measure should feed valuation or coherence analysis"]
pub fn cmp_scenario_risk_measure(branches: &[CmpBranchOutcome]) -> Option<CmpScenarioRiskMeasure> {
    let raw: Vec<BranchOutcome> = branches
        .iter()
        .map(|b| BranchOutcome {
            probability: b.probability,
            branch_return: b.branch_return,
        })
        .collect();
    let inner = scenario_risk_measure(&raw)?;
    let cmp_branch_count = branches.iter().filter(|b| b.cmp_source.is_some()).count();
    let cmp_controlled = cmp_branch_count == branches.len() && !branches.is_empty();
    Some(CmpScenarioRiskMeasure {
        inner,
        cmp_controlled,
        cmp_branch_count,
    })
}

// ── R5: Contract-price coherence (H3 reframed) ──────────────────────────────
//
// The arbitrage analysis on the contracts: are the tree-implied joint
// probabilities coherent with observed contract prices (incl. parlay/joint
// contracts where listed)? Divergence = the analyzable signal.
//
// This is the H3 test (reframed per user correction): NO equity-return
// regressions, NO betas. The arbitrage-pricing apparatus applies to the
// contracts — decomposing and bridging their prices and analyzing their
// coherence — never to modeling stock returns.

/// The coherence between a tree-implied joint probability and a market price.
///
/// `divergence` = |tree_implied - market_price|. When `divergence <= cost_band`,
/// the tree is coherent with the market (the gap is within transaction costs).
/// When `divergence > cost_band`, the gap is the arbitrage signal — the tree
// and the market disagree beyond what transaction costs explain.
#[derive(Debug, Clone, Copy)]
pub struct CoherenceMeasure {
    /// The tree-implied joint probability.
    pub tree_implied: f64,
    /// The observed market price (joint/parlay contract, or single contract).
    pub market_price: f64,
    /// |tree_implied - market_price| — the absolute divergence.
    pub divergence: f64,
    /// The transaction-cost band (passed variable). Divergences within this
    /// band are not actionable (transaction costs eat the arbitrage).
    pub cost_band: f64,
    /// Whether the divergence is within the transaction-cost band.
    pub coherent: bool,
}

/// Measure the coherence between a tree-implied joint probability and a
/// market price (R5).
///
/// `tree_implied` is the joint probability from the CMP-controlled tree
/// (e.g. P(rates increase AND oil increase) from `compose_cmp_tree` output).
/// `market_price` is the observed price of a parlay/joint contract on the
/// same events (or a single contract's price for a marginal comparison).
/// `cost_band` is the transaction-cost band (a passed variable — the sum of
/// bid-ask spreads, fees, and slippage for both legs of the arbitrage).
///
/// Returns `None` when either input is outside [0, 1] — a coherence measure
/// over an invalid probability is undefined, never fabricated.
///
/// The falsifier (H3): if `coherent` is systematically false across many
/// CMP-controlled trees (the tree diverges from the market beyond costs),
/// the composition algebra adds no pricing coherence — H3 is refuted. If
/// `coherent` is true on CMP trees but false on raw-snapshot trees, CMP is
/// the active ingredient — H3b corroborated.
#[must_use = "coherence measure should feed the H3 falsification log"]
pub fn contract_price_coherence(
    tree_implied: f64,
    market_price: f64,
    cost_band: f64,
) -> Option<CoherenceMeasure> {
    if !(0.0..=1.0).contains(&tree_implied) || !(0.0..=1.0).contains(&market_price) {
        return None;
    }
    let divergence = (tree_implied - market_price).abs();
    let coherent = divergence <= cost_band;
    Some(CoherenceMeasure {
        tree_implied,
        market_price,
        divergence,
        cost_band,
        coherent,
    })
}

// ── R2: Duration matching vs constant maturity ─────────────────────────────
//
// Compares equity duration (Macaulay years) against the fixed CMP tenors
// (1m/3m/6m = ~0.083/0.25/0.5 years). The gap is the H2 signal: equity
// duration is typically 5-15+ years, while CMP tenors are sub-year. This
// maturity-transformation gap is now a controlled quantity (CMP fixes the
// tenor) rather than an unmeasurable confound (decaying contract snapshots).

/// The standard CMP tenors in years (1m/3m/6m = 30/90/180 days).
pub const CMP_TENORS_YEARS: [f64; 3] = [30.0 / 365.25, 90.0 / 365.25, 180.0 / 365.25];

/// The labels for the standard CMP tenors.
pub const CMP_TENOR_LABELS: [&str; 3] = ["1m", "3m", "6m"];

/// One entry in the duration-vs-CMP comparison.
#[derive(Debug, Clone)]
pub struct DurationGap {
    /// The CMP tenor label ("1m", "3m", "6m").
    pub tenor_label: &'static str,
    /// The CMP tenor in years.
    pub tenor_years: f64,
    /// |equity_duration − tenor| in years — the maturity-transformation gap.
    pub gap_years: f64,
    /// The ratio equity_duration / tenor — how many CMP tenors fit inside the
    /// equity duration. A ratio of 20 means the equity claim is 20 CMP-3m
    /// periods long — the maturity transformation is 20:1.
    pub ratio: f64,
}

/// Compare an equity duration against the fixed CMP tenors (R2).
///
/// Returns one `DurationGap` per CMP tenor (1m, 3m, 6m). The gap is the
/// absolute difference between the equity duration and the tenor; the ratio
/// is how many tenors fit inside the equity duration. This is the H2/T1
/// dataset: the maturity-transformation gap is now a controlled quantity
/// (CMP fixes the tenor) rather than an unmeasurable confound.
///
/// Returns `None` when `equity_duration_years` is not positive — a duration
/// over a non-positive stream is not meaningful (mirrors `EquityDuration`'s
/// None contract). Never a fabricated number.
#[must_use = "duration gap should be used or the None inspected"]
pub fn duration_vs_cmp_tenors(equity_duration_years: f64) -> Option<Vec<DurationGap>> {
    if equity_duration_years <= 0.0 {
        return None;
    }
    Some(
        CMP_TENORS_YEARS
            .iter()
            .zip(CMP_TENOR_LABELS.iter())
            .map(|(&tenor, &label)| DurationGap {
                tenor_label: label,
                tenor_years: tenor,
                gap_years: (equity_duration_years - tenor).abs(),
                ratio: equity_duration_years / tenor,
            })
            .collect(),
    )
}

// ── R3: CMP index provenance (shared bridge contract) ─────────────────────
//
// The bridge contract between `hkask-mcp-scenarios`'s `scenario_from_cmp_indices`
// emitter and `hkask-mcp-companies`'s `EventTreeProjection` deserializer. Both
// crates re-export this struct as their `superforecast::CmpIndexProvenance` so the
// two sides cannot drift apart at the type level — the pin tests in each crate
// (`scenario_from_cmp_indices_emits_full_cmp_provenance_inside_tree` in
// scenarios, `cmp_provenance_round_trips_real_scenarios_emitter` in companies)
// enforce that the real emitters produce the full 7-field shape.
//
// `id` is the canonical index identity (`cmp:{family}:{tenor}:{orientation}`);
// the other 6 fields are the index identity components the companies bridge
// surfaces in the tree-weighted output so the consumer can cite the CMP index
// provenance without re-parsing the `id` string. Each field carries
// `#[serde(default)]` so the struct deserializes from a minimal `
// `{"id": "..."}` entry (the pre-R3 shape) — the pin tests enforce that the
// real emitters populate all 7, but the deserializer tolerates partial output
// rather than failing the whole tree on one malformed entry. The
// `#[serde(default)]` per field is NOT a security gate (per the repo `.rules`
// on advertised invariants) — the real gates are the two pin tests. Advertised
// invariant: "the real emitters populate all 7 fields" — enforced by the pin
// tests, not by the struct's `#[serde(default)]`.

/// R3: CMP index provenance — the full 7-field identity of one CMP index in a
/// `scenario_from_cmp_indices` tree. Re-exported by both `hkask-mcp-scenarios`
/// and `hkask-mcp-companies` as their `superforecast::CmpIndexProvenance` so the
/// bridge contract has a single type-level source of truth.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct CmpIndexProvenance {
    /// The CMP index identity: `cmp:{family}:{tenor}:{orientation}`.
    #[serde(default)]
    pub id: String,
    /// The base-event family label (e.g. `"policy_interest_rate"`).
    #[serde(default)]
    pub family: String,
    /// The CMP tenor label (`"1m"`, `"2m"`, `"3m"`, `"6m"`).
    #[serde(default)]
    pub tenor: String,
    /// The orientation (`"increase"`, `"decline"`, `"stable"`).
    #[serde(default)]
    pub orientation: String,
    /// The venue (`"kalshi"`, `"polymarket"`).
    #[serde(default)]
    pub venue: String,
    /// The construction method (`"interpolated"` or `"bucketed_sparse"`).
    #[serde(default)]
    pub method: String,
    /// The maturity-matching error in days (|weighted maturity − target|).
    #[serde(default)]
    pub maturity_error_days: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    // ── Brier scoring ───────────────────────────────────────────────

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

    #[test]
    fn brier_interpretation_thresholds() {
        assert_eq!(brier_interpretation(0.0), "excellent");
        assert_eq!(brier_interpretation(0.08), "good");
        assert_eq!(brier_interpretation(0.15), "fair");
        assert_eq!(brier_interpretation(0.30), "poor");
        assert_eq!(brier_interpretation(0.90), "worse_than_climatology");
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
        assert!(many.0 > few.0, "more references should raise toward base rate");
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

    // ── Tree walk ───────────────────────────────────────────────────

    #[test]
    fn combine_tree_single_root_returns_its_marginal() {
        let nodes = vec![TreeNode {
            id: "outcome".into(),
            marginal_probability: Some(0.42),
            depends_on: vec![],
        }];
        let combined = combine_tree_probabilities(&nodes, &["outcome"], "outcome").unwrap();
        assert!(close(combined, 0.42));
    }

    #[test]
    fn combine_tree_dependent_node_marginalizes_over_parent() {
        let nodes = vec![
            TreeNode {
                id: "root".into(),
                marginal_probability: Some(0.5),
                depends_on: vec![],
            },
            TreeNode {
                id: "outcome".into(),
                marginal_probability: None,
                depends_on: vec![TreeDependency {
                    parent_ids: vec!["root".into()],
                    conditionals: vec![0.1, 0.9],
                }],
            },
        ];
        let combined =
            combine_tree_probabilities(&nodes, &["root", "outcome"], "outcome").unwrap();
        assert!(close(combined, 0.1 * 0.5 + 0.9 * 0.5));
    }

    #[test]
    fn combine_tree_rejects_duplicate_ids() {
        let node = || TreeNode {
            id: "dup".into(),
            marginal_probability: Some(0.5),
            depends_on: vec![],
        };
        assert!(matches!(
            combine_tree_probabilities(&[node(), node()], &["dup"], "dup"),
            Err(ForecastError::TreeMissingNode(_))
        ));
    }

    #[test]
    fn combine_tree_rejects_wrong_conditional_table_length() {
        let nodes = vec![
            TreeNode {
                id: "root".into(),
                marginal_probability: Some(0.5),
                depends_on: vec![],
            },
            TreeNode {
                id: "dep".into(),
                marginal_probability: None,
                depends_on: vec![TreeDependency {
                    parent_ids: vec!["root".into()],
                    conditionals: vec![0.5], // needs 2^1 = 2 entries
                }],
            },
        ];
        assert!(matches!(
            combine_tree_probabilities(&nodes, &["root", "dep"], "dep"),
            Err(ForecastError::TreeConditionalLength(_, 0, 2))
        ));
    }

    #[test]
    fn combine_tree_rejects_node_with_both_marginal_and_dependencies() {
        let nodes = vec![TreeNode {
            id: "ambiguous".into(),
            marginal_probability: Some(0.5),
            depends_on: vec![TreeDependency {
                parent_ids: vec!["self".into()],
                conditionals: vec![0.0, 1.0],
            }],
        }];
        assert!(matches!(
            combine_tree_probabilities(&nodes, &["ambiguous"], "ambiguous"),
            Err(ForecastError::TreeUndefinedNode(_))
        ));
    }

    #[test]
    fn combine_tree_missing_outcome_is_an_error() {
        let nodes = vec![TreeNode {
            id: "a".into(),
            marginal_probability: Some(0.5),
            depends_on: vec![],
        }];
        assert!(matches!(
            combine_tree_probabilities(&nodes, &["a"], "nope"),
            Err(ForecastError::TreeMissingOutcome(_))
        ));
    }

    // ── Certainty tiers ─────────────────────────────────────────────

    #[test]
    fn certainty_tier_boundaries() {
        assert_eq!(certainty_tier(0.67), "proximate");
        assert_eq!(certainty_tier(0.66), "probable");
        assert_eq!(certainty_tier(0.33), "probable");
        assert_eq!(certainty_tier(0.32), "possible");
    }

    // ── Calibration adjustment ──────────────────────────────────────

    #[test]
    fn calibration_overconfidence_pulls_extreme_prior_toward_half() {
        // Overconfident forecaster (positive bias): extreme 0.9 regresses down.
        let adjusted = apply_calibration_adjustment(0.9, 0.4);
        assert!(adjusted < 0.9 && adjusted > 0.5);
    }

    #[test]
    fn calibration_underconfidence_pushes_prior_away_from_half() {
        let adjusted = apply_calibration_adjustment(0.9, -0.4);
        assert!(adjusted > 0.9);
    }

    #[test]
    fn calibration_bias_influence_is_clamped() {
        // A wild bias must not invert the forecast past the clamp bounds.
        let adjusted = apply_calibration_adjustment(0.9, 50.0);
        assert!((0.01..=0.99).contains(&adjusted));
    }
}
