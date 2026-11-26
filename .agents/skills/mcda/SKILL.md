---
name: mcda
description: "Multi-Criteria Decision Analysis. Identifies decision criteria, weights and scores alternatives, ranks options with compensation masking detection, and performs sensitivity analysis to assess decision robustness.
"
audience: user
---

# Mcda

Multi-Criteria Decision Analysis. Identifies decision criteria, weights and scores alternatives, ranks options with compensation masking detection, and performs sensitivity analysis to assess decision robustness.


## Reference models

Belton & Stewart, *Multiple Criteria Decision Analysis: An Integrated Approach* (2002) — weighted-sum value model and sensitivity analysis; Keeney & Raiffa, *Decisions with Multiple Objectives* (1976) — swing weighting. `onto_anchor` → `multi_criteria_decision_analysis` resolves at tier `derived` (`derived:multi_criteria_decision_analysis`, Belton & Stewart 2002; Keeney & Raiffa 1976, operator ruling 2026-09-25), with `cso:multi-criteria_decision_analysis` (`tier: domain_supplement`, CSO 3.5) in `alternatives` — verified 2026-11-19.

**D/P labelling.** Criteria, classification, weights and raw scores are P (judgment — elicited through `kask.models.decision_model` via the `mcda_weight_score` tool per the S15 clause, not typed by the thread model; critique: the operator, and the sensitivity analysis shows how much the decision depends on them). Normalization, composites, ranking, compensation-masking threshold detection (`MCDA-2`), perturbation and robustness class are D (`lisp_eval`, helpers below).

## Initial and target condition

- **Initial condition:** the decision question, the enumerated alternatives, and any operator-fixed weights or veto criteria.
- **Target condition:** a ranking whose robustness class is `robust` or `moderate`, with every compensation warning reported — or, after the bound, a fragile ranking reported with its critical weights for the operator to decide.

## Deterministic helpers (`lisp_eval`)

Prepend these to each form. They compute over the weights and scores supplied; they do not vouch for those judgments.

```lisp
(define sum (lambda (l) (if (is_null l) 0 (+ (car l) (sum (cdr l))))))
(define all-nonneg (lambda (l) (if (is_null l) true (and (numberp (car l)) (>= (car l) 0) (all-nonneg (cdr l))))))
(define scale (lambda (l k) (if (is_null l) (quote ()) (cons (* (car l) k) (scale (cdr l) k)))))
(define norm (lambda (w) (if (and (listp w) (not (is_null w)) (all-nonneg w) (> (sum w) 0)) (scale w (/ 1 (sum w))) "invalid_weights")))
(define norm-scores (lambda (raw) (scale raw (/ 1 10))))
(define dot-raw (lambda (a b) (if (is_null a) 0 (+ (* (car a) (car b)) (dot-raw (cdr a) (cdr b))))))
(define dot (lambda (a b) (if (and (listp a) (listp b) (= (length a) (length b))) (dot-raw a b) "length_mismatch")))
(define count-gt (lambda (v xs) (if (is_null xs) 0 (+ (if (> (car xs) v) 1 0) (count-gt v (cdr xs))))))
(define ranks (lambda (xs all) (if (is_null xs) (quote ()) (cons (+ 1 (count-gt (car xs) all)) (ranks (cdr xs) all)))))
(define bump (lambda (w i k) (if (is_null w) (quote ()) (cons (if (= i 0) (* (car w) k) (car w)) (bump (cdr w) (- i 1) k)))))
(define argmax (lambda (w alts) (let ((nw (norm w))) (if (not (listp nw)) "invalid_weights" (if (or (not (listp alts)) (is_null alts)) "invalid_alts" (let ((best (lambda (as i bi bv) (if (is_null as) bi (let ((v (dot nw (car as)))) (if (not (numberp v)) "length_mismatch" (if (> v bv) (best (cdr as) (+ i 1) i v) (best (cdr as) (+ i 1) bi bv)))))))) (best alts 0 -1 -1)))))))
(define top (argmax W ALTS))
(define flips (lambda (d) (let ((chk (lambda (i) (if (= i (length W)) 0 (if (or (!= (argmax (bump W i (+ 1 d)) ALTS) top) (!= (argmax (bump W i (- 1 d)) ALTS) top)) 1 (chk (+ i 1))))))) (chk 0))))
(define scan (lambda (ds) (if (is_null ds) "none" (if (= (flips (car ds)) 1) (car ds) (scan (cdr ds))))))
```

