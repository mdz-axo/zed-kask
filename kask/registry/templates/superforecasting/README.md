# Superforecasting Pipeline

**Location:** `registry/manifests/superforecasting.yaml`
**Templates:** `registry/templates/superforecasting/`
**Version:** 0.35.0

## Overview

This pipeline implements Philip Tetlock's Fermi-ization methodology from the Good Judgment Project. It provides a structured, multi-stage approach to producing well-calibrated probabilistic forecasts.

## Pipeline Stages

| Stage | Template                                                                                           | Purpose                                                                                                                                                             | Energy Cap |
| ----- | -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| 0     | `stage_0_triage.j2`                                                                                | Classify question difficulty (Goldilocks zone)                                                                                                                      | 2,048      |
| 1     | `stage_1_fermi_decompose.j2`                                                                       | Decompose into tractable sub-questions                                                                                                                              | 4,096      |
| 2     | `stage_2_outside_view.j2`                                                                          | Establish base rates from reference classes                                                                                                                         | 4,096      |
| 3     | `falsifiability-hypothesize` → `falsifiability-counterfactual` → `stage_3_probability_estimate.j2` | Generate causal hypotheses + counterfactual necessary-conditions (delegated to falsifiability), then estimate probabilities and adjust from the outside-view anchor | 4,096      |
| 4     | `stage_4_evidence_update.j2`                                                                       | Bayesian belief revision                                                                                                                                            | 4,096      |
| 5     | `stage_5_synthesis.j2`                                                                             | Dragonfly eye aggregation of perspectives                                                                                                                           | 4,096      |
| 6     | `stage_6_calibration.j2`                                                                           | Assign precise, calibrated probability                                                                                                                              | 4,096      |
| 7     | `stage_7_record.j2`                                                                                | Record forecast for tracking/audit                                                                                                                                  | 2,048      |
| 8     | `forecast-quality-gate.j2`                                                                         | Independent quality gate (calibration, confidence, evidence, record)                                                                                                | 3,072      |
| 9     | `superforecasting-convergence-check.j2`                                                            | Convergence metric + materiality guard                                                                                                                              | 2,048      |

