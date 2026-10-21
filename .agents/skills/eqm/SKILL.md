---
name: eqm
description: "Explanation Quality Markers: score the predictive 12 with market_score_rationale, retain the 60-marker reference catalog (the paper's verbatim pattern set), aggregate complete results, validate against realized outcomes, and improve a rationale through a bounded evidence-grounded loop."
---

# EQM — Explanation Quality Markers (Measure and Improve)

Measurement instrument for forecast-rationale quality, grounded in Karvetski,
Huang, Kučinskas et al. (2026, arXiv:2606.30987), "Measuring Judgment Quality in Natural-Language
Explanations: Evidence from Forecasting Tournaments" — Forecasting Research
Institute. The paper defines the EQM instrument — 60 theory-guided marker patterns across six families; the reference catalog carries all 60 verbatim from Appendix A.4; the live `market_score_rationale` tool scores the predictive 12 using an LLM. Aggregate only complete tool results to forecast- and forecaster-level composites,
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
| Karvetski et al. (2026, arXiv:2606.30987) — the EQM method (`onto_anchor` → derived `explanation_quality_marker`, operator ruling 2026-09-25) | The marker instrument (LLM-scored 0/1/2), composite, asymmetric signal, forecast-level + forecaster-level prediction. Defines the Score → Aggregate → Validate → Feedback shape. |
| Tetlock Brier scoring (via superforecasting) | A loss measure on independently resolved forecasts, not ground truth or calibration by itself (Murphy 1973). |
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

## D/P labelling

Marker identification over a rationale, red-flag interpretation, and
Improve-phase rationale revision are P — judgment, critiqued by the
alignment check against the stated forecast probability and by gaming
detection on realized outcomes. The scoring call is P too, executed by a
tool-owned LLM (`market_score_rationale`) and calibrated by eqm-validate
against outcomes; subset admission and forecaster grouping (`EQM-1`), the
red-flag/green-flag screens (`EQM-2`), the ID-reconciliation check, and
every mean, Brier and correlation run in `lisp_eval` over the scorer's
returned values and the recorded outcomes (the deterministic helpers
below); the Improve-step priority order and target mapping are inline D
text. The instrument measures the written rationale; it does not vouch for
the forecast.

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

### eqm-select (D — `lisp_eval` `EQM-1` subset gate and batch preparation)