env: `W` = raw weights (one per criterion, non-negative with positive sum), `ALTS` = one list per alternative of normalized scores (`(norm-scores <raw_alt>)`, i.e., `raw / 10`) in criterion order. Composites: `(dot (norm W) <alt>)`; standard competition ranks (ties share rank and skip next): `(ranks composites composites)`. `min_flip`: `(scan (quote (0.01 0.02 0.03 0.05 0.1)))` — the smallest relative one-weight change that flips the top choice, or `"none"` within 10%. `argmax` keeps the first alternative on ties and returns `"invalid_weights"`, `"invalid_alts"`, or `"length_mismatch"` on malformed inputs; report ties explicitly.

## When to Use

- When a decision question involves multiple alternatives and you need to enumerate, classify (benefit or cost), and validate the independence of decision criteria before weighting.
- When you need to assign weights to criteria using a specified method (direct or swing) and score each alternative to produce normalized scores and composite rankings.
- When you need to rank alternatives by composite score and detect compensation masking — cases where strong performance on non-critical criteria hides poor performance on a critical criterion.
- When you need to assess decision robustness by perturbing criterion weights to identify rank reversals, critical weights, and classify overall stability.
- When you need to compute a convergence metric for an MCDA PDCA cycle to determine whether ranking confidence and sensitivity robustness are sufficient to stop iterating.

## When NOT to Use