**Total Energy Budget:** 25,000 tokens

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
| 2 Outside view                            | `outside_view_adjustment`                                  | Shrinkage estimator blending base rate with inside estimate.                                                                                                                                                                                                                     |
| 3 Inside view (probability estimate)      | `combine_tree_probabilities`                              | Probability estimation is LLM reasoning against the anchor; the final combine step delegates to the deterministic tree marginalization (which in turn delegates per-node to `marginalize`). Replaces the former "Aggregate hypothesis probabilities" heuristic.                                                                                              |
| 4 Evidence update                         | `bayesian_update`                                          | `posterior = prior × likelihood / evidence_base_rate`, clamped to [0.01, 0.99].                                                                                                                                                                                                  |
| 5 Synthesis (MCDA)                        | —                                                          | Natural-language only. Dragonfly-eye MCDA aggregation is LLM reasoning.                                                                                                                                                                                                          |
| 6 Calibration                             | —                                                          | Natural-language only (forward-looking single-forecast calibration). Backward-looking 10-bin calibration tracking is `compute_calibration_curve` in `hkask-mcp-scenarios`.                                                                                                       |
| 7 Record                                  | —                                                          | Natural-language only (forecast record structure). Persistent journal storage is `ForecastStore` in `hkask-mcp-scenarios`.                                                                                                                                                       |
| Quality gate                              | —                                                          | Natural-language only (independent rubric evaluation).                                                                                                                                                                                                                           |
| Convergence check                         | —                                                          | Natural-language only (materiality guard + weighted-penalty rubric).                                                                                                                                                                                                             |
| Brier scoring (cross-cutting)             | `brier_score`, `brier_score_multi`, `brier_interpretation` | Used by stage 7 record feedback and the MCP servers' outcome tracking.                                                                                                                                                                                                           |
| Marginalization (cross-cutting)           | `marginalize`                                              | Marginal probability over a set of parent variables with conditional probabilities. Used by event-tree scenario analysis in `hkask-mcp-scenarios`.                                                                                                                               |
| Tree combine (cross-cutting)              | `combine_tree_probabilities`                               | Walks a conditional probability tree in topological order and computes the outcome marginal. Delegates per-node to `marginalize` and combines multi-entry dependencies by product. Replaces the stage_3 heuristic; also the pure-math factorization of `hkask-graph-widget::recompute_marginals`. |
| Certainty tier (cross-cutting)            | `certainty_tier`                                           | Maps a probability to a qualitative tier (proximate / probable / possible) for display coloring consistency.                                                                                                                                                                     |
| Calibration feedback (cross-cutting)      | `apply_calibration_adjustment`                             | Closes the Tetlock learning loop: consumes a calibration curve's overconfidence bias (from `compute_calibration_curve` in `hkask-mcp-scenarios`) to adjust the next forecast's prior toward 0.5. The first operational bridge between recorded outcomes and future forecasts.    |
| Log-odds transform (cross-cutting)        | `log_odds`, `from_log_odds`                                | Logit and its inverse (logistic sigmoid). Interpolation and regression over bounded probabilities happen in log-odds space — linear-in-p would leak outside [0,1]. Input clamped to keep the log finite. Consumed by the prediction-markets server's calibration layer.          |
| Isotonic recalibration (cross-cutting)    | `isotonic_apply`                                           | Applies a PAVA isotonic fit (piecewise-constant calibrated probability for a raw probability). Pairs with the non-`#[must_use]` constructor `isotonic_fit`. Follows arXiv:2604.20421 §6.1's isotonic baseline.                                                                   |
| Domain-bias correction (cross-cutting)    | `domain_bias_correction`                                   | De-compresses underconfident market-implied probabilities toward the tails: `p' = 0.5 + (p-0.5)(1+δ)`, clamped to [0.01, 0.99] (arXiv:2602.19520). δ sourced from measured per-domain calibration; δ=0 is the honest default when data is insufficient.                          |
| Volatility regime (cross-cutting)         | `volatility_regime`                                        | Classifies a price series as Smooth vs JumpLike (arXiv:2607.08199): economics-style contracts move smoothly, sports-style are jump-concentrated. Returns `InsufficientData` when fewer than 2 price moves.                                                                       |
| Scenario risk measure (cross-cutting)     | `scenario_risk_measure`                                    | Probability-weighted expected return and σ over scenario-tree branches (T8a risk core). Returns `None` on zero probability mass — a risk measure over no mass is never fabricated.                                                                                               |
| Scenario factor loading (cross-cutting)   | `scenario_node_loading`                                    | APT-style factor exposure: `β(node) = E[r                                                                                                                                                                                                                                        | node true] − E[r            | node false]`over scenario branches. Returns`None` when either conditioning set has zero mass.                                |
| Volatility fusion (cross-cutting)         | `fuse_volatility`                                          | Root-sum-square fusion of realized market volatility with scenario-implied σ (independent risk channels). Degrades to realized volatility when no scenario tree — the simple path is the default.                                                                                |
| CMP scenario risk measure (cross-cutting) | `cmp_scenario_risk_measure`                                | Scenario risk measure over CMP-controlled branches. `cmp_controlled` only when every branch sources its probability from a CMP index — a single raw-contract branch contaminates the measure with the maturity-transformation confound. Returns `None` on zero probability mass. |
| Contract-price coherence (cross-cutting)  | `contract_price_coherence`                                 | R5 coherence between a tree-implied joint probability and a market price: `divergence =                                                                                                                                                                                          | tree_implied − market_price | `, `coherent`when within the transaction-cost band. Returns`None` for inputs outside [0, 1]. Feeds the H3 falsification log. |
| Duration vs CMP tenors (cross-cutting)    | `duration_vs_cmp_tenors`                                   | R2 maturity-transformation gap: compares an equity duration (Macaulay years) against the fixed CMP tenors (1m/3m/6m). Returns one `DurationGap` per tenor. `None` for non-positive duration.                                                                                     |

