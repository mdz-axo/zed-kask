---
name: eqm
visibility: public
description: "Explanation Quality Markers measurement instrument. Scores forecast rationales against 60 EQMs via the market_score_rationale MCP tool, aggregates to composites, validates against realized outcomes (Brier), and emits an overconfidence_bias signal."
---

# EQM — Explanation Quality Markers (Measurement Instrument)

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
- When eqm-improvement needs a score profile to drive rationale improvement.

## When NOT to Use

- To *produce* a forecast — use `superforecasting`.
- To *improve* a rationale — use `eqm-improvement` (which calls this skill).
- To self-assess forecast quality — use superforecasting's `forecast-quality-gate`.

## Ontological Anchors

| Anchor | How it shapes the skill |
|---|---|
| Karvetski et al. (2026) — the EQM method | 60 markers, LLM-scored 0/1/2, composite, asymmetric signal, forecast-level + forecaster-level prediction. Defines the Score → Aggregate → Validate → Feedback shape. |
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

### eqm-select

1. Choose the EQM subset: `predictive_12` (default, matches the MCP tool's
   KEY_EQMS), `full_60` (research/validation), or `domain_tuned`.
2. Gather the rationale corpus: array of {rationale, forecast_probability,
   question, forecaster_id?} objects.
3. Prepare the scoring batch and cost estimate (~$0.007 per rationale).

### eqm-score (MCP tool step)

1. Call `market_score_rationale` (hkask-mcp-prediction-markets) per rationale.
2. Collect per-rationale EqmResult: composite_score, scores, red_flags,
   green_flags, interpretation, model, caveat, missing_eqms.
3. The MCP tool is the single source of truth for the 12-EQM LLM scoring;
   this skill does not re-implement it. If the tool returns `missing_eqms`
   (non-empty), flag the result as an incomplete assessment — the composite
   is pulled toward 0 for those dimensions.

### eqm-aggregate

1. Aggregate per-rationale scores to forecast-level composite (mean across
   rationales for the same question).
2. Aggregate to forecaster-level composite (mean across a forecaster's
   rationales — the paper's r=0.51 signal).
3. Apply the asymmetric decision rule: red_flag_screen (high confidence) vs
   green_flag_endorsement (weak).
4. Compute overconfidence_bias: signed signal (positive = overconfident, red
   flags dominate; negative = underconfident).

### eqm-validate

1. If realized_outcomes are present: correlate EQM composite with accuracy
   (Brier). Check directional-hypothesis match (paper's >90% finding).
2. If EQM scores rose but accuracy didn't improve → emit `gaming_suspected`
   verdict (halts eqm-improvement's loop).
3. If realized_outcomes absent → return `Undetermined` (not Ready-with-empty —
   per the advertised-invariants rule).

### Convergence

Cauchy criterion on the forecaster-level composite across iterations. The
convergence signal is the marker-space gap (distance from current composite to
target composite), computed deterministically via lisp_eval.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `eqm-select.j2` | Choose the EQM subset (predictive_12 default, full_60, or domain_tuned) and gather the rationale corpus. Prepares the scoring batch and cost estimate. |
| `eqm-score.j2` | Score rationales via the market_score_rationale MCP tool. Collect per-rationale EqmResult: composite_score, scores, red_flags, green_flags. The MCP tool is the single source of truth for 12-EQM LLM scoring. |
| `eqm-aggregate.j2` | Aggregate per-rationale scores to forecast-level and forecaster-level composites. Apply the asymmetric decision rule: red_flag_screen (high confidence) vs green_flag_endorsement (weak). Compute overconfidence_bias. |
| `eqm-validate.j2` | If realized_outcomes present: correlate EQM composite with accuracy (Brier), check directional-hypothesis match. If scores rose but accuracy didn't improve → gaming_suspected verdict. If outcomes absent → Undetermined (not Ready-with-empty). |
| `eqm-catalog.yaml` | Reference: the full 60 EQM definitions from Karvetski et al. (2026), organized by category (good_habits / warning_signs). The 12 most predictive are marked predictive: true. Single source of truth for EQM definitions; the MCP tool's KEY_EQMS const carries the predictive 12. |

## Constraints

- All flow templates have Public visibility.
- Maximum 10 iterations.
- The convergence decision is deterministic (lisp_eval compute step) — no LLM convergence-check template.
- The MCP tool `market_score_rationale` is the single source of truth for 12-EQM scoring; this skill adds the measurement procedure around it.
- Never fabricate EQM scores — if the MCP tool call fails, propagate the error (do not default to 0).
- If the MCP tool returns `missing_eqms` (non-empty), flag the result as an incomplete assessment — the composite is pulled toward 0 for those dimensions and the operator should not interpret a low score as "low quality" when EQMs are missing.