- Decisions with fixed methodology weights — when the weights are fixed by firm methodology (e.g. `skill-discovery`'s fixed 0.50 / 0.25 / 0.25 route fit-score weights), a `lisp_eval` scoring call is the honest instrument; MCDA adds ceremony (the essentialist Surface gate rejection).
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
8. (D) Verify criteria count (3–12) and independence flag reconciliation (all `dependent_corrs` > 0.7 and matching `flagged_count`) in `lisp_eval` (`MCDA-1`):
   - form: `(begin (define all-gt (lambda (xs t) (if (is_null xs) true (and (numberp (car xs)) (> (car xs) t) (all-gt (cdr xs) t))))) (and (listp criteria) (>= (length criteria) 3) (<= (length criteria) 12) (listp dependent_corrs) (all-gt dependent_corrs 0.7) (numberp flagged_count) (= (length dependent_corrs) flagged_count)))`
   - env: `{ "criteria": <identify-criteria.criteria>, "dependent_corrs": <estimated_correlation values from dependent_pairs>, "flagged_count": <int> }`
   - receipts:
     ```lisp-receipts
     {"gate": "MCDA-1", "case": "valid-three-criteria-one-dependent", "env": {"criteria": ["a", "b", "c"], "dependent_corrs": [0.75], "flagged_count": 1}, "expected": true}
     {"gate": "MCDA-1", "case": "too-few-criteria-fails", "env": {"criteria": ["a", "b"], "dependent_corrs": [], "flagged_count": 0}, "expected": false}
     {"gate": "MCDA-1", "case": "sub-threshold-corr-fails", "env": {"criteria": ["a", "b", "c"], "dependent_corrs": [0.65], "flagged_count": 1}, "expected": false}
     {"gate": "MCDA-1", "case": "count-mismatch-fails", "env": {"criteria": ["a", "b", "c"], "dependent_corrs": [0.8], "flagged_count": 0}, "expected": false}
     ```

### weight-and-score

1. Call the `mcda_weight_score` tool with the identify-criteria pass-through: `decision_question`, `criteria` (the structured criterion objects — name, benefit-or-cost type, description), `alternatives` (each `{name, description}`), and `weighting_method` (`direct` or `swing`). The tool routes the semantic halves through `kask.models.decision_model` (the Decisions API — never a chat endpoint) as ONE call of typed Score questions over the fixed 1–10 answer space: one weight question per criterion under the named weighting method (swing carries the all-criteria-at-worst-level rubric), one score question per (alternative, criterion) with cost criteria pre-inverted (1 = worst/most costly, 10 = best/least costly). It returns `weights` (one per criterion, in criterion order) and `raw_scores` (one per alternative × criterion) — the shape the former weight-and-score render produced.
2. (D) Weights are normalized by `(norm W)` in `lisp_eval`; the tool's weights are not.
3. (D) Normalize scores to 0–1 via `(norm-scores <raw_alt>)` in `lisp_eval` (`raw / 10`) — `ALTS` is one list per alternative of normalized scores in criterion order.
4. (D) Compute composites with `lisp_eval` `(dot (norm W) <alt>)` for each alternative — the weighted sum model.
5. (D) Compute standard competition ranks with `lisp_eval` `(ranks composites composites)` (rank 1 = highest composite; identical composites receive the same rank and skip the next rank).

**S15 clause.** The bounded weight and score judgments (each a calibrated score over the fixed 1–10 answer space) route through `kask.models.decision_model` with fail-visible resolution: an unset model is a typed refusal naming the setting, a private-provenance call refuses before any dispatch, and a missing judgment refuses rather than fabricating. The oracle for their deterministic consequences — normalization, composites, ranking, `MCDA-2`, `min_flip` — is the named `lisp_eval` gates.

### sensitivity-analysis

1. (D) Detect compensation masking on the top-ranked alternative in `lisp_eval` (`MCDA-2`), normalizing `crit_weights` via `(norm crit_weights)`, honoring an optional `essential_mask` (boolean per criterion, or `[]` when none are marked essential), and returning the list of masked 0-based criterion indices (`[]` = no masking, `"invalid_input"` on malformed weights/scores/lengths):
   - form: `(begin (define sum (lambda (l) (if (is_null l) 0 (+ (car l) (sum (cdr l)))))) (define all-nonneg (lambda (l) (if (is_null l) true (and (numberp (car l)) (>= (car l) 0) (all-nonneg (cdr l)))))) (define scale (lambda (l k) (if (is_null l) (quote ()) (cons (* (car l) k) (scale (cdr l) k))))) (define norm (lambda (w) (if (and (listp w) (not (is_null w)) (all-nonneg w) (> (sum w) 0)) (scale w (/ 1 (sum w))) "invalid_weights"))) (define all-unit (lambda (l) (if (is_null l) true (and (numberp (car l)) (>= (car l) 0) (<= (car l) 1) (all-unit (cdr l)))))) (define masked-idx (lambda (ws ss es i thresh) (if (is_null ws) (quote ()) (let ((rest (masked-idx (cdr ws) (cdr ss) (if (is_null es) (quote ()) (cdr es)) (+ i 1) thresh)) (ess (if (is_null es) false (car es)))) (if (and (or (> (car ws) 0.1) ess) (< (car ss) thresh)) (cons i rest) rest))))) (let ((nw (norm crit_weights)) (es (if (listp essential_mask) essential_mask (quote ())))) (if (and (listp nw) (listp top_scores) (= (length nw) (length top_scores)) (all-unit top_scores) (numberp danger_threshold) (>= danger_threshold 0) (<= danger_threshold 1) (or (is_null es) (= (length es) (length nw)))) (masked-idx nw top_scores es 0 danger_threshold) "invalid_input")))`
   - env: `{ "crit_weights": <raw or normalized weights>, "top_scores": <normalized scores of top alternative>, "essential_mask": <[] or bools per criterion>, "danger_threshold": <0.3 unless veto level fixed> }`
   - receipts:
     ```lisp-receipts
     {"gate": "MCDA-2", "case": "raw-weights-non-critical-not-masked", "env": {"crit_weights": [1, 10, 10], "top_scores": [0.2, 0.9, 0.9], "essential_mask": [], "danger_threshold": 0.3}, "expected": []}
     {"gate": "MCDA-2", "case": "essential-overrides-low-weight", "env": {"crit_weights": [1, 10, 10], "top_scores": [0.2, 0.9, 0.9], "essential_mask": [true, false, false], "danger_threshold": 0.3}, "expected": [0]}
     {"gate": "MCDA-2", "case": "critical-weight-below-threshold-masked", "env": {"crit_weights": [5, 3, 2], "top_scores": [0.2, 0.9, 0.9], "essential_mask": [], "danger_threshold": 0.3}, "expected": [0]}
     {"gate": "MCDA-2", "case": "negative-weight-refused", "env": {"crit_weights": [-1, 2, 0], "top_scores": [0.2, 0.9, 0.9], "essential_mask": [], "danger_threshold": 0.3}, "expected": "invalid_input"}
     {"gate": "MCDA-2", "case": "zero-sum-weights-refused", "env": {"crit_weights": [0, 0, 0], "top_scores": [0.2, 0.9, 0.9], "essential_mask": [], "danger_threshold": 0.3}, "expected": "invalid_input"}
     ```
2. (D) Compute `min_flip` with `lisp_eval` (helpers above): each weight is perturbed one at a time by ±1%, 2%, 3%, 5% and 10% of its value, renormalized, and the composites recomputed; `min_flip` is the smallest change that changes the top choice.
3. (D) Identify critical weights: for each weight, the same grid gives the smallest change that flips the top choice, its direction, and the new top choice.
4. (D) Classify robustness from `min_flip`: **robust** = `"none"` (no flip within 10%); **moderate** = smallest flip above 5% and at most 10%; **fragile** = a flip at 5% or less.
5. (P) Render `mcda/sensitivity-analysis` with the full contract context (`decision_question`, `ranking`, `weights`, `normalized_scores`, `criteria`, `danger_threshold`, `min_flip`, `critical_weights`) to identify each alternative's key strength and weakness, interpret compensation masking warnings, and assess what the critical weights mean for the decision.
6. If the criteria independence check from Stage 1 identified dependent pairs (correlation >0.7), warn that OAT perturbation underestimates true sensitivity and suggest a combined perturbation test shifting both correlated weights simultaneously.
7. Provide an actionable recommendation addressing whether to proceed, gather more data, add a veto criterion, or restructure criteria.

### Convergence

8. Gate — call `lisp_eval` with:
   - form: `(or (and (stringp min_flip) (string= min_flip "none")) (and (numberp min_flip) (> min_flip 0.05)))`
   - env: `{ "min_flip": <the lisp_eval min_flip result> }`
   - receipts:
     ```lisp-receipts
     {"gate":"MCDA-converge","case":"robust-none-closes","env":{"min_flip":"none"},"expected":true}
     {"gate":"MCDA-converge","case":"moderate-on-grid-0.1-closes","env":{"min_flip":0.1},"expected":true}
     {"gate":"MCDA-converge","case":"exact-0.05-fragile-fails","env":{"min_flip":0.05},"expected":false}
     {"gate":"MCDA-converge","case":"fragile-0.03-fails","env":{"min_flip":0.03},"expected":false}
     {"gate":"MCDA-converge","case":"invalid-string-fails","env":{"min_flip":"bad"},"expected":false}
     ```
   Robust or moderate outcomes (no flip at 5% or less) close the loop. A
   fragile outcome re-enters weight-and-score once with the critical-weight
   findings — add a veto criterion or restructure the criteria. Bound: max 2 restructurings; a third
   fragile outcome is reported honestly with the critical weights — the
   decision is the operator's, not the loop's.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `identify-criteria.j2` | Identify and classify decision criteria as benefit or cost dimensions. Validates criteria independence and produces a structured criterion set. |
| `sensitivity-analysis.j2` | Synthesize the `lisp_eval`-computed ranking, compensation masking check (`MCDA-2`), and perturbation results (`min_flip`, `critical_weights`): top choice, compensation warnings, rank-reversal implications, and robustness class. |

To render a template, call the `render_template` tool with the template ref (e.g., `mcda/identify-criteria`) and a context object with the required variables.

Copyable `render_template` context blocks (field names and types from each template's `[inference]` contract):

`mcda/identify-criteria`:
```json
{
  "decision_question": "<decision question>",
  "alternatives": [],
  "criteria_hints": []
}
```

`mcda/sensitivity-analysis`:
```json
{
  "decision_question": "<decision question>",
  "ranking": [],
  "weights": [],
  "normalized_scores": [],
  "criteria": [],
  "danger_threshold": 0.3,
  "min_flip": "none",
  "critical_weights": []
}
```

## Regression case

Run the deterministic helpers block through `lisp_eval` over a stable env
(`W = (0.5 0.3 0.2)`, `ALTS = ((0.8 0.6 0.4) (0.4 0.9 0.7))`):
composites 0.66 / 0.61, top = alternative 0, `min_flip = "none"`
(robust — no flip within 10%). Run it over a fragile env
(`W = (0.34 0.33 0.33)`, `ALTS = ((0.6 0.5 0.5) (0.5 0.6 0.5))`):
`min_flip = 0.03` (fragile — a flip at 3%). Run the convergence gate both
ways: `min_flip = "none"` → true (close the loop); `min_flip = 0.03` →
false (fragile re-enters weight-and-score with the critical-weight
findings). All receipts executed through the live tool 2026-09-29.

## Constraints

- Composites, competition ranks, `MCDA-2` masked indices, and `min_flip` are computed by `lisp_eval` (the helpers block) — a render never re-ranks or invents masked criteria.
- `argmax` keeps the first alternative on ties; report ties explicitly,
  never silently.
- Raw scores are 1–10 (the tool's fixed ten-level answer space);
  normalized = raw / 10 (1–10 → [0.1, 1.0]); cost criteria are
  pre-inverted (1 = most costly/worst, 10 = least costly/best). Weights
  are the tool's 1–10 judgments — raw, never normalized at the source;
  `(norm W)` owns normalization.
- The danger threshold is supplied explicitly as `danger_threshold` (0.3
  unless the decision question fixes a veto level — the render contract
  requires the field); a critical criterion (weight > 0.1 or marked
  essential by the decision question) below it is a compensation warning
  — severity minor (non-critical) or major (critical).
- Dependent criteria pairs (correlation > 0.7) make one-at-a-time
  perturbation underestimate sensitivity — apply the combined
  perturbation the sensitivity step suggests (composable as
  `(bump (bump W i k) j k)` over the correlated pair) when the operator
  accepts it.
- The loop bound is 2 restructurings; a third fragile outcome is
  reported honestly with its critical weights — the decision is the
  operator's, not the loop's.

