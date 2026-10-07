# Superforecasting Pipeline

**Templates:** `registry/templates/superforecasting/`
**Version:** 0.40.2

## Overview

This pipeline implements Philip Tetlock's Fermi-ization methodology from the Good Judgment Project. It provides a structured, multi-stage approach to producing well-calibrated probabilistic forecasts.

## Pipeline Stages

| Stage | Template                                                                                           | Purpose                                                                                                                                                             |
| ----- | -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0     | `stage_0_triage.j2`                                                                                | Classify question difficulty (Goldilocks zone)                                                                                                                      |
| 1     | `stage_1_fermi_decompose.j2`                                                                       | Decompose into tractable sub-questions                                                                                                                              |
| 2     | `stage_2_outside_view.j2`                                                                          | Separate sourced historical frequencies from labeled market/expert priors; stop if no numeric anchor is supported |
| 3     | `falsifiability-hypothesize` → `falsifiability-counterfactual` → `stage_3_probability_estimate.j2` | Generate causal hypotheses + counterfactual necessary-conditions (delegated to falsifiability), then estimate probabilities and adjust from the outside-view anchor |
| 4     | `stage_4_evidence_update.j2`                                                                       | Bayesian belief revision                                                                                                                                            |
| 5     | `stage_5_synthesis.j2`                                                                             | Dragonfly eye aggregation of perspectives                                                                                                                           |
| 6     | `stage_6_calibration.j2`                                                                           | Assign precise, calibrated probability                                                                                                                              |
| 7     | `stage_7_record.j2`                                                                                | Record forecast for tracking/audit                                                                                                                                  |
| 8     | `forecast-quality-gate.j2`                                                                         | Independent quality gate (calibration, confidence, evidence, record)                                                                                                |
The bounded Check→Act loop is in `.agents/skills/superforecasting/SKILL.md`; there is no convergence-check template.

## Theoretical Foundation

Based on Tetlock's **Ten Commandments for Aspiring Superforecasters**:

1. **Triage** (Commandment 1) — Focus on questions where effort pays off
2. **Fermi-ization** (Commandment 2) — Decompose intractable problems
3. **Outside/Inside View** (Commandment 3) — Anchor on base rates, adjust for specifics
4. **Evidence Updating** (Commandment 4) — Bayesian belief revision
5. **Causal Synthesis** (Commandment 5) — Dragonfly eye perspective aggregation
6. **Precision Calibration** (Commandments 6-7) — Use full probability scale
7. **Error Tracking** (Commandment 8) — Prepare for post-mortem analysis

## Deterministic Primitives (Rust Conformance Contract)

The natural-language pipeline above is backed by a small set of deterministic
primitives in the `hkask-forecast` crate (`crates/hkask-forecast/src/lib.rs`) —
the canonical pure-math core of the Tetlock methodology. The skill's LLM stages
consume these formulas implicitly; the MCP servers (`hkask-mcp-scenarios`,
`hkask-mcp-companies`) consume them explicitly via `hkask_forecast::*`.

This table is the conformance contract: each skill stage is mapped to the
`hkask-forecast` function that implements its deterministic core, or marked
"natural-language only" when no pure-math core exists. The contract is
mechanically verified by `scripts/check-forecast-conformance.sh` in CI.

