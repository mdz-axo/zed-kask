---
name: eqm
description: "Explanation Quality Markers: score the predictive 12 with market_score_rationale, retain the 60-marker reference catalog, aggregate complete results, validate against realized outcomes, and improve a rationale through a bounded evidence-grounded loop."
---

# EQM — Explanation Quality Markers (Measure and Improve)

Measurement instrument for forecast-rationale quality, grounded in Karvetski,
Huang, Kučinskas et al. (2026), "Measuring Judgment Quality in Natural-Language
Explanations: Evidence from Forecasting Tournaments" — Forecasting Research
Institute. The paper defines 60 theory-guided patterns; the live `market_score_rationale` tool scores the predictive 12 using an LLM. Aggregate only complete tool results to forecast- and forecaster-level composites,
validates against realized outcomes, and emits calibration feedback.

## Initial and target condition

- **Initial condition:** the exact rationale corpus, its question/probability/forecaster IDs, the requested marker set, the scorer's returned 12-marker coverage, and any matched realized outcomes. Missing scores or outcomes are unavailable evidence, not zero quality or proof against gaming.
- **Target condition:** a complete predictive-12 measurement with its red-flag limitations and source model named, or an explicit incomplete/unsupported result. In Improve, preserve the original forecast probability and sourced facts while meeting *each* selected marker's directional target; outcome validation is separate and requires comparable subsequent resolved forecasts.

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

1. Admit only `predictive_12`, the MCP tool's fixed KEY_EQMS set. The 60-marker catalog is reference material, not a 60-marker scorer; a `full_60` or `domain_tuned` request returns `unsupported_subset` and stops before paid tool calls.
2. Gather the rationale corpus: array of {rationale, forecast_probability,
   question, forecaster_id?} objects.
3. Prepare the scoring batch; label ~$0.007 per rationale as the paper/tool's indicative estimate, not an observed provider charge.

### eqm-score (P scorer, tool-owned — `market_score_rationale`; calibrated by eqm-validate against outcomes)

1. Call `market_score_rationale` (hkask-mcp-prediction-markets) per rationale.
2. Collect per-rationale EqmResult: composite_score, scores, red_flags,
   green_flags, interpretation, model, caveat, missing_eqms.
3. The MCP tool is the single source of truth for the 12-EQM LLM scoring; do not reimplement it. A failed tool call propagates as a batch failure. For each result, load the 12 IDs marked `predictive: true` in `eqm-catalog.yaml` and compare them to the returned `scores[].id` using `lisp_eval`: `(begin (define all-present (lambda (xs ys) (if (is_null xs) t (and (member (car xs) ys) (all-present (cdr xs) ys))))) (and (= (length expected_ids) 12) (= (length expected_ids) (length observed_ids)) (all-present expected_ids observed_ids) (all-present observed_ids expected_ids)))`. A nonempty `missing_eqms`, an absent/duplicate ID, or any failed call makes the batch incomplete. Retain raw evidence but stop numeric aggregation and overconfidence-bias calculation; zero-filled missing markers are not observed low-quality scores.

### eqm-aggregate (D — `lisp_eval` over the tool's scores)

1. An empty corpus has no mean or bias: report `unassessed` and stop. Only after every requested rationale has a successful, complete predictive-12 result, render `eqm/eqm-aggregate` to group the scores; compute each forecast-level
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
2. Do not infer gaming from the same forecast before/after a probability-preserving rationale rewrite: its Brier score is mathematically unchanged. Only comparable *subsequent* resolved forecasts with independent outcomes can support a gaming signal; with no such cohort, report `Undetermined` and do not assert `gaming_suspected: false` as proof of safety.
3. If realized outcomes or a comparable validation cohort are absent → `Undetermined` (not Ready-with-empty). Pass the actually computed, sample-size-qualified Brier/correlation results into `eqm/eqm-validate`; template rendering never performs those calculations.

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
   Compute the target-set summary with `lisp_eval`
   `(- (sum helps_targets) (sum hurts_targets))` unless the operator set it. This subset summary is not the whole-rationale composite and cannot by itself close the loop.
