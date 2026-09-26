---
name: mcda
description: "Multi-Criteria Decision Analysis. Identifies decision criteria, weights and scores alternatives, ranks options with compensation masking detection, and performs sensitivity analysis to assess decision robustness.
"
---

# Mcda

Multi-Criteria Decision Analysis. Identifies decision criteria, weights and scores alternatives, ranks options with compensation masking detection, and performs sensitivity analysis to assess decision robustness.


## Reference models

Belton & Stewart, *Multiple Criteria Decision Analysis: An Integrated Approach* (2002) — weighted-sum value model and sensitivity analysis; Keeney & Raiffa, *Decisions with Multiple Objectives* (1976) — swing weighting. `onto_anchor` reaches only the 5W1H core for "multi-criteria decision analysis" (coarse; no operator ruling yet).

**D/P labelling.** Criteria, classification, weights and raw scores are P (judgment; critique: the operator, and the sensitivity analysis shows how much the decision depends on them). Normalization, composites, ranking, perturbation and robustness class are D (`lisp_eval`, helpers below). The compensation-masking check is D once scores and weights exist.

## Initial and target condition

- **Initial condition:** the decision question, the enumerated alternatives, and any operator-fixed weights or veto criteria.
- **Target condition:** a ranking whose robustness class is `robust` or `moderate`, with every compensation warning reported — or, after the bound, a fragile ranking reported with its critical weights for the operator to decide.

## Deterministic helpers (`lisp_eval`)

Prepend these to each form. They compute over the weights and scores supplied; they do not vouch for those judgments.

```lisp
(define sum (lambda (l) (if (is_null l) 0 (+ (car l) (sum (cdr l))))))
(define scale (lambda (l k) (if (is_null l) (quote ()) (cons (* (car l) k) (scale (cdr l) k)))))
(define norm (lambda (w) (scale w (/ 1 (sum w)))))
(define dot (lambda (a b) (if (is_null a) 0 (+ (* (car a) (car b)) (dot (cdr a) (cdr b))))))
(define bump (lambda (w i k) (if (is_null w) (quote ()) (cons (if (= i 0) (* (car w) k) (car w)) (bump (cdr w) (- i 1) k)))))
(define argmax (lambda (w alts) (let ((best (lambda (as i bi bv) (if (is_null as) bi (let ((v (dot (norm w) (car as)))) (if (> v bv) (best (cdr as) (+ i 1) i v) (best (cdr as) (+ i 1) bi bv))))))) (best alts 0 -1 -1))))
(define top (argmax W ALTS))
(define flips (lambda (d) (let ((chk (lambda (i) (if (= i (length W)) 0 (if (or (!= (argmax (bump W i (+ 1 d)) ALTS) top) (!= (argmax (bump W i (- 1 d)) ALTS) top)) 1 (chk (+ i 1))))))) (chk 0))))
(define scan (lambda (ds) (if (is_null ds) "none" (if (= (flips (car ds)) 1) (car ds) (scan (cdr ds))))))
```

env: `W` = raw weights (one per criterion), `ALTS` = one list per alternative of normalized scores (`raw / 10`) in criterion order. Composites: `(dot (norm W) <alt>)`. `min_flip`: `(scan (quote (0.01 0.02 0.03 0.05 0.1)))` — the smallest relative one-weight change that flips the top choice, or `"none"` within 10%. `argmax` keeps the first alternative on ties; report ties explicitly.

## When to Use

- When a decision question involves multiple alternatives and you need to enumerate, classify (benefit or cost), and validate the independence of decision criteria before weighting.
- When you need to assign weights to criteria using a specified method (direct or swing) and score each alternative to produce normalized scores and composite rankings.
- When you need to rank alternatives by composite score and detect compensation masking — cases where strong performance on non-critical criteria hides poor performance on a critical criterion.
- When you need to assess decision robustness by perturbing criterion weights to identify rank reversals, critical weights, and classify overall stability.
- When you need to compute a convergence metric for an MCDA PDCA cycle to determine whether ranking confidence and sensitivity robustness are sufficient to stop iterating.

## When NOT to Use