| Stage                                     | `hkask-forecast` function                                  | Notes                                                                                                                                                                                                                                                                            |
| ----------------------------------------- | ---------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0 Triage                                  | —                                                          | Natural-language only. A deterministic heuristic (`triage_question`) lives in `hkask-mcp-scenarios` for tooling, but skill stage 0 is LLM judgment.                                                                                                                              |
| 1 Fermi decomposition                     | `calibrate_from_fermi`                                     | Confidence-weighted average of `FermiQuestion` estimates.                                                                                                                                                                                                                        |
| 2 Outside view                            | `outside_view_adjustment`                                  | Candidate historical rates require sourced outcome counts and sample size; the invoking agent verifies their arithmetic via `lisp_eval`. Market/expert priors remain distinct. Shrinkage blend of inside estimate toward the base rate, weight rising with `reference_count` (the skill agent applies it via `scenario_calibrate`).|
| 3 Inside view (probability estimate)      | `marginalize`                                              | Probability estimation is LLM reasoning against the anchor; the invoking agent then calls `scenario_quantify` (hkask-mcp-scenarios), which marginalizes the tree per node via `marginalize`. |
| 4 Evidence update                         | `bayesian_update`                                          | `posterior = prior × likelihood / evidence_base_rate`, clamped to [0.01, 0.99].                                                                                                                                                                                                  |
| 5 Synthesis (MCDA)                        | —                                                          | Natural-language model generation; the weighted-average synthesis is deterministic via `lisp_eval` (one `(* m_i c_i)` term per model, normalized by the composite-score sum).                                                                                                                                                                                                          |
| 6 Calibration                             | —                                                          | Natural-language only (forward-looking single-forecast calibration). Backward-looking 10-bin calibration tracking is `compute_calibration_curve` in `hkask-mcp-scenarios`.                                                                                                       |
| 7 Record                                  | —                                                          | Natural-language only (forecast record structure). Persistent journal storage is `ForecastStore` in `hkask-mcp-scenarios`.                                                                                                                                                       |
| Quality gate                              | —                                                          | Natural-language only (independent rubric evaluation).                                                                                                                                                                                                                           |
| Convergence check                         | —                                                          | The gate loop re-renders the quality gate; the pass rule is deterministic via `lisp_eval` (all four scores >= 0.60), bounded at 2 gate cycles.                                                                                                                                                                                                             |
| Brier scoring (cross-cutting)             | `brier_score`, `brier_score_multi`, `brier_interpretation` | Used by stage 7 record feedback and the MCP servers' outcome tracking.                                                                                                                                                                                                           |
| Calibration confidence (cross-cutting)   | `wilson_bounds`                                            | Wilson score interval on per-bin hit rates — the confidence bounds `hkask-mcp-scenarios` reports alongside its backward-looking calibration curve (stage 6).                                                                                                                       |
| Marginalization (cross-cutting)           | `marginalize`                                              | Marginal probability over a set of parent variables with conditional probabilities. Used by event-tree scenario analysis in `hkask-mcp-scenarios`.                                                                                                                               |
| Certainty tier (cross-cutting)            | `certainty_tier`                                           | Maps a probability to a qualitative tier (proximate / probable / possible) for display coloring consistency.                                                                                                                                                                     |
| Calibration feedback (cross-cutting)      | `apply_calibration_adjustment`                             | Closes the Tetlock learning loop: consumes a calibration curve's overconfidence bias (from `compute_calibration_curve` in `hkask-mcp-scenarios`) to adjust the next forecast's prior toward 0.5. The first operational bridge between recorded outcomes and future forecasts.    |
| Log-odds transform (cross-cutting)        | `log_odds`, `from_log_odds`                                | Logit and its inverse (logistic sigmoid). Interpolation and regression over bounded probabilities happen in log-odds space — linear-in-p would leak outside [0,1]. Input clamped to keep the log finite. Consumed by the prediction-markets server's calibration layer.          |
| Isotonic recalibration (cross-cutting)    | `isotonic_apply`                                           | Applies a PAVA isotonic fit (piecewise-constant calibrated probability for a raw probability). Pairs with the non-`#[must_use]` constructor `isotonic_fit`. Follows arXiv:2604.20421 §6.1's isotonic baseline. Applied by the scenarios server's `scenario_calibrate` tool (`isotonic_calibrated_probability`, with the fit knots emitted for inspection).                                                                   |
| Domain-bias correction (cross-cutting)    | `domain_bias_correction`                                   | De-compresses underconfident market-implied probabilities toward the tails: `p' = 0.5 + (p-0.5)(1+δ)`, clamped to [0.01, 0.99] (arXiv:2602.19520). δ sourced from measured per-domain calibration; δ=0 is the honest default when data is insufficient.                          |
| Volatility regime (cross-cutting)         | `volatility_regime`                                        | Classifies a price series as Smooth vs JumpLike (arXiv:2607.08199): economics-style contracts move smoothly, sports-style are jump-concentrated. Returns `InsufficientData` when fewer than 2 price moves.                                                                       |
| Scenario risk measure (cross-cutting)     | `scenario_risk_measure`                                    | Probability-weighted expected return and σ over scenario-tree branches (risk core). Returns `None` on zero probability mass — a risk measure over no mass is never fabricated. Emitted by the companies server's `scenario_analysis` tool (tree-weighted mode) as `risk_measure`.                                                                                                                  |
| Scenario factor loading (cross-cutting)   | `scenario_node_loading`                                    | APT-style factor exposure: `β(node) = E[r                                                                                                                                                                                                                                        | node true] − E[r            | node false]`over scenario branches. Returns`None` when either conditioning set has zero mass. Emitted by the companies server's `scenario_analysis` tool (tree-weighted mode) as `factor_loadings`.                                |
| Volatility fusion (cross-cutting)         | `fuse_volatility`                                          | Root-sum-square fusion of realized market volatility with scenario-implied σ (independent risk channels). Degrades to realized volatility when no scenario tree — the simple path is the default. Emitted by the companies server's `scenario_impact_valuation` tool as `fused_volatility` (realized σ caller-supplied, scenario channel weighted by tree coverage).                                                                                |
| CMP scenario risk measure (cross-cutting) | `cmp_scenario_risk_measure`                                | Scenario risk measure over CMP-controlled branches. `cmp_controlled` only when every branch sources its probability from a CMP index — a single raw-contract branch contaminates the measure with the maturity-transformation confound. Returns `None` on zero probability mass. Emitted by the companies server's `scenario_analysis` tool (tree-weighted mode) as `cmp_risk_measure`. |
| Contract-price coherence (cross-cutting)  | `contract_price_coherence`                                 | Coherence between a tree-implied joint probability and a market price: `divergence =                                                                                                                                                                                          | tree_implied − market_price | `, `coherent`when within the transaction-cost band. Returns`None` for inputs outside [0, 1]. Feeds the H3 falsification log. |
| Duration vs CMP tenors (cross-cutting)    | `duration_vs_cmp_tenors`                                   | Maturity-transformation gap: compares an equity duration (Macaulay years) against the fixed CMP tenors (1m/3m/6m). Returns one `DurationGap` per tenor. `None` for non-positive duration. Emitted by the companies server's `equity_duration` tool as `cmp_tenor_gaps`.                                                                       |

