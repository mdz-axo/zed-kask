---
name: metacognition
core: true
description: "Master self-reflection skill following the Toyota Improvement Kata. Grasps the current metacognitive condition, establishes a target, predicts which calibration closes the gap, runs the experiment, then measures the gap and scores it via Brier."
---

# Metacognition

Master self-reflection skill following the Toyota Improvement Kata (Rother 2010).
The skill runs actual PDCA: grasp the current condition, establish a target
condition, make a prediction, run an experiment, measure the gap, and score
the prediction. Evaluate convergence via the hypotenuse
of object-space (Dublin Core) and process-space (PKO) gaps, plus Brier-scored
prediction calibration.

## When to Use

- When an agent needs to reflect on its own metacognitive state and identify what it knows and doesn't know.
- When an agent needs to establish a measurable target condition for its meta-knowledge.
- When an agent needs to make a calibrated prediction about which intervention will improve its understanding.
- When an agent needs to run an experiment (apply a calibration) and measure whether it closed the gap.

## When NOT to Use

- Coaching a human or agent through the kata — use `kata-coaching`; this skill is the practitioner's own reflection loop.
- Executing a specific improvement — `kata-improvement` / `self-improvement` own the act; this skill measures the gap and scores the prediction.
- Forecast-calibration tracking in the prediction-market domain — `calibration-stewardship` and `superforecasting`'s stage 6 own that loop.

## Instructions

### Step 0 — Read prior calibration (execute)

1. Read prior calibration from the scenarios MCP forecast store via `scenario_calibration` — the Brier score history and overconfidence_bias from all resolved forecasts.
2. The overconfidence_bias feeds the grasp-current step so the agent knows its historical calibration. "No stored forecasts" is an expected empty calibration state: record calibration context as unavailable and continue without reporting a skill-use failure. Other failures remain visible, and the Kata cycle proceeds without calibration context.

### meta-grasp-current (Kata Step 1: Grasp Current Condition)

1. Measure the agent's actual metacognitive state — don't assume, measure.
2. Identify obstacles (typed, severity-rated), surface assumptions, count grounded claims.
3. Produce current_artifacts (Dublin Core) and current_procedure (PKO) for gap computation.
4. On refinement cycles, compare to the previous grasp and note what changed.

### meta-establish-target (Kata Step 2: Establish Target Condition)

1. Declare the target metacognitive state — what "sufficient meta-knowledge" looks like.
2. Produce target_artifacts and target_procedure that the gap computation measures toward.
3. The target should be one step beyond the current knowledge threshold — challenging but achievable.

### meta-predict (Kata Step 3: Make a Prediction)

1. Predict one calibration and an `expected_gap_reduction` in (0,1] relative to the measured starting gap. Keep the target artifacts and procedure fixed through this experiment; changing the target makes before/after gaps incomparable.
2. `confidence` in [0,1] is the probability of this predeclared binary event: after the experiment, `gap_after <= gap_before * (1 - expected_gap_reduction)`. It is not a confidence in an unspecified improvement or the fractional reduction itself.
3. If `gap_before` is zero or unavailable, do not make or score a gap-reduction forecast. Record the target as reached or the measurement as pending, respectively.

### meta-experiment (Kata Step 4: Experiment / Do)

1. Apply the predicted calibration — Falstaffian perspective rotation, ellipsis analysis, or strategy adjustment.
2. Re-measure the current condition after the experiment (the experiment changed the system).
3. Produce new current_artifacts and current_procedure for the gap computation.

### Convergence (Steps 5-9: Check + Act — model-evaluated)

1. Compute object-space gap (Dublin Core artifact completeness).
2. Compute process-space gap (PKO procedure progress).
3. Compute hypotenuse: sqrt(object_gap² + process_gap²).
4. With a measured `gap_before > 0` and `gap_after` against the same target, determine whether the Step 3 event occurred, then score `Brier = (confidence - outcome)^2` with outcome 1 for true and 0 for false. Use `lisp_eval` for the comparison and arithmetic; never select the event or threshold after observing the result. If either gap is unmeasured, report calibration pending, not zero error.
5. Check convergence against a declared epsilon and measured before/after gaps. Stability requires two measured iterations, and Brier calibration requires resolved predictions; missing measurements cannot satisfy a convergence branch. If no branch passes, re-enter grasp-current with the experiment's observed result; stop after three cycles and report the remaining gap and pending measurements.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `meta-grasp-current.j2` | Measure the agent's actual metacognitive state right now. Identify obstacles, surface assumptions, count grounded claims, enumerate options. Produces current_artifacts and current_procedure for gap computation. |
| `meta-establish-target.j2` | Declare the target metacognitive state — what sufficient meta-knowledge looks like for this goal. Produces target_artifacts and target_procedure for gap computation. |
| `meta-predict.j2` | Predict which calibration will close the gap and by how much. Carry a confidence in [0,1]. The Brier score tracks whether the confidence is calibrated. |
| `meta-experiment.j2` | Apply the predicted calibration — Falstaffian perspective rotation, ellipsis analysis, or strategy adjustment. Re-measure the current condition after the experiment. Produces new current_artifacts and current_procedure. |
| `ellipsis-analysis.j2` | Apply Bloom's five-step method to detect gaps in context, classify them as ellipsis (deliberate) or leak (unintentional), and surface what is not inferable. Used by the experiment step for ellipsis perspective. |

To render a template, call `render_template` with the template ref and these **top-level** context keys (not nested under `variables`). Pass the prior step's result as the named condition, not as `current_grasp` or `predicted_calibration`:

| Template ref | Required context keys | Optional context keys |
|---|---|---|
| `metacognition/meta-grasp-current` | `goal` | `session_history`, `current_state`, `prior_outcomes`, `prev_grasp`, `prior_calibration` |
| `metacognition/meta-establish-target` | `goal`, `current_condition` (Step 1 result) | — |
| `metacognition/meta-predict` | `goal`, `current_condition` (Step 1 result), `target_condition` (Step 2 result) | `prev_prediction` |
| `metacognition/meta-experiment` | `goal`, `current_condition` (Step 1 result), `prediction` (Step 3 result) | `perspectives`, `context_text`, `prev_experiment` |
| `metacognition/ellipsis-analysis` | `text`, `expectations`, `biases`, `assumptions`, `domain` | — |

A `render_template` call renders the prompt; it does not execute the inference step or produce that step's result. Use the measured result of each step when supplying the next step's condition.

## Constraints

- All templates are prompt templates with Public visibility.
- Evaluate convergence after each full iteration using the criteria described above.
- Execute the four Kata steps (grasp, target, predict, experiment), then evaluate the gap and Brier score to determine convergence.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
