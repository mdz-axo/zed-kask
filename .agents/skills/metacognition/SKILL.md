---
name: metacognition
description: "Master self-reflection skill following the Toyota Improvement Kata. Grasps the current metacognitive condition (meta-knowledge, confidence, obstacles), establishes a target condition, makes a prediction about which calibration will close the gap, runs the experiment (applies a Falstaffian perspective rotation, ellipsis analysis, or strategy adjustment), then measures the gap and scores the prediction via Brier. Convergence is detected deterministically: gap closure (limit of a sequence), iterate stabilization (Cauchy criterion), or prediction calibration (Brier score). Any userpod may invoke this skill."
---

# Metacognition

Master self-reflection skill following the Toyota Improvement Kata (Rother 2010).
The skill runs actual PDCA: grasp the current condition, establish a target
condition, make a prediction, run an experiment, measure the gap, and score
the prediction. Convergence is detected deterministically via the hypotenuse
of object-space (Dublin Core) and process-space (PKO) gaps, plus Brier-scored
prediction calibration.

## When to Use

- When an agent needs to reflect on its own metacognitive state and identify what it knows and doesn't know.
- When an agent needs to establish a measurable target condition for its meta-knowledge.
- When an agent needs to make a calibrated prediction about which intervention will improve its understanding.
- When an agent needs to run an experiment (apply a calibration) and measure whether it closed the gap.
- When the convergence decision should be deterministic (gap + Brier) rather than an LLM self-grade.

## Instructions

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

### Convergence (Steps 5-9: Check + Act — deterministic compute, no LLM)

1. Compute object-space gap (Dublin Core artifact completeness).
2. Compute process-space gap (PKO procedure progress).
3. Compute hypotenuse: sqrt(object_gap² + process_gap²).
4. Score the prediction via Brier score.
5. Check convergence: gap < epsilon, or Cauchy (iterates stabilized), or Brier calibrated.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `meta-grasp-current.j2` | KnowAct | Improvement Kata Step 1: measure the current metacognitive condition. |
| `meta-establish-target.j2` | KnowAct | Improvement Kata Step 2: declare the target condition. |
| `meta-predict.j2` | KnowAct | Improvement Kata Step 3: predict which calibration will close the gap, with confidence. |
| `meta-experiment.j2` | KnowAct | Improvement Kata Step 4: apply the calibration and re-measure. |
| `falstaffian-perspective-engine.yaml` | RenderAct | Reference: three-fold structure (shapes, experience, spirit) with shape selection decision tree. |
| `falstaffian-shapes.yaml` | RenderAct | Reference: seven semantic graph transformation operators. |
| `falstaffian-variance-analysis.yaml` | RenderAct | Reference: three-pass variance calibration with agreement matrix. |

## Constraints

- All flow templates are KnowAct type with Public visibility. Reference documents are RenderAct.
- Energy caps: grasp-current (6144), establish-target (4096), predict (4096), experiment (6144).
- Gas cap: 150,000 per invocation. Maximum 10 iterations (safety valve — the real stop conditions are gap, Cauchy, and Brier).
- The convergence decision is deterministic (compute steps) — no LLM convergence-check template.
- The LLM's job is the four Kata steps (grasp, target, predict, experiment); the executor computes the gap and Brier score.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