**Layering rule:** `hkask-forecast` holds pure-math primitives only — no domain
types, no NLP, no I/O. Domain-shaped logic (`WeightedScenario`,
`ForecastOutcome`, `ForecastStore`, event-tree marginalization, `FermiDefaults`
env loading) stays in the MCP servers where it is consumed. The skill operates on
natural language and does not call Rust directly, but its stage descriptions
must stay consistent with the formulas the primitives implement.

## Usage

### Invoking the Skill

Read `.agents/skills/superforecasting/SKILL.md` and carry a question with
resolution criteria and a deadline through its stages. No manifest executes
this pipeline; template rendering prepares prompts but does not perform
inference or call `lisp_eval`.

### Stage Outputs

Each stage produces structured JSON output that feeds into subsequent stages:

```json
// Stage 0: Triage
{
  "difficulty_level": "goldilocks",
  "goldilocks_zone": true,
  "proceed_recommendation": true,
  "rationale": "..."
}

// Stage 1: Fermi Decomposition
{
  "sub_questions": ["...", "..."],
  "assumptions": [...],
  "knowns": [...],
  "unknowns": [...]
}

// Stage 2: Outside View (a sourced market prior; no historical observations)
{
  "reference_classes": [],
  "base_rates": [],
  "candidate_priors": [{"kind": "market", "probability": 0.35, "source": "<observed market URL>", "usable": true}],
  "starting_probability": 0.35,
  "anchor_source": {"kind": "market", "source": "<observed market URL>", "rationale": "<matched outcome and deadline>"}
}
// Without a supported historical rate or matched prior, use null for both
// starting_probability and anchor_source; do not enter stage 3.

// Stage 6: Final Calibration
{
  "final_probability": 0.42,
  "confidence_level": "medium",
  "precision_justification": "...",
  "defensible_range": {"lower": 0.35, "upper": 0.50}
}
```