**Layering rule:** `hkask-forecast` holds pure-math primitives only — no domain
types, no NLP, no I/O. Domain-shaped logic (`WeightedScenario`,
`ForecastOutcome`, `ForecastStore`, event-tree marginalization, `FermiDefaults`
env loading) stays in the MCP servers where it is consumed. The skill operates on
natural language and does not call Rust directly, but its stage descriptions
must stay consistent with the formulas the primitives implement.

## Usage

### Invoking the Pipeline

```yaml
# Example pipeline invocation
manifest_id: superforecasting
input:
  forecasting_question: "Will [specific outcome] occur by [date]?"
  domain: "geopolitics" # optional
  time_horizon: "6 months" # optional
  resolution_criteria: "How the outcome will be judged"
  expiration_date: "2026-12-31"
```

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

// Stage 2: Outside View
{
  "reference_classes": [...],
  "base_rates": [...],
  "starting_probability": 0.35
}

// Stage 6: Final Calibration
{
  "final_probability": 0.42,
  "confidence_level": "medium",
  "precision_justification": "...",
  "defensible_range": {"lower": 0.35, "upper": 0.50}
}
```

## Regulation Integration

The pipeline emits Regulation spans for monitoring:

- `hkask.template.select` — Pipeline selection
- `hkask.template.render` — Template execution at each stage
- `hkask.template.outcome` — Forecast recorded

**Variety Counters:**

- `hypothesis_count` — Number of causal hypotheses generated
- `reference_class_count` — Number of reference classes identified
- `evidence_item_count` — Number of evidence items evaluated

**Algedonic Alert:** Triggered if variety deficit >100 (escalates to Curator)

## Capability Requirements

The pipeline requires the following capabilities:

- Template render permissions for all 8 stages
- Manifest execution permission
- Regulation emission permission
- Memory storage permission (for forecast recording)

All capabilities are template-scoped and expire after 3600 seconds.

## Error Handling

| Error Type         | Behavior                  |
| ------------------ | ------------------------- |
| Energy exceeded    | Abort                     |
| Timeout            | Retry (max 2, 2s backoff) |
| Validation failure | Abort                     |
| Capability denied  | Escalate to Curator       |

## Audit Trail

All pipeline executions are logged with:

- Input question and parameters
- Output from each stage
- Energy costs per stage
- Regulation event references
- Final forecast record

## Testing the Pipeline

1. **Unit tests:** Test each template independently with mock inputs
2. **Integration tests:** Run full pipeline on historical questions with known outcomes
3. **Calibration tests:** Compare predicted probabilities to actual outcomes over time

## Future Enhancements

- [x] Iterative loop (return to earlier stages on new evidence) — step 11 restarts at Fermi decomposition (step 2), carrying forward the prior iteration's calibrated probability for the materiality guard
- [x] Independent quality gate (step 9) — evaluates calibration realism, confidence justification, evidence trail, and record completeness without self-assessment bias
- [ ] Ensemble mode (multiple parallel pipeline runs) — Note: distinct from hKask ensemble module (deferred 2026-06-14)
- [ ] Human-in-the-loop checkpoints
- [ ] Automatic reference class lookup from knowledge base
- [ ] Brier score tracking and feedback
- [x] MCDA-style weighted aggregation in stage 5 (synthesis) — causal models scored against evidence alignment, reference class stability, causal mechanism clarity, and model confidence criteria, with compensation masking detection. Embedded in the synthesis template rather than delegated via template_ref to avoid flow step ordinal shifts.
- [ ] Sub-question independence validation in stage 1 (Fermi) — hypothesis-framer interface mismatch: FINER/PICO evaluates research question quality, not Fermi sub-question independence. A lightweight independence check embedded in the Fermi template is a better fit than cross-skill delegation.

## References

- Tetlock, P. & Gardner, D. (2015). _Superforecasting: The Art and Science of Prediction_
- Good Judgment Project: https://goodjudgment.com/
- Fermi-ization methodology: https://goodjudgment.com/superforecasters-toolbox-fermi-ization-in-forecasting/
- Ten Commandments: https://goodjudgment.com/philip-tetlocks-10-commandments-of-superforecasting/
