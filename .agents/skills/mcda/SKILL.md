---
name: mcda
visibility: public
description: "Multi-Criteria Decision Analysis. Identifies decision criteria, weights and scores alternatives, ranks options with compensation masking detection, and performs sensitivity analysis to assess decision robustness.
"
---

# Mcda

Multi-Criteria Decision Analysis. Identifies decision criteria, weights and scores alternatives, ranks options with compensation masking detection, and performs sensitivity analysis to assess decision robustness.


## When to Use

- When a decision question involves multiple alternatives and you need to enumerate, classify (benefit or cost), and validate the independence of decision criteria before weighting.
- When you need to assign weights to criteria using a specified method (direct or swing) and score each alternative to produce normalized scores and composite rankings.
- When you need to rank alternatives by composite score and detect compensation masking — cases where strong performance on non-critical criteria hides poor performance on a critical criterion.
- When you need to assess decision robustness by perturbing criterion weights to identify rank reversals, critical weights, and classify overall stability.
- When you need to compute a convergence metric for an MCDA PDCA cycle to determine whether ranking confidence and sensitivity robustness are sufficient to stop iterating.

## Instructions

### identify-criteria

1. Enumerate all relevant decision dimensions that differentiate the alternatives, considering functional, economic, social, environmental, and strategic factors.
2. Classify each criterion as a **benefit** criterion (higher values are better) or a **cost** criterion (lower values are better).
3. Evaluate pairwise correlations between criteria and flag any pair with estimated correlation >0.7 as potentially dependent.
4. For each criterion, provide a clear description and note the measurement type (quantitative, qualitative, or ordinal).
5. Produce at least 3 and at most 12 criteria — too few collapses dimensions, too many dilutes discrimination.
6. Ensure every criterion clearly differentiates at least two alternatives.
7. Flag dependent pairs but do not automatically merge — leave that decision to the weight-and-score stage.

### rank-alternatives

1. Rank alternatives by composite score, consistent with the `composite_scores` input — do not re-rank arbitrarily.
2. Identify the top choice by composite score.
3. For the top-ranked alternative, identify any criterion where the normalized score is below the danger threshold (default 0.3 out of 1.0).
4. Check whether that criterion is critical (weight >0.1 or marked as essential by the decision question).
5. If a critical criterion has a score below the threshold, flag a compensation warning and assess severity as minor (one weak criterion, non-critical) or major (weak on critical criterion).
6. For every ranked alternative, identify both a strength and a weakness.
7. Provide an actionable recommendation: proceed, investigate further, or add a veto criterion.

### sensitivity-analysis

1. For each criterion weight, perturb it by ±10% of its current value (increase: `w_new = w × 1.10`; decrease: `w_new = w × 0.90`).
2. Renormalize all weights so they sum to 1.0 after perturbation.
3. Recompute composite scores with the perturbed weights.
4. Check if the top-ranked alternative changes (rank reversal) for each perturbation.
5. Identify critical weights — those where perturbation causes the top choice to flip — and record which weight, perturbation direction, and the new top choice.
6. Classify robustness as **robust** (no rank reversal at ±10%), **moderate** (reversal only when weight changes exceed ±5%), or **fragile** (reversal with weight changes less than ±5%).
7. If the criteria independence check from Stage 1 identified dependent pairs (correlation >0.7), warn that OAT perturbation underestimates true sensitivity and suggest a combined perturbation test shifting both correlated weights simultaneously.
8. Provide a recommendation addressing whether to proceed, gather more data, or restructure criteria.

### weight-and-score

1. Assign weights to criteria using the specified weighting method (direct or swing).
2. For swing weighting: imagine all criteria at their worst level; the criterion whose improvement from worst to best provides the greatest swing in overall value gets the highest weight.
3. For direct weighting: assign weights directly reflecting the relative importance of each criterion and justify each assignment.
4. Normalize weights so they sum to exactly 1.0 and verify before output.
5. For each alternative on each criterion, assign a raw score on a 0–10 scale (benefit: 0 = worst, 10 = best; cost: 0 = most costly, 10 = least costly, already inverted).
6. Normalize scores to 0–1 range: `normalized = raw / 10`.
7. Compute composite scores using the weighted sum model: `composite_score(A) = Σ (weight_i × normalized_score(A, i))`.
8. Rank alternatives by composite score descending (rank 1 = best).
9. If two alternatives have identical composite scores, assign the same rank and skip the next rank.

### mcda-convergence-check

1. Measure convergence on [0,1] where 0 means ranking confidence is high and sensitivity analysis shows acceptable robustness, and 1 means not converged.
2. Score how much work remains based on the ranking result and sensitivity result inputs.
3. Return the convergence metric, a short rationale, and any blockers.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `identify-criteria.j2` | KnowAct | Identify and classify decision criteria as benefit or cost dimensions. Validates criteria independence and produces a structured criterion set.  |
| `rank-alternatives.j2` | KnowAct | Rank alternatives by composite scores with compensation masking detection. Produces a top choice recommendation with warnings for cases where strong performance on one criterion masks poor performance.  |
| `sensitivity-analysis.j2` | KnowAct | Perform sensitivity analysis on decision rankings by perturbing weights. Identifies rank reversals, critical weights, and classifies overall decision robustness.  |
| `weight-and-score.j2` | KnowAct | Weight criteria and score alternatives using the specified weighting method (direct or swing). Produces normalized scores and composite rankings for each alternative.  |
| `mcda-convergence-check.j2` | KnowAct | Compute normalized convergence metric for MCDA PDCA cycles. Returns convergence_metric plus rationale and blockers.  |

## Constraints

- `identify-criteria.j2`: Public.
- `rank-alternatives.j2`: Public.
- `sensitivity-analysis.j2`: Public.
- `weight-and-score.j2`: Public.
- `mcda-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