4. **Predict** (P — calibrated by operator-scored Brier on the recorded goal) — render `eqm/eqm-imp-predict`: "intervention X raises marker Y
   from A to B", with a confidence. Record it with `kanban_goal_create`
   (`goal_text` `eqm: <prediction>`, the confidence as `prediction`) so the
   operator can score it.
5. **Experiment** (P — critiqued by re-scoring and the three mitigations) — render `eqm/eqm-imp-experiment` and produce the rewrite
   under the three mitigations.
6. **Check** (D) — re-score the *same predictive-12* set, requiring complete coverage and the original probability; compare the submitted forecast probability to the original with `lisp_eval` `(= original_probability revised_probability)`. Join each selected `marker_target.id` to its fresh score; missing/duplicate IDs, an unknown direction, or a score outside 0–2 are not a pass. Call `lisp_eval` with the `marker_targets` records (`id`, `direction`, `current_score` from the new tool response, `target_score`) and the pinned `misses` form below; judge the goal (`kanban_goal_judge`) with actual per-marker evidence. The intake Brier score arrives only when the operator resolves the goal.
7. **Act** — a nonempty failing-marker list names the next experiment; if empty and at least one target was checked, stop. Stop immediately on a *supported* `gaming_suspected` verdict, or after 8 iterations with remaining markers named. Missing outcomes leave external accuracy `Undetermined`, never a claim that the rationale is proven ungamed.

### Convergence

The local stop signal is per-marker, not a Cauchy criterion or the difference between a targeted-subset summary and a whole-rationale composite. After matching unique marker IDs and requiring a nonempty target list, call `lisp_eval`:

```lisp
(begin (define misses (lambda (items) (if (is_null items) (list) (let ((item (car items))) (if (if (string= (assoc "direction" item) "hurts") (<= (assoc "current_score" item) (assoc "target_score" item)) (>= (assoc "current_score" item) (assoc "target_score" item))) (misses (cdr items)) (cons (assoc "id" item) (misses (cdr items)))))))) (misses marker_targets))
```

An empty result closes only when every selected marker has a fresh score and the original forecast probability is unchanged. A positive helps marker cannot offset an off-target hurts marker.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `eqm-select.j2` | Admit the live predictive 12; return unsupported for full-60/domain-tuned requests, prepare a sourced scoring batch and indicative cost. |
| `eqm-score.j2` | Score rationales via the market_score_rationale MCP tool. Collect per-rationale EqmResult: composite_score, scores, red_flags, green_flags. The MCP tool is the single source of truth for 12-EQM LLM scoring. |
| `eqm-aggregate.j2` | Aggregate per-rationale scores to forecast-level and forecaster-level composites. Apply the asymmetric decision rule: red_flag_screen (high confidence) vs green_flag_endorsement (weak). Compute overconfidence_bias. |
| `eqm-validate.j2` | Interpret caller-computed Brier/correlation over comparable resolved outcomes; unchanged probability on the same forecast cannot diagnose gaming. Missing cohorts remain Undetermined. |
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
- Maximum 10 bounded measurement requests; the Improve loop stops at 8 re-score/rewrite iterations or sooner on per-marker target attainment or a supported gaming signal.
- The Improve loop preserves the forecast probability and never fabricates evidence; each prediction names a specific intervention and marker.
- The convergence decision is deterministic (lisp_eval compute step) — no LLM convergence-check template.
- The MCP tool `market_score_rationale` is the single source of truth for 12-EQM scoring; this skill adds the measurement procedure around it.
- Never fabricate EQM scores — if the MCP tool call fails, propagate the error (do not default to 0).
- If the MCP tool returns `missing_eqms` (non-empty), flag the result as an incomplete assessment — the composite is pulled toward 0 for those dimensions and the operator should not interpret a low score as "low quality" when EQMs are missing.