1. Admit only `predictive_12`, the MCP tool's fixed KEY_EQMS set, using `lisp_eval` (`EQM-1` — subset admission and batch cost gate; no LLM wrapper template):
   - form: `(if (and (string= eqm_subset "predictive_12") (> batch_len 0) (<= batch_len 10)) (list "predictive_12" (* batch_len 0.007)) (list "unsupported" 0))`
   - env: `{ "eqm_subset": <requested subset>, "batch_len": <number of rationales> }`
   The 60-marker catalog is reference material (the paper's verbatim pattern set), not a 60-marker scorer; a `full_60` or `domain_tuned` request returns `"unsupported"` and stops before paid tool calls.
2. Gather the rationale corpus: array of `{rationale, forecast_probability, question, forecaster_id?, timestamp}` objects (the timestamp orders the corpus — the Dublin Core metadata row and the gaming rule's subsequent-resolved-cohort requirement both depend on it). Label `batch_len * $0.007` as the paper/tool's indicative estimate, not an observed provider charge.
3. Group the corpus by `forecaster_id` (rationales without one stay ungrouped) into `forecaster_groups` — `{forecaster_id: [zero-based rationale indexes]}` — and verify the partition with `lisp_eval` (`EQM-1` — the grouping check the former `eqm-select.j2` performed; no LLM wrapper template):
   - form: `(begin (define uniq (lambda (xs seen) (if (is_null xs) seen (if (member (car xs) seen) (uniq (cdr xs) seen) (uniq (cdr xs) (cons (car xs) seen)))))) (define gkeys (lambda (gs) (if (is_null gs) (quote ()) (cons (car (car gs)) (gkeys (cdr gs)))))) (define sizes (lambda (gs) (if (is_null gs) (quote ()) (cons (length (car (cdr (car gs)))) (sizes (cdr gs)))))) (define cover (lambda (ks ids) (if (is_null ks) t (and (member (car ks) ids) (cover (cdr ks) ids))))) (let ((keys (uniq forecaster_ids (quote ())))) (and (cover (gkeys forecaster_groups) forecaster_ids) (cover keys (gkeys forecaster_groups)) (= (sum (sizes forecaster_groups)) (length forecaster_ids)))))`
   - env: `{ "forecaster_ids": <one forecaster_id per rationale that carries one>, "forecaster_groups": <the prepared {forecaster_id: [zero-based rationale indexes]}> }`
   Every group key is a corpus `forecaster_id`, every corpus `forecaster_id` has a group, and the groups' index counts sum to the rationales that carry one — a partition, not a sample.

### eqm-score (P scorer, tool-owned — direct `market_score_rationale`; calibrated by eqm-validate against outcomes)

1. Call `market_score_rationale` (hkask-mcp-prediction-markets) directly per rationale (no LLM wrapper template).
2. Collect per-rationale EqmResult: composite_score, scores, red_flags,
   green_flags, interpretation, model, caveat, missing_eqms.
3. The MCP tool is the single source of truth for the 12-EQM LLM scoring; do not reimplement it. A failed tool call propagates as a batch failure. For each result, load the 12 IDs marked `predictive: true` in `eqm-catalog.yaml` and compare them to the returned `scores[].id` using `lisp_eval`: `(begin (define all-present (lambda (xs ys) (if (is_null xs) t (and (member (car xs) ys) (all-present (cdr xs) ys))))) (and (= (length expected_ids) 12) (= (length expected_ids) (length observed_ids)) (all-present expected_ids observed_ids) (all-present observed_ids expected_ids)))`. A nonempty `missing_eqms`, an absent/duplicate ID, or any failed call makes the batch incomplete. Retain raw evidence but stop numeric aggregation and overconfidence-bias calculation; zero-filled missing markers are not observed low-quality scores.

### eqm-aggregate (D — `lisp_eval` over the tool's scores)

1. An empty corpus has no mean or bias: report `unassessed` and stop. Only after every requested rationale has a successful, complete predictive-12 result, render `eqm/eqm-aggregate` to group the scores; compute each forecast-level
   composite as `(mean <composites for the question>)` via `lisp_eval`.
2. Compute each forecaster-level composite as `(mean <that forecaster's
   composites>)` — the paper's r=0.51 signal.
3. Verify the asymmetric decision-rule screens with `lisp_eval` (`EQM-2` — red-flag screen and green-flag endorsement; the thresholds are deterministic, not LLM judgment):
   - form: `(begin (define hurts-at-2 (lambda (ms) (if (is_null ms) 0 (let ((m (car ms))) (if (and (string= (assoc "direction" m) "hurts") (= (assoc "score" m) 2)) (+ 1 (hurts-at-2 (cdr ms))) (hurts-at-2 (cdr ms))))))) (define hurts-above-0 (lambda (ms) (if (is_null ms) 0 (let ((m (car ms))) (if (and (string= (assoc "direction" m) "hurts") (> (assoc "score" m) 0)) (+ 1 (hurts-above-0 (cdr ms))) (hurts-above-0 (cdr ms))))))) (list (>= (hurts-at-2 scores) 2) (and (>= composite 4) (= (hurts-above-0 scores) 0))))`
   - env: `{ "scores": <that rationale's scores[] from market_score_rationale>, "composite": <its composite_score> }`
   Red-flag screen (high confidence — the paper's strong signal): ≥2 Hurts at score 2. Green-flag endorsement (weak): composite ≥ 4 and 0 Hurts above 0. The rendered `eqm/eqm-aggregate` output's `red_flag_screen` and `green_flag_endorsement` entries must agree with these gate verdicts.
4. Compute overconfidence_bias with `lisp_eval` from the four listed marker
   sums: `(/ (- (+ ec frm) (+ sr bp)) (* 2 n))`, env the sums of
   `extreme_confidence`, `forecast_rationale_misalign`,
   `statistical_reasoning`, `best_practices` and `n` rationales. Positive =
   overconfident, negative = underconfident. **Domain note:** this bias is
   marker-domain [-2, 2] and skill-authored (the paper defines no such
   measure); the consumer (`hkask_forecast::apply_calibration_adjustment`)
   expects probability-domain. Until a validated conversion exists, the
   superforecasting stage-6 departure applies the DIRECTION (positive →
   widen, negative → sharpen) with the magnitude as a qualitative guide —
   never as a direct probability adjustment.

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

1. **Direction, Current Condition, and Target (D)** — score the rationale directly with `market_score_rationale` (confirming `forecast_probability` to preserve and available evidence sources). Extract the failing markers (Hurts with score > 0, Helps with score < 2) and set marker-level targets deterministically as inline D text (replacing the former `eqm-imp-direction`, `eqm-imp-current`, and `eqm-imp-target` wrapper templates):
   - Constant target mapping: every failing Hurts marker → target 0 (eliminate the warning sign entirely); every failing Helps marker → target 2 (achieve the good habit clearly).
   - Fixed priority order: Hurts(2) > Hurts(1) > Helps(0) > Helps(1) — red-flag elimination before green-flag polish, the paper's asymmetric signal.
   - Target-set summary via `lisp_eval`: `(- (sum helps_targets) (sum hurts_targets))`, env `{ "helps_targets": <2 per failing Helps marker>, "hurts_targets": <0 per failing Hurts marker> }`.
   Join each failing marker's `id` to its verbatim `description` in `kask/registry/templates/eqm/eqm-catalog.yaml` (read the catalog, match by id — the single source of truth for what a score of 2 or 0 looks like) to form `prioritized_markers` and `failing_markers` before rendering `eqm-imp-predict` and `eqm-imp-experiment`. This subset summary is not the whole-rationale composite and cannot by itself close the loop.
2. **Predict** (P — critiqued by measured marker changes and source preservation) — render `eqm/eqm-imp-predict`: "intervention X raises marker Y from A to B". Record the confirmed target, criteria and actual source with `kanban_goal_create` if tracking it; omit routine `forecast`. Optional forecasts require the full prospective pact, not just confidence.
3. **Experiment** (P — critiqued by re-scoring and the three mitigations) — render `eqm/eqm-imp-experiment` and produce the rewrite
   under the three mitigations.
4. **Check** (D) — re-score the *same predictive-12* set, requiring complete coverage and the original probability; compare the submitted forecast probability to the original with `lisp_eval` `(= original_probability revised_probability)`. Join each selected `marker_target.id` to its fresh score; missing/duplicate IDs, an unknown direction, or a score outside 0–2 are not a pass. Call `lisp_eval` with the `marker_targets` records (`id`, `direction`, `current_score` from the new tool response, `target_score`) and the `misses` form below; judge the goal (`kanban_goal_judge`) with actual per-marker evidence. The worker finding does not close the goal; request native human closure separately and preserve independent assessment limitations.
5. **Act** — a nonempty failing-marker list names the next experiment; if empty and at least one target was checked, stop. Stop immediately on a *supported* `gaming_suspected` verdict, or after 8 iterations with remaining markers named. Missing outcomes leave external accuracy `Undetermined`, never a claim that the rationale is proven ungamed.

### Convergence

The local stop signal is per-marker, not a Cauchy criterion or the difference between a targeted-subset summary and a whole-rationale composite. After matching unique marker IDs and requiring a nonempty target list, call `lisp_eval`:

```lisp
(begin (define misses (lambda (items) (if (is_null items) (list) (let ((item (car items))) (cond ((string= (assoc "direction" item) "hurts") (if (<= (assoc "current_score" item) (assoc "target_score" item)) (misses (cdr items)) (cons (assoc "id" item) (misses (cdr items))))) ((string= (assoc "direction" item) "helps") (if (>= (assoc "current_score" item) (assoc "target_score" item)) (misses (cdr items)) (cons (assoc "id" item) (misses (cdr items))))) (t (cons (assoc "id" item) (misses (cdr items))))))))) (misses marker_targets))
```

An unknown `direction` value is NAMED in the result (the `t` clause) — it is
not a pass, per the Check step's rule; the pre-hardening form silently applied
the helps rule to it.

An empty result closes only when every selected marker has a fresh score and the original forecast probability is unchanged. A positive helps marker cannot offset an off-target hurts marker.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `eqm-aggregate.j2` | Group per-rationale scores to forecast-level and forecaster-level structures and apply the asymmetric decision rule: red_flag_screen (high confidence) vs green_flag_endorsement (weak). Means and overconfidence_bias are computed via `lisp_eval`. |
| `eqm-validate.j2` | Interpret caller-computed Brier/correlation over comparable resolved outcomes; unchanged probability on the same forecast cannot diagnose gaming. Missing cohorts remain Undetermined. |
| `eqm-imp-predict.j2` | Improve step 2: a specific intervention-to-marker prediction with confidence over the deterministically prioritized markers and catalog descriptions. |
| `eqm-imp-experiment.j2` | Improve step 3: rewrite the rationale for each failing marker with real evidence, preserving the probability. |
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
      "scores": [{ "id": "<eqm id>", "name": "<eqm name>", "direction": "helps", "score": 0 }],
      "red_flags": ["<hurts marker names with score > 0>"],
      "green_flags": ["<helps marker names with score > 0>"]
    }
  ],
  "forecaster_groups": { "<forecaster_id>": [0] }
}
```

`eqm/eqm-validate`:
```json
{
  "forecaster_level_composite": {
    "<forecaster_id>": { "mean_composite": 0, "red_flag_frequency": 0, "green_flag_frequency": 0, "rationale_count": 0 }
  },
  "realized_outcomes": [
    { "question": "<the question>", "forecast_probability": 0.5, "outcome": true, "forecaster_id": "<id>" }
  ],
  "per_rationale_scores": [
    { "rationale_index": 0, "question": "<the question>", "forecaster_id": "<id>", "composite_score": 0 }
  ],
  "outcome_statistics": { "<forecaster_id>": { "brier": 0.0, "correlation_r": 0.0 } }
}
```

`eqm/eqm-imp-predict`:
```json
{
  "target_condition": {
    "alignment_invariant": "forecast_probability preserved",
    "marker_targets": [
      { "id": "<eqm id>", "name": "<eqm name>", "direction": "helps", "current_score": 0, "target_score": 2, "gap": 2, "description": "<verbatim EQM description from eqm-catalog.yaml>" }
    ]
  },
  "prioritized_markers": [
    { "id": "<eqm id>", "name": "<eqm name>", "direction": "hurts", "current_score": 2, "target_score": 0, "gap": 2, "priority": 1, "description": "<verbatim EQM description from eqm-catalog.yaml>" }
  ],
  "current_composite": 0,
  "target_composite_score": 0
}
```

`eqm/eqm-imp-experiment`:
```json
{
  "rationale": "<the written reasoning>",
  "forecast_probability": 0.5,
  "question": "<the question being forecast>",
  "prediction": { "hypothesis": "<intervention X raises marker Y from A to B>", "predicted_marker": "<eqm id>", "predicted_target_score": 2 },
  "target_condition": {
    "alignment_invariant": "forecast_probability preserved",
    "marker_targets": [
      { "id": "<eqm id>", "direction": "helps", "current_score": 0, "target_score": 2, "description": "<verbatim EQM description from eqm-catalog.yaml>" }
    ]
  },
  "failing_markers": [
    { "id": "<eqm id>", "name": "<eqm name>", "direction": "helps", "current_score": 0, "target_score": 2, "gap": 2, "description": "<verbatim EQM description from eqm-catalog.yaml>" }
  ]
}
```

## Regression case

Run the deterministic helpers block through `lisp_eval`: `(mean (list 1 2 3))`
= 2.0, `(brier (list 0.7) (list 1))` = 0.09, and Pearson over a perfectly
linear pair = 1.0 — any drift in the helper definitions fails these. Run the
ID-reconciliation form with twelve identical id lists (true) and with an
empty pair (false — the length assertion discriminates). Run the
overconfidence_bias form with all-zero sums and n=1 (0.0). Run the
`misses` form with an empty `marker_targets` (closes — empty list) and with
one helps-target whose `current_score` is below its `target_score` (the
marker's `id` must be named in the result). Run the `EQM-1` grouping form
with a matching partition (`forecaster_ids` `["f1","f1","f2"]`, groups
`{"f1":[0,1],"f2":[2]}` — true) and with a group key absent from the
corpus (false). Run the `EQM-2` screens form with two Hurts markers at
score 2 (red-flag screen true) and with composite 4 plus one Hurts above
0 (green-flag endorsement false). The `market_score_rationale` MCP
seam needs live server state and a real rationale corpus; it is a recorded
scope boundary, not run here.

## Constraints

- Maximum 10 bounded measurement requests; the Improve loop stops at 8 re-score/rewrite iterations or sooner on per-marker target attainment or a supported gaming signal.
- The Improve loop preserves the forecast probability and never fabricates evidence; each prediction names a specific intervention and marker.
- The convergence decision is deterministic (lisp_eval compute step) — no LLM convergence-check template.
- The MCP tool `market_score_rationale` is the single source of truth for 12-EQM scoring; this skill adds the measurement procedure around it.
- Never fabricate EQM scores — if the MCP tool call fails, propagate the error (do not default to 0).
- If the MCP tool returns `missing_eqms` (non-empty), flag the result as an incomplete assessment — the composite is pulled toward 0 for those dimensions and the operator should not interpret a low score as "low quality" when EQMs are missing.