- Decisions with fixed methodology weights — when the weights are fixed by firm methodology (e.g. GORILLA's 25/30/25/20), a `lisp_eval` scoring call is the honest instrument; MCDA adds ceremony (the essentialist Surface gate rejection).
- Single-criterion decisions — with one criterion there is nothing to weight, mask, or perturb; just decide.
- No enumerable criteria — without criteria to classify and weight, the method has no input; elicit requirements first.

## Instructions

### identify-criteria

1. Enumerate all relevant decision dimensions that differentiate the alternatives, considering functional, economic, social, environmental, and strategic factors.
2. Classify each criterion as a **benefit** criterion (higher values are better) or a **cost** criterion (lower values are better).
3. Evaluate pairwise correlations between criteria and flag any pair with estimated correlation >0.7 as potentially dependent.
4. For each criterion, provide a clear description and note the measurement type (quantitative, qualitative, or ordinal).
5. Produce at least 3 and at most 12 criteria — too few collapses dimensions, too many dilutes discrimination.
6. Ensure every criterion clearly differentiates at least two alternatives.
7. Flag dependent pairs but do not automatically merge — leave that decision to the weight-and-score stage.

### weight-and-score

1. Assign weights to criteria using the specified weighting method (direct or swing).
2. For swing weighting: imagine all criteria at their worst level; the criterion whose improvement from worst to best provides the greatest swing in overall value gets the highest weight.
3. For direct weighting: assign weights directly reflecting the relative importance of each criterion and justify each assignment.
4. (D) Weights are normalized by `(norm W)` in `lisp_eval`; the render's own normalization is not used.
5. For each alternative on each criterion, assign a raw score on a 0–10 scale (benefit: 0 = worst, 10 = best; cost: 0 = most costly, 10 = least costly, already inverted).
6. (D) Normalize scores to 0–1: `normalized = raw / 10`.
7. (D) Compute composites with `lisp_eval` `(dot (norm W) <alt>)` for each alternative — the weighted sum model.
8. (D) Rank alternatives by composite descending (rank 1 = best).
9. If two alternatives have identical composite scores, assign the same rank and skip the next rank.

### rank-alternatives

1. Render `mcda/rank-alternatives` with the `lisp_eval` composites as `composite_scores` and `raw / 10` values as `normalized_scores`; rank consistent with that input — do not re-rank arbitrarily.
2. Identify the top choice by composite score.
3. For the top-ranked alternative, identify any criterion where the normalized score is below the danger threshold (default 0.3 out of 1.0).
4. Check whether that criterion is critical (weight >0.1 or marked as essential by the decision question).
5. If a critical criterion has a score below the threshold, flag a compensation warning and assess severity as minor (one weak criterion, non-critical) or major (weak on critical criterion).
6. For every ranked alternative, identify both a strength and a weakness.
7. Provide an actionable recommendation: proceed, investigate further, or add a veto criterion.

### sensitivity-analysis

1. (D) Compute `min_flip` with `lisp_eval` (helpers above): each weight is perturbed one at a time by ±1%, 2%, 3%, 5% and 10% of its value, renormalized, and the composites recomputed; `min_flip` is the smallest change that changes the top choice.
2. (D) Identify critical weights: for each weight, the same grid gives the smallest change that flips the top choice, its direction, and the new top choice.
3. (D) Classify robustness from `min_flip`: **robust** = `"none"` (no flip within 10%); **moderate** = smallest flip above 5% and at most 10%; **fragile** = a flip at 5% or less.
4. (P) Render `mcda/sensitivity-analysis` with `min_flip` and `critical_weights` to interpret what the critical weights mean for the decision.
5. If the criteria independence check from Stage 1 identified dependent pairs (correlation >0.7), warn that OAT perturbation underestimates true sensitivity and suggest a combined perturbation test shifting both correlated weights simultaneously.
6. Provide a recommendation addressing whether to proceed, gather more data, or restructure criteria.

### Convergence

9. Gate — call `lisp_eval` with:
   - form: `(or (string= min_flip "none") (> min_flip 0.05))`
   - env: `{ "min_flip": <the lisp_eval min_flip result> }`
   Robust or moderate outcomes (no flip at 5% or less) close the loop. A
   fragile outcome re-enters weight-and-score once with the critical-weight
   findings — add the veto criterion rank-alternatives step 7 names, or
   restructure the criteria sensitivity-analysis step 8 names. Bound: max 2
   restructurings; a third fragile outcome is reported honestly with the
   critical weights — the decision is the operator's, not the loop's.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `identify-criteria.j2` | Identify and classify decision criteria as benefit or cost dimensions. Validates criteria independence and produces a structured criterion set. |
| `rank-alternatives.j2` | Rank alternatives by composite scores with compensation masking detection. Produces a top choice recommendation with warnings for cases where strong performance on one criterion masks poor performance. |
| `sensitivity-analysis.j2` | Perform sensitivity analysis on decision rankings by perturbing weights. Identifies rank reversals, critical weights, and classifies overall decision robustness. |
| `weight-and-score.j2` | Weight criteria and score alternatives using the specified weighting method (direct or swing). Produces raw weights and raw scores; composites and ranking are computed by `lisp_eval`. Context: `decision_question` (string), `criteria` (array of `{name, type}`), `alternatives` (array of `{name, scores}`), `weighting_method` (`direct` or `swing`). |

To render a template, call the `render_template` tool with the template ref (e.g., `mcda/identify-criteria`) and a context object with the required variables.

## Constraints

- `identify-criteria.j2`: Public.
- `rank-alternatives.j2`: Public.
- `sensitivity-analysis.j2`: Public.
- `weight-and-score.j2`: Public.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
