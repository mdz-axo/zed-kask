---
name: eqm
description: "Explanation Quality Markers: score the predictive 12 with market_score_rationale, retain the 60-marker reference catalog (the paper's verbatim pattern set), aggregate complete results, validate against realized outcomes, and improve a rationale through a bounded evidence-grounded loop."
audience: user
---

# EQM — Explanation Quality Markers (Measure and Improve)

Measurement instrument for forecast-rationale quality, grounded in Karvetski,
Huang, Kučinskas et al. (2026, arXiv:2606.30987), "Measuring Judgment Quality in Natural-Language
Explanations: Evidence from Forecasting Tournaments" — Forecasting Research
Institute. The paper defines the EQM instrument — 60 theory-guided marker patterns across six families; the reference catalog carries all 60 verbatim from Appendix A.4; the live `market_score_rationale` tool scores the predictive 12 using the decision model (`kask.models.decision_model` — one typed score question per marker, answered with calibrated per-level probabilities, never text). Aggregate only complete tool results to forecast- and forecaster-level composites,
validates against realized outcomes, and emits calibration feedback.

## Goal responsibilities

Follow [GOAL_INTERFACE.md](../../../kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md).
Explanation scores assess a rationale, not goal achievement or forecast calibration. Preserve the human acceptance boundary when reporting improvements.

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
| Karvetski et al. (2026, arXiv:2606.30987) — the EQM method (`onto_anchor` → derived `explanation_quality_marker`, operator ruling 2026-09-25) | The marker instrument (decision-model-scored 0/1/2), composite, asymmetric signal, forecast-level + forecaster-level prediction. Defines the Score → Aggregate → Validate → Feedback shape. |
| Tetlock Brier scoring (via superforecasting) | A loss measure on independently resolved forecasts, not ground truth or calibration by itself (Murphy 1973). |
| PKO (Procedural Knowledge Ontology) | The skill models a measurement procedure: specification (which EQMs, which rationales) / execution (decision-model scoring) / verification (outcome correlation). |
| Dublin Core | Metadata for the rationale corpus (forecaster id, question id, timestamp, resolution status) needed by forecaster-level aggregation. |

## Asymmetric Signal (the paper's central finding)

EQMs flag bad forecasts more reliably than they identify excellent ones. The
skill's decision rule encodes this asymmetry:

- **Red-flag screen (high confidence):** ≥2 red flags at score 2 → flag rationale
  as likely underperformer. This is the paper's strong signal.
- **Green-flag endorsement (weak):** high composite with no red flags → *weak*
  positive endorsement, not a "strong rationale" claim. Reserve "strong" for
  forecasters whose EQM composite correlates with accuracy on realized outcomes.

## D/P labelling

Marker identification over a rationale, red-flag interpretation, and
Improve-phase rationale revision are P — judgment, critiqued by the
alignment check against the stated forecast probability and by gaming
detection on realized outcomes. The scoring call is P too, executed by a
tool-owned decision model (`market_score_rationale`) and calibrated by eqm-validate
against outcomes. Everything derived from the scorer's values is D: subset
admission (`EQM-1`) and every Brier and correlation run in `lisp_eval`; the
completeness check, the forecast- and forecaster-level composites, the
red-flag/green-flag screens (`EQM-2`), the failing markers with their
priority order and targets, and `overconfidence_bias` are computed by the
`eqm/eqm-aggregate` render (Jinja over its own input — no model fills
anything in). The instrument measures the written rationale; it does not
vouch for the forecast.

## Instructions

### Deterministic helpers (D — `lisp_eval`)

Every Brier score and correlation in this skill is computed with these definitions, prepended to the form. `lisp_eval` computes over the values supplied; it does not vouch for where they came from (the MCP scorer and the recorded outcomes do).

```lisp
(define sum (lambda (l) (if (is_null l) 0 (+ (car l) (sum (cdr l))))))
(define mean (lambda (l) (/ (sum l) (length l))))
(define zip* (lambda (a b) (if (is_null a) (quote ()) (cons (* (car a) (car b)) (zip* (cdr a) (cdr b))))))
(define dev (lambda (l m) (if (is_null l) (quote ()) (cons (- (car l) m) (dev (cdr l) m)))))
(define brier (lambda (p o) (if (is_null p) (quote ()) (cons (* (- (car p) (car o)) (- (car p) (car o))) (brier (cdr p) (cdr o))))))
(define pearson (lambda (x y) (let ((dx (dev x (mean x))) (dy (dev y (mean y)))) (/ (sum (zip* dx dy)) (sqrt (* (sum (zip* dx dx)) (sum (zip* dy dy))))))))
(define outcome-r (lambda (composites briers) (if (< (length composites) 5) "Undetermined" (let ((dx (dev composites (mean composites))) (dy (dev briers (mean briers)))) (if (or (= (sum (zip* dx dx)) 0) (= (sum (zip* dy dy)) 0)) "Undetermined" (- 0 (pearson composites briers)))))))
```

