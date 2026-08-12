---
name: eqm-improvement
visibility: public
description: "Improve a forecast rationale's quality by reverse-engineering the reasoning patterns the 60 EQMs specify. PDCA loop: score, target, rewrite, re-score, iterate to convergence. Preserves the forecast probability and grounds evidence in real sources."
---

# EQM Improvement — Reverse-Engineer Quality Forecasts (Improvement Instrument)

Improvement instrument for forecast rationales, grounded in Karvetski et al.
(2026) and structured as a Toyota Improvement Kata PDCA loop modeled on the
`metacognition` skill. The EQM definitions are the failing tests; the rationale
is the code; the rewrite is the green phase; re-scoring is the test run.

## When to Use

- When you need to improve a forecast rationale's quality as measured by EQM
  passage rate (the paper's validated instrument).
- When you want to reverse-engineer what good reasoning looks like from the
  EQM specifications (each EQM description defines what a score of 2 looks like).
- When you need to close the loop between EQM measurement and rationale
  improvement, with gaming detection.
- When you want the before/after delta to feed kata-improvement for
  forecaster-level learning across forecasts.

## When NOT to Use

- To *produce* a forecast from scratch — use `superforecasting`.
- To *measure* rationale quality — use `eqm` (this skill calls it).
- To self-assess without an external instrument — use `metacognition`.

## Ontological Anchors

| Anchor | How it shapes the skill |
|---|---|
| Karvetski et al. (2026) — the 60 EQM definitions | Each EQM description is a *specification* of what a score of 2 looks like. The improvement treats these as target specs, exactly as TDD treats test assertions as target behavior. |
| Toyota Improvement Kata (Rother 2010; Ren et al. 2026) | The PDCA shape: Direction → Current Condition → Target Condition → Experiment → Measure → Score Prediction. Modeled on the `metacognition` skill's Kata structure. |
| TDD red-green-refactor (the `tdd` skill's anchor) | Structural analog: red = EQM reveals gap, green = rewrite to address, refactor = tighten reasoning without changing the forecast probability. Borrows the shape; does not delegate to `tdd` (different domain). |
| PKO (Procedural Knowledge Ontology) | The rationale is a reasoning Procedure; improving it means improving the procedure's steps (cite base rate → consider alternatives → check disconfirming evidence → calibrate confidence). |

## Relationship to Metacognition

This skill is modeled on `metacognition`'s Kata structure, with three
substitutions:

1. **EQM marker-space gap** instead of Dublin Core + PKO gap. The gap is the
   distance from current EQM scores to target scores, computed via lisp.eval.
2. **External re-score** as the Brier outcome. Metacognition uses the hypotenuse
   (self-measured); EQM-improvement uses the `market_score_rationale` re-score
   (external, grounded in the paper).
3. **Specific intervention + marker predictions.** Metacognition predicts
   "calibration X will close the gap by Z"; EQM-improvement predicts
   "intervention X will raise marker Y from A to B" — more specific, more
   testable, and the specificity is part of the gaming mitigation.

## The Gaming-the-Scorer Problem (Critical Design Constraint)

When an LLM rewrites an artifact to score higher on an LLM-scored instrument,
the score can rise without genuine improvement — the rewriter learns to emit
marker-keywords the scorer rewards. Three mitigations, all required:

1. **Evidence grounding (Phase 4 delegates, never fabricates).** For
   `fact_based` / `statistical_reasoning`, the intervention is not "add a
   sentence that sounds statistical" — it's "find a real base rate." Phase 4
   delegates to `superforecasting/stage_2_outside_view`, `hkask-mcp-research`
   (web_search), or `falsifiability/falsifiability-hypothesize` for real
   evidence. If no real evidence can be found, the improvement notes the gap
   honestly rather than fabricating.
2. **Forecast-probability preservation (alignment invariant).** The improvement
   preserves the forecast probability — the rationale is improved, but the
   probability it supports stays the same. Enforced by the
   `forecast_rationale_align` EQM on re-score.
3. **Outcome validation (eqm skill's Phase 4 catches gaming).** When realized
   outcomes are available, the `eqm` skill's validation correlates EQM composite
   with accuracy. If scores rose but accuracy didn't improve → `gaming_suspected`
   verdict halts this loop.

## Instructions

### eqm-imp-direction (Kata Step 1: Direction)

1. Establish the improvement direction: "Improve rationale quality as measured
   by EQM passage rate, prioritizing red-flag elimination over green-flag
   polish" (the paper's asymmetric signal).
2. Confirm the forecast probability is preserved (the alignment invariant).
3. Identify available evidence sources for delegation (research,
   superforecasting, falsifiability).

### eqm-imp-current (Kata Step 2: Grasp Current Condition)

1. Call `market_score_rationale` (via the eqm skill or directly) to score the
   current rationale.
2. Receive failing_markers, red_flag_screen, green_flag_endorsement,
   composite_score.
3. Measure, don't assume — the EQM scores are the current condition.

### eqm-imp-target (Kata Step 3: Establish Target Condition)

1. Set marker-level targets derived from the EQM descriptions (what a 2 looks
   like).
2. Prioritize red flags (Hurts markers with score > 0) over green flags (Helps
   markers with score < 2) per the asymmetric signal.
3. The target is one step beyond the current condition — challenging but
   achievable.

### eqm-imp-predict (Kata Step 4: Make a Prediction)

1. Predict which intervention will close the gap and by how much.
2. **Specific prediction:** "Intervention X (e.g., 'cite the historical base
   rate for this reference class') will raise marker Y (e.g.,
   `statistical_reasoning`) from 0 to 2."
3. Carry a confidence in [0,1] — how sure is the agent that this prediction is
   correct? The Brier score tracks calibration.

### eqm-imp-experiment (Kata Step 5: Experiment / Do)

1. Apply the predicted intervention — rewrite the rationale to address each
   failing marker, guided by the EQM description (the spec for what a 2 looks
   like).
2. **CRITICAL CONSTRAINTS:**
   - Preserve `forecast_probability` — the rewrite must support the same
     probability (enforced by `forecast_rationale_align` EQM on re-score).
   - Never fabricate evidence — for `fact_based` / `statistical_reasoning`,
     delegate to `superforecasting/stage_2_outside_view` or
     `hkask-mcp-research` (web_search) for real data. If no real evidence
     found, note the gap honestly.
   - For `confirmation_bias`, delegate to `falsifiability/falsifiability-
     hypothesize` for genuine opposing hypotheses (not strawmen).
3. Produce the improved rationale.

### Convergence (Steps 6-9: Check + Act — deterministic compute, no LLM)

1. Re-score the improved rationale via `market_score_rationale`.
2. Compute marker-space gap (lisp.eval): distance from current scores to target
   scores.
3. Brier-score the prediction (lisp.eval): did the predicted marker reach the
   predicted level?
4. Check convergence: gap < epsilon, or Cauchy (iterates stabilized), or Brier
   calibrated.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `eqm-imp-direction.j2` | KnowAct | Kata Step 1: establish improvement direction. |
| `eqm-imp-current.j2` | KnowAct | Kata Step 2: grasp current EQM scores (calls market_score_rationale). |
| `eqm-imp-target.j2` | KnowAct | Kata Step 3: set marker-level targets from EQM descriptions. |
| `eqm-imp-predict.j2` | KnowAct | Kata Step 4: predict which intervention raises which marker, with confidence. |
| `eqm-imp-experiment.j2` | KnowAct | Kata Step 5: rewrite rationale per failing marker (delegates for evidence). |

## Constraints

- All flow templates are KnowAct type with Public visibility.
- Energy caps: direction (2048), current (4096), target (3072), predict (3072), experiment (6144).
- Gas cap: 80,000 per invocation. Maximum 8 iterations (improvement is costlier per iteration).
- The convergence decision is deterministic (lisp.eval compute steps) — no LLM convergence-check template.
- The forecast probability is preserved across all iterations (alignment invariant).
- Never fabricate evidence — delegate to research/superforecasting/falsifiability for real data.
- The prediction must name a specific intervention + specific marker (not "the rationale will improve").
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
