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

## Instructions

### Step 0 — Read prior calibration (execute)

1. Read prior calibration from the scenarios MCP forecast store via `scenario_calibration` — the Brier score history and overconfidence_bias from all resolved forecasts.
2. The overconfidence_bias feeds the grasp-current step so the agent knows its historical calibration. On failure, the Kata cycle proceeds without calibration context.

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

1. Predict which calibration will close the gap and by how much.
2. Carry a confidence in [0,1] — how sure is the agent that this prediction is correct?
3. The Brier score tracks whether the confidence is calibrated across cycles.

### meta-experiment (Kata Step 4: Experiment / Do)

1. Apply the predicted calibration — Falstaffian perspective rotation, ellipsis analysis, or strategy adjustment.
2. Re-measure the current condition after the experiment (the experiment changed the system).
3. Produce new current_artifacts and current_procedure for the gap computation.

### Convergence (Steps 5-9: Check + Act — model-evaluated)

1. Compute object-space gap (Dublin Core artifact completeness).
2. Compute process-space gap (PKO procedure progress).
3. Compute hypotenuse: sqrt(object_gap² + process_gap²).
4. Score the prediction via Brier score.
5. Check convergence: gap < epsilon, or stability check (iterates stabilized), or Brier calibrated.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `meta-grasp-current.j2` | KnowAct | Measure the agent's actual metacognitive state right now. Identify obstacles, surface assumptions, count grounded claims, enumerate options. Produces current_artifacts and current_procedure for gap computation. |
| `meta-establish-target.j2` | KnowAct | Declare the target metacognitive state — what sufficient meta-knowledge looks like for this goal. Produces target_artifacts and target_procedure for gap computation. |
| `meta-predict.j2` | KnowAct | Predict which calibration will close the gap and by how much. Carry a confidence in [0,1]. The Brier score tracks whether the confidence is calibrated. |
| `meta-experiment.j2` | KnowAct | Apply the predicted calibration — Falstaffian perspective rotation, ellipsis analysis, or strategy adjustment. Re-measure the current condition after the experiment. Produces new current_artifacts and current_procedure. |
| `falstaffian-perspective-engine.yaml` | RenderAct | Reference: three-fold structure (shapes, experience, spirit) with metacognitive application steps and shape selection decision tree. |
| `falstaffian-shapes.yaml` | RenderAct | Reference: seven semantic graph transformation operators — the Falstaffian shapes with input/output structures and tension components. |
| `falstaffian-variance-analysis.yaml` | RenderAct | Reference: three-pass variance calibration with agreement matrix and final taxonomy of core, secondary, and candidate shapes. |
| `ellipsis-analysis.j2` | KnowAct | Apply Bloom's five-step method to detect gaps in context, classify them as ellipsis (deliberate) or leak (unintentional), and surface what is not inferable. Used by the experiment step for ellipsis perspective. |

## Constraints

- All flow templates are KnowAct type with Public visibility. Reference documents are RenderAct.
- Evaluate convergence after each full iteration using the criteria described above.
- Execute the four Kata steps (grasp, target, predict, experiment), then evaluate the gap and Brier score to determine convergence.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