`outcome-r` is the composite–accuracy correlation with its two guards inside the form: fewer than 5 pairs, or a zero-variance series, returns `"Undetermined"` (the scenarios server's 5-resolved-forecast floor). It is negated because a lower Brier is better, so positive means higher EQM with better accuracy.

### eqm-select (D — `lisp_eval` `EQM-1` subset gate)

1. Admit only `predictive_12`, the MCP tool's fixed KEY_EQMS set, using `lisp_eval` (`EQM-1` — subset admission and batch cost gate; no LLM wrapper template):
   - form: `(if (and (string= eqm_subset "predictive_12") (> batch_len 0) (<= batch_len 10)) (list "predictive_12" (* batch_len 0.007)) (list "unsupported" 0))`
   - env: `{ "eqm_subset": <requested subset>, "batch_len": <number of rationales> }`
   The 60-marker catalog is reference material (the paper's verbatim pattern set), not a 60-marker scorer; a `full_60` or `domain_tuned` request returns `"unsupported"` and stops before paid tool calls.
2. Gather the rationale corpus: array of `{rationale, forecast_probability, question, forecaster_id?, timestamp}` objects (the timestamp orders the corpus — the Dublin Core metadata row and the gaming rule's subsequent-resolved-cohort requirement both depend on it). Label `batch_len * $0.007` as the paper/tool's indicative estimate, not an observed provider charge. Grouping by `forecaster_id` needs no step of its own: the aggregate render groups on the field each record carries.

### eqm-score (P scorer, tool-owned — direct `market_score_rationale`; calibrated by eqm-validate against outcomes)

1. Call `market_score_rationale` (hkask-mcp-prediction-markets) directly per rationale (no LLM wrapper template). The calls are independent; issue them together.
2. Keep each EqmResult whole — composite_score, scores, red_flags, green_flags, interpretation, model, caveat, missing_eqms — and build one `per_rationale_scores` record from it: the corpus row's `rationale_index`, `question` and `forecaster_id`, plus the result's `composite_score`, `scores` (each `{id, score}`) and `missing_eqms`, copied unchanged.
3. The MCP tool is the single source of truth for the 12-EQM LLM scoring; do not reimplement it. A failed tool call propagates as a batch failure: stop, and do not render the aggregate. Zero-filled missing markers are not observed low-quality scores.

### eqm-aggregate (D — the `eqm/eqm-aggregate` render computes; no model call)

1. Render `eqm/eqm-aggregate` with `per_rationale_scores` once every requested rationale has a successful tool result. The render's output is the aggregate itself, a JSON document; read it, do not rewrite it.
2. Read `status` first:
   - `"unassessed"` — the corpus was empty; there is no mean or bias. Report it and stop.
   - `"incomplete"` — `incomplete_rationales` names each rationale and why: the tool's `missing_eqms`, score ids that are not exactly the predictive twelve (`absent_ids`, `unknown_ids`, `duplicate_ids`), a score outside 0–2, or a `composite_score` that is not Σhelps − Σhurts. No numeric aggregate is emitted. Retain the raw evidence, report the batch incomplete and stop.
   - `"complete"` — the remaining fields are the measurement.
3. `forecast_level_composite` holds each question's mean composite and flag totals; `forecaster_level_composite` holds each forecaster's mean composite — the paper's r=0.51 signal — and flag frequencies (rationales without a `forecaster_id` are in neither group).
4. `red_flag_screen` and `green_flag_endorsement` are the asymmetric decision rule (`EQM-2`): red-flag screen (high confidence — the paper's strong signal) is ≥2 Hurts at score 2; green-flag endorsement (weak) is composite ≥ 4 with 0 Hurts above 0. Never label a rationale "strong" from EQM scores alone.
5. `failing_marker_summary` lists each rationale's off-target markers in priority order with the catalog description of each — the operator's view of what a 2 (Helps) or a 0 (Hurts) looks like.
6. `overconfidence_bias` is `(extreme_confidence + forecast_rationale_misalign − statistical_reasoning − best_practices)` summed over rationales and divided by `2n`. Positive =
   overconfident, negative = underconfident. **Domain note:** this bias is
   marker-domain [-2, 2] and skill-authored (the paper defines no such
   measure); the hkask-forecast isotonic/log-odds recalibration path (`isotonic_fit`
   over (probability, outcome) pairs, `log_odds` for log-odds-space
   interpolation) operates in probability-domain. Until a validated
   conversion exists, the
   superforecasting stage-6 departure applies the DIRECTION (positive →
   widen, negative → sharpen) with the magnitude as a qualitative guide —
   never as a direct probability adjustment.

### eqm-validate (D — `lisp_eval` Brier and correlation; ground truth: realized outcomes)

1. With no realized outcomes, render `eqm/eqm-validate` with an empty `realized_outcomes`: the render returns the fixed `undetermined` result as JSON and no model call follows. `Undetermined` is not Ready-with-empty.
2. With realized outcomes: compute per-forecaster Brier as
   `(mean (brier ps os))` and the composite–accuracy correlation as
   `(outcome-r composites briers)` via `lisp_eval`, one composite and one Brier per forecaster with resolved outcomes. Pass them as `outcome_statistics`; template rendering never performs those calculations. A forecaster with resolved outcomes and no entry in `brier_by_forecaster` makes the render return the fixed `undetermined` result naming them, with no model call.
3. Do not infer gaming from the same forecast before/after a probability-preserving rationale rewrite: its Brier score is mathematically unchanged. Only comparable *subsequent* resolved forecasts with independent outcomes can support a gaming signal; with no such cohort, report `Undetermined` and do not assert `gaming_suspected: false` as proof of safety.
4. The paper's >90% directional-hypothesis match is a per-marker correlation over a cohort; this skill computes no per-marker correlation, so it reports no directional-match figure.

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
   for `confirmation_bias`, get genuine opposing hypotheses by rendering
   `falsifiability/falsifiability-hypothesize` with `admitted_target` (the
   question), `domain` and `context` (the rationale and its probability). With no real evidence, record
   the gap instead of writing a plausible sentence. When both are needed, the
   two lookups are independent; run them together.
2. **Probability preservation.** The rewrite supports the same
   `forecast_probability`; `forecast_rationale_align` checks it on re-score.
3. **Outcome validation.** `eqm-validate`'s `gaming_suspected` verdict halts the
   loop.

Steps:

1. **Direction, Current Condition, and Target (D)** — score the rationale directly with `market_score_rationale` (confirming `forecast_probability` to preserve and available evidence sources). Keep the returned `scores` unchanged: the two Improve templates take them as given and compute the failing markers, their targets (every failing Hurts marker → 0, every failing Helps marker → 2), the priority order (Hurts(2) > Hurts(1) > Helps(0) > Helps(1) — red-flag elimination before green-flag polish, the paper's asymmetric signal) and each marker's catalog description. Nothing is extracted, sorted or copied by hand.
2. **Predict** (P — critiqued by measured marker changes and source preservation) — render `eqm/eqm-imp-predict` with `scores` and `current_composite`; the render shows the one marker this iteration addresses and the model names the intervention: "intervention X raises marker Y from A to B". Record the confirmed target, criteria and actual source with `kanban_goal_create` if tracking it; omit routine `forecast`. Optional forecasts require the full prospective pact, not just confidence.
3. **Experiment** (P — critiqued by re-scoring and the three mitigations) — render `eqm/eqm-imp-experiment` with the same `scores` and the Step 2 `prediction`, and produce the rewrite
   under the three mitigations. Report its `fabrication_check`, `evidence_sourced` and `interventions_applied` with the rewrite: a `fabricated_evidence: true` or an unsourced claim discards the rewrite before any re-score.
4. **Check** (D) — re-score the *same predictive-12* set and call `lisp_eval` with the Convergence form below; judge the goal (`kanban_goal_judge`) with actual per-marker evidence. The worker finding does not close the goal; request native human closure separately and preserve independent assessment limitations.
5. **Act** — follow the form's `decision`: `continue` names the next experiment from `misses`; `stop_closed`, `stop_gaming`, `stop_bound` and `stop_invalid` end the loop, the last three with the remaining markers named. Missing outcomes leave external accuracy `Undetermined`, never a claim that the rationale is proven ungamed.

### Convergence

The local stop signal is per-marker, not a Cauchy criterion or a whole-rationale composite. One `lisp_eval` form takes the scores the loop started from, the fresh scores, both probabilities, the iteration number and the gaming verdict, and returns the decision:

```lisp
(begin (define hurts-ids (list "gut_based" "simplification_bias" "confirmation_bias" "extreme_confidence" "forecast_rationale_misalign" "speculative_terms")) (define count-id (lambda (id xs) (if (is_null xs) 0 (+ (if (string= (assoc "id" (car xs)) id) 1 0) (count-id id (cdr xs)))))) (define score-of (lambda (id xs) (if (is_null xs) -1 (if (string= (assoc "id" (car xs)) id) (assoc "score" (car xs)) (score-of id (cdr xs)))))) (define target-of (lambda (id) (if (member id hurts-ids) 0 2))) (define selected (lambda (xs) (if (is_null xs) (list) (let ((id (assoc "id" (car xs)))) (if (= (assoc "score" (car xs)) (target-of id)) (selected (cdr xs)) (cons id (selected (cdr xs)))))))) (define bad (lambda (ids fresh) (if (is_null ids) (list) (let ((s (score-of (car ids) fresh))) (if (and (= (count-id (car ids) fresh) 1) (>= s 0) (<= s 2)) (bad (cdr ids) fresh) (cons (car ids) (bad (cdr ids) fresh))))))) (define misses (lambda (ids fresh) (if (is_null ids) (list) (if (= (score-of (car ids) fresh) (target-of (car ids))) (misses (cdr ids) fresh) (cons (car ids) (misses (cdr ids) fresh)))))) (let ((sel (selected original_scores))) (let ((inv (bad sel fresh_scores)) (mis (misses sel fresh_scores)) (same (= original_probability revised_probability))) (list (list "decision" (cond (gaming_suspected "stop_gaming") ((is_null sel) "stop_invalid") ((not (is_null inv)) "stop_invalid") ((and (is_null mis) same) "stop_closed") ((>= iteration 8) "stop_bound") (t "continue"))) (list "misses" mis) (list "invalid" inv) (list "probability_preserved" same)))))
```

- env: `{ "original_scores": <scores[] from the Step 1 call>, "fresh_scores": <scores[] from this iteration's re-score>, "original_probability": <the forecast probability to preserve>, "revised_probability": <the probability submitted with the rewrite>, "iteration": <1 for the first re-score>, "gaming_suspected": <true only on a supported eqm-validate verdict, else false> }`

The selected markers are the ones off target in `original_scores`. A selected marker with no fresh score, a duplicated one, or a score outside 0–2 is `invalid` and stops the loop — it is not a pass. `stop_closed` needs every selected marker on target **and** the original probability unchanged; a changed probability keeps the loop open even with no misses. A positive helps marker cannot offset an off-target hurts marker.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `eqm-aggregate.j2` | Computed render (no model call): completeness check, forecast-level and forecaster-level composites, the asymmetric decision rule — red_flag_screen (high confidence) vs green_flag_endorsement (weak) — the failing-marker summary and `overconfidence_bias`, as JSON. |
| `eqm-validate.j2` | Interpret caller-computed Brier/correlation over comparable resolved outcomes; unchanged probability on the same forecast cannot diagnose gaming. With no outcomes the render returns the fixed `undetermined` result and no model call follows. |
| `eqm-imp-predict.j2` | Improve step 2: name the intervention for the one marker the render selects from the scorer's `scores`. |
| `eqm-imp-experiment.j2` | Improve step 3: rewrite the rationale for each failing marker the render lists from the scorer's `scores`, with real evidence, preserving the probability. |
| `predictive-markers.j2` | Shared fragment, never rendered directly: the predictive twelve's id, name, direction and description, and the failing-marker macro the templates above import. Generated from `eqm-catalog.yaml`. |
| `eqm-catalog.yaml` | Reference: the paper's full 60 EQM definitions from Karvetski et al. (2026, arXiv:2606.30987) Appendix A.4, verbatim, organized by the paper's six theory-informed families. The 12 markers selected for the live scorer are marked `predictive: true` — a hypothesis-faithful curation (the paper's Appendix C training experiment supports the helps-picks; the directional hypotheses are the paper's), NOT a paper-licensed top-12 ranking: the paper's LASSO tables (A.7/A.9) weight several of the chosen 12 near zero while excluding stronger patterns. The equal-weight composite (Σhelps − Σhurts) is likewise a skill-side simplification of the paper's LASSO-weighted composite — disclosed here, not presented under the paper's umbrella. Single source of truth for EQM definitions as published; the MCP tool's KEY_EQMS const carries the predictive 12's operational scoring descriptions. |

To render a template, call the `render_template` tool with the template ref (e.g., `eqm/eqm-aggregate`) and a context object with the required variables.

Copyable `render_template` context blocks (variable names from each template's `[inference] contract`):

`eqm/eqm-aggregate`:
```json
{
  "per_rationale_scores": [
    {
      "rationale_index": 0,
      "question": "<the question being forecast>",
      "forecaster_id": "<optional grouping key>",
      "composite_score": 0,
      "scores": [{ "id": "<eqm id>", "score": 0 }],
      "missing_eqms": []
    }
  ]
}
```

`eqm/eqm-validate`:
```json
{
  "forecaster_level_composite": [
    { "forecaster_id": "<id>", "mean_composite": 0, "red_flag_frequency": 0, "green_flag_frequency": 0, "rationale_count": 0 }
  ],
  "realized_outcomes": [
    { "question": "<the question>", "forecast_probability": 0.5, "outcome": true, "forecaster_id": "<id>" }
  ],
  "outcome_statistics": {
    "forecasters_resolved": 0,
    "brier_by_forecaster": { "<forecaster_id>": 0.0 },
    "composite_accuracy_r": "Undetermined"
  }
}
```

`eqm/eqm-imp-predict`:
```json
{
  "scores": [{ "id": "<eqm id>", "score": 0 }],
  "current_composite": 0
}
```

`eqm/eqm-imp-experiment`:
```json
{
  "rationale": "<the written reasoning>",
  "forecast_probability": 0.5,
  "question": "<the question being forecast>",
  "prediction": { "hypothesis": "<intervention X raises marker Y from A to B>", "intervention_type": "<type>", "delegation_target": "<source of real evidence, or null>" },
  "scores": [{ "id": "<eqm id>", "score": 0 }]
}
```

## Regression case

Run the deterministic helpers block through `lisp_eval`: `(mean (list 1 2 3))`
= 2.0, `(brier (list 0.7) (list 1))` = 0.09, and Pearson over a perfectly
linear pair = 1.0 — any drift in the helper definitions fails these. Run
`outcome-r` with four pairs (`"Undetermined"`), with five pairs whose
composites are all equal (`"Undetermined"`), and with composites
`(1 2 3 4 5)` against briers `(0.5 0.4 0.3 0.2 0.1)` (1.0). Run the
Convergence form five ways: every selected marker on target with equal
probabilities (`stop_closed`); the same with a changed probability at
iteration 1 (`continue`, `probability_preserved` false); one selected
marker still off target at iteration 8 (`stop_bound`, the marker in
`misses`); a selected marker absent from `fresh_scores` (`stop_invalid`,
the marker in `invalid`); `gaming_suspected` true (`stop_gaming`).
`eqm/eqm-aggregate`, `eqm/eqm-imp-predict`, `eqm/eqm-imp-experiment` and
the no-outcomes branch of `eqm/eqm-validate` are pinned by the
`eqm_` render tests in `crates/agent/src/tools/render_template_tool.rs`
(`cargo test -p agent --lib eqm_`). The `market_score_rationale` MCP
seam needs live server state and a real rationale corpus; it is a recorded
scope boundary, not run here.

## Constraints

- Maximum 10 bounded measurement requests; the Improve loop stops at 8 re-score/rewrite iterations or sooner on per-marker target attainment or a supported gaming signal.
- The Improve loop preserves the forecast probability and never fabricates evidence; each prediction names a specific intervention and marker.
- The convergence decision is deterministic (lisp_eval compute step) — no LLM convergence-check template.
- The MCP tool `market_score_rationale` is the single source of truth for 12-EQM scoring; this skill adds the measurement procedure around it.
- Never fabricate EQM scores — if the MCP tool call fails, propagate the error (do not default to 0).
- If the MCP tool returns `missing_eqms` (non-empty), the batch is incomplete — pass the field through unchanged; the aggregate render refuses to compute over it, and the operator should not read a low score as "low quality" when EQMs are missing.
