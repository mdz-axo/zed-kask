---
name: eqm
description: "Explanation Quality Markers: measure forecast rationales against 60 EQMs via market_score_rationale, aggregate to composites, validate against realized outcomes (Brier), emit overconfidence_bias, and improve a rationale with an in-session PDCA loop that preserves its probability and grounds evidence in real sources."
---

# EQM — Explanation Quality Markers (Measure and Improve)

Measurement instrument for forecast-rationale quality, grounded in Karvetski,
Huang, Kučinskas et al. (2026), "Measuring Judgment Quality in Natural-Language
Explanations: Evidence from Forecasting Tournaments" — Forecasting Research
Institute. Scores rationales against 60 theory-guided reasoning patterns (EQMs)
using an LLM, aggregates to forecast-level and forecaster-level composites,
validates against realized outcomes, and emits calibration feedback.

## When to Use

- When you need to score a forecast rationale against a validated, peer-reviewed
  instrument (not an LLM self-grade).
- When you need a forecaster-level quality signal (the paper's r=0.51 finding).
- When you need to detect gaming (EQM scores rose but accuracy didn't improve on
  realized outcomes).
- When you need to emit an overconfidence_bias signal back to superforecasting's
  calibration-adjustment step.
- When you need to improve a rationale's EQM passage rate by reverse-engineering what each marker's score of 2 looks like (the Improve phase), with gaming detection.

## When NOT to Use

- To *produce* a forecast — use `superforecasting`.
- To self-assess forecast quality — use superforecasting's `forecast-quality-gate`.
- To self-assess without an external instrument — use `metacognition`.

## Ontological Anchors

| Anchor | How it shapes the skill |
|---|---|
| Karvetski et al. (2026) — the EQM method (`onto_anchor` → derived `explanation_quality_marker`, operator ruling 2026-09-25) | 60 markers, LLM-scored 0/1/2, composite, asymmetric signal, forecast-level + forecaster-level prediction. Defines the Score → Aggregate → Validate → Feedback shape. |
| Tetlock Brier scoring (via superforecasting) | The outcome ground truth that validates EQM scores against accuracy. |
| PKO (Procedural Knowledge Ontology) | The skill models a measurement procedure: specification (which EQMs, which rationales) / execution (LLM scoring) / verification (outcome correlation). |
| Dublin Core | Metadata for the rationale corpus (forecaster id, question id, timestamp, resolution status) needed by forecaster-level aggregation. |

## Asymmetric Signal (the paper's central finding)

EQMs flag bad forecasts more reliably than they identify excellent ones. The
skill's decision rule encodes this asymmetry:

- **Red-flag screen (high confidence):** ≥2 red flags at score 2 → flag rationale
  as likely underperformer. This is the paper's strong signal.
- **Green-flag endorsement (weak):** high composite with no red flags → *weak*
  positive endorsement, not a "strong rationale" claim. Reserve "strong" for
  forecasters whose EQM composite correlates with accuracy on realized outcomes.

## Instructions

### Deterministic helpers (D — `lisp_eval`)

Every mean, Brier score and correlation in this skill is computed with these definitions, prepended to the form. `lisp_eval` computes over the values supplied; it does not vouch for where they came from (the MCP scorer and the recorded outcomes do).

```lisp
(define sum (lambda (l) (if (is_null l) 0 (+ (car l) (sum (cdr l))))))
(define mean (lambda (l) (/ (sum l) (length l))))
(define zip* (lambda (a b) (if (is_null a) (quote ()) (cons (* (car a) (car b)) (zip* (cdr a) (cdr b))))))
(define dev (lambda (l m) (if (is_null l) (quote ()) (cons (- (car l) m) (dev (cdr l) m)))))
(define brier (lambda (p o) (if (is_null p) (quote ()) (cons (* (- (car p) (car o)) (- (car p) (car o))) (brier (cdr p) (cdr o))))))
(define pearson (lambda (x y) (let ((dx (dev x (mean x))) (dy (dev y (mean y)))) (/ (sum (zip* dx dy)) (sqrt (* (sum (zip* dx dx)) (sum (zip* dy dy))))))))
```

Correlation requires ≥5 pairs; below that report `Undetermined` (matches the scenarios server's 5-resolved-forecast floor). A zero-variance series makes `pearson` divide by zero — report `Undetermined` for it too.

### eqm-select (P — subset choice; critique: operator)

1. Choose the EQM subset: `predictive_12` (default, matches the MCP tool's
   KEY_EQMS), `full_60` (research/validation), or `domain_tuned`.
2. Gather the rationale corpus: array of {rationale, forecast_probability,
   question, forecaster_id?} objects.
3. Prepare the scoring batch and cost estimate (~$0.007 per rationale).

### eqm-score (P scorer, tool-owned — `market_score_rationale`; calibrated by eqm-validate against outcomes)

1. Call `market_score_rationale` (hkask-mcp-prediction-markets) per rationale.
2. Collect per-rationale EqmResult: composite_score, scores, red_flags,
   green_flags, interpretation, model, caveat, missing_eqms.
3. The MCP tool is the single source of truth for the 12-EQM LLM scoring;
   this skill does not re-implement it. If the tool returns `missing_eqms`
   (non-empty), flag the result as an incomplete assessment — the composite
   is pulled toward 0 for those dimensions.

### eqm-aggregate (D — `lisp_eval` over the tool's scores)

1. Render `eqm/eqm-aggregate` to group the scores; compute each forecast-level
   composite as `(mean <composites for the question>)` via `lisp_eval`.
2. Compute each forecaster-level composite as `(mean <that forecaster's
   composites>)` — the paper's r=0.51 signal.
3. Apply the asymmetric decision rule: red_flag_screen (high confidence) vs
   green_flag_endorsement (weak).
4. Compute overconfidence_bias with `lisp_eval` from the four listed marker
   sums: `(/ (- (+ ec frm) (+ sr bp)) (* 2 n))`, env the sums of
   `extreme_confidence`, `forecast_rationale_misalign`,
   `statistical_reasoning`, `best_practices` and `n` rationales. Positive =
   overconfident, negative = underconfident.

### eqm-validate (D — `lisp_eval` Brier and Pearson; ground truth: realized outcomes)

1. If realized_outcomes are present: compute per-forecaster Brier as
   `(mean (brier ps os))` and the composite–accuracy correlation as
   `(- 0 (pearson composites briers))` via `lisp_eval` (negated because lower
   Brier = better, so positive = higher EQM with better accuracy); Undetermined below 5 forecasters. Check directional-hypothesis
   match (paper's >90% finding).
2. If EQM scores rose but accuracy didn't improve → emit `gaming_suspected`
   verdict (halts the Improve loop).
3. If realized_outcomes absent → return `Undetermined` (not Ready-with-empty —
   per the advertised-invariants rule).

### Improve (optional) — in-session PDCA on one rationale

Run only when asked to improve a rationale. The EQM descriptions are the
failing tests, the rationale is the code, the rewrite is the green phase and
re-scoring is the test run (Karvetski et al. 2026; Improvement Kata). The loop
runs within this session and improves the *rationale*; it never evaluates the
skill itself.

**Gaming the scorer is the central risk.** When an LLM rewrites text to score
higher on an LLM-scored instrument, the score can rise without better
reasoning. All three mitigations are required:

1. **Evidence grounding.** For `fact_based` / `statistical_reasoning`, find a
   real base rate via `superforecasting` (stage_2_outside_view) or `web_search`;
   for `confirmation_bias`, get genuine opposing hypotheses from
   `falsifiability/falsifiability-hypothesize`. With no real evidence, record
   the gap instead of writing a plausible sentence.
2. **Probability preservation.** The rewrite supports the same
   `forecast_probability`; `forecast_rationale_align` checks it on re-score.
3. **Outcome validation.** `eqm-validate`'s `gaming_suspected` verdict halts the
   loop.

Steps:

1. **Direction** — render `eqm/eqm-imp-direction`: raise EQM passage rate,
   red-flag elimination before green-flag polish; confirm the probability to
   preserve and the evidence sources available.
2. **Current condition** — render `eqm/eqm-imp-current` over a fresh
   `market_score_rationale` result (failing markers, red-flag screen, composite).
3. **Target** (P targets; D composite) — render `eqm/eqm-imp-target`: marker-level targets from each
   EQM description, red flags first, one step beyond the current condition.
   Compute `target_composite` with `lisp_eval`
   `(- (sum helps_targets) (sum hurts_targets))` unless the operator set it.
4. **Predict** (P — calibrated by operator-scored Brier on the recorded goal) — render `eqm/eqm-imp-predict`: "intervention X raises marker Y
   from A to B", with a confidence. Record it with `kanban_goal_create`
   (`goal_text` `eqm: <prediction>`, the confidence as `prediction`) so the
   operator can score it.
5. **Experiment** (P — critiqued by re-scoring and the three mitigations) — render `eqm/eqm-imp-experiment` and produce the rewrite
   under the three mitigations.
6. **Check** (D) — re-score via `market_score_rationale`; `lisp_eval`
   `(abs (- target_score current_score))` per marker for the gap; judge the
   goal (`kanban_goal_judge`) with the measured marker level. The Brier score
   arrives when the operator scores the goal.
7. **Act** (D stop rule) — stop at gap ≤ epsilon, a `gaming_suspected` verdict, or 8
   iterations; otherwise re-enter step 2 with the new rationale.

### Convergence

Cauchy criterion on the forecaster-level composite across iterations. The
convergence signal is the marker-space gap (distance from current composite to
target composite), computed deterministically via lisp_eval:

```lisp
(let ((gap (abs (- target_composite current_composite)))) (if (<= gap epsilon) 'converged 'continue))
```

env: `{ "target_composite": <target>, "current_composite": <latest iteration>, "epsilon": 0.05 }`.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `eqm-select.j2` | Choose the EQM subset (predictive_12 default, full_60, or domain_tuned) and gather the rationale corpus. Prepares the scoring batch and cost estimate. |
| `eqm-score.j2` | Score rationales via the market_score_rationale MCP tool. Collect per-rationale EqmResult: composite_score, scores, red_flags, green_flags. The MCP tool is the single source of truth for 12-EQM LLM scoring. |
| `eqm-aggregate.j2` | Aggregate per-rationale scores to forecast-level and forecaster-level composites. Apply the asymmetric decision rule: red_flag_screen (high confidence) vs green_flag_endorsement (weak). Compute overconfidence_bias. |
| `eqm-validate.j2` | If realized_outcomes present: correlate EQM composite with accuracy (Brier), check directional-hypothesis match. If scores rose but accuracy didn't improve → gaming_suspected verdict. If outcomes absent → Undetermined (not Ready-with-empty). |
| `eqm-imp-direction.j2` | Improve step 1: direction — raise passage rate, red flags first; confirm the probability to preserve. |
| `eqm-imp-current.j2` | Improve step 2: current condition from a fresh `market_score_rationale` result. |
| `eqm-imp-target.j2` | Improve step 3: marker-level targets from EQM descriptions, red flags first. |
| `eqm-imp-predict.j2` | Improve step 4: a specific intervention-to-marker prediction with confidence. |
| `eqm-imp-experiment.j2` | Improve step 5: rewrite the rationale for each failing marker with real evidence, preserving the probability. |
| `eqm-catalog.yaml` | Reference: the full 60 EQM definitions from Karvetski et al. (2026), organized by category (good_habits / warning_signs). The 12 most predictive are marked predictive: true. Single source of truth for EQM definitions; the MCP tool's KEY_EQMS const carries the predictive 12. |

To render a template, call the `render_template` tool with the template ref (e.g., `eqm/eqm-select`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `eqm-aggregate.j2`: `per_rationale_scores`,`forecaster_groups`
- `eqm-score.j2`: `scoring_batch`,`selected_subset`
- `eqm-imp-predict.j2`: `target_condition`,`prioritized_markers` `current_composite`,`target_composite_score`


## Constraints

- All flow templates have Public visibility.
- Maximum 10 measurement iterations; the Improve loop stops at 8 (each iteration re-scores and rewrites).
- The Improve loop preserves the forecast probability and never fabricates evidence; each prediction names a specific intervention and marker.
- The convergence decision is deterministic (lisp_eval compute step) — no LLM convergence-check template.
- The MCP tool `market_score_rationale` is the single source of truth for 12-EQM scoring; this skill adds the measurement procedure around it.
- Never fabricate EQM scores — if the MCP tool call fails, propagate the error (do not default to 0).
- If the MCP tool returns `missing_eqms` (non-empty), flag the result as an incomplete assessment — the composite is pulled toward 0 for those dimensions and the operator should not interpret a low score as "low quality" when EQMs are missing.