## Audit Trail

The pipeline's durable records (no manifest executes this pipeline — the
SKILL.md's stage instructions are the operating procedure):

- The forecast record (stage 7's output: tracking ID, question, resolution
  criteria, probability, confidence, expiration)
- The scenarios server's propagation journal (`scenario_propagate` writes
  it — the per-node before/after record of a prior revision;
  `scenario_update` itself is stateless: it computes and returns the
  posterior, and the durable record is the forecast journal below)
- The forecast journal at resolution (`scenario_score` — the only writer;
  Brier is computed there, on the tree's resolved MARGINAL probabilities:
  for a dependent event the scored belief is the marginal, not the raw
  prior field; journal schema v3 marks those records `scored_from_marginal`)

(An earlier revision of this README described a flow engine emitting
`hkask.template.*` Regulation spans, variety counters, an algedonic alert,
manifest-execution permissions, energy caps, and retry backoff — none of
that machinery exists; the description was fabricated and is removed.)

## Testing the Pipeline

1. **Unit tests:** Test each template independently with mock inputs
2. **Integration tests:** Run full pipeline on historical questions with known outcomes
3. **Calibration tests:** Compare predicted probabilities to actual outcomes over time

## Future Enhancements

- [x] Iterative loop — the SKILL.md's Loop (PDCA over the gate): the gate's fix notes name the stage to re-run, downstream stages re-run, max 2 gate cycles, then deliver with the failing dimensions recorded
- [x] Independent quality gate (step 9) — evaluates calibration realism, confidence justification, evidence trail, and record completeness without self-assessment bias
- [ ] Ensemble mode (multiple parallel pipeline runs) — Note: distinct from hKask ensemble module (deferred 2026-06-14)
- [ ] Human-in-the-loop checkpoints
- [ ] Automatic reference class lookup from knowledge base
- [x] Brier score tracking and feedback — `scenario_score` at resolution writes the forecast journal and reports Brier; `scenario_calibration` computes the curve; `scenario_calibrate` applies the learned bias (≥5 resolved)
- [x] MCDA-style weighted aggregation in stage 5 (synthesis) — causal models scored against evidence alignment, reference class stability, causal mechanism clarity, and model confidence criteria, with compensation masking detection. Embedded in the synthesis template rather than delegated via template_ref (the platform has no flow-step ordinal machinery; embedding avoids inventing one).
- [ ] Sub-question independence validation in stage 1 (Fermi) — hypothesis-framer interface mismatch: FINER/PICO evaluates research question quality, not Fermi sub-question independence. A lightweight independence check embedded in the Fermi template is a better fit than cross-skill delegation.

## References

- Tetlock, P. & Gardner, D. (2015). _Superforecasting: The Art and Science of Prediction_
- Good Judgment Project: https://goodjudgment.com/
- Fermi-ization methodology: https://goodjudgment.com/superforecasters-toolbox-fermi-ization-in-forecasting/
- Ten Commandments: https://goodjudgment.com/philip-tetlocks-10-commandments-of-superforecasting/
