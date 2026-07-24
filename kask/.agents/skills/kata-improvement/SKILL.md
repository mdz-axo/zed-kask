---
name: kata-improvement
visibility: public
description: "4-step Improvement Kata templates for scientific capability development. Step 1: Understand Direction. Step 2: Grasp Current Condition. Step 3: Establish Target Condition. Step 4: Experiment (PDCA). Each step references prior outputs. The cycle closes with before/after measurement.
"
---

# Kata Improvement

4-step Improvement Kata templates for scientific capability development. Step 1: Understand Direction. Step 2: Grasp Current Condition. Step 3: Establish Target Condition. Step 4: Experiment (PDCA). Each step references prior outputs. The cycle closes with before/after measurement.


## When to Use

- When practicing the Toyota Improvement Kata for scientific capability development.
- When articulating the strategic direction and challenge from the level above.
- When grasping the current condition by gathering facts and data to establish a baseline.
- When establishing a measurable, time-bounded next target condition.
- When designing rapid PDCA experiments with testable predictions toward the target.
- When computing a normalized convergence metric to evaluate the coherence of a PDCA cycle.

## Instructions

### improvement-step1-direction

1. Articulate the direction before measuring progress toward it.
2. Answer what the challenge is with specific, measurable statements.
3. Describe what excellent performance looks like in measurable terms.
4. Define how you will know you've improved by stating the metric and measurement plan.
5. Mark the boundary of your current knowledge threshold explicitly.
6. Respond with a JSON object containing `challenge`, `excellent_performance`, `measurement_plan`, and `knowledge_threshold`.

### improvement-step2-current

1. Go and see to gather the facts; do not assume—measure.
2. Collect real data to describe the actual performance now.
3. List every metric describing the current state with method and source.
4. Observe what patterns exist in the data.
5. Redraw the boundary between known and assumed for your knowledge threshold.
6. Record the baseline measurements you commit to measuring against as `metric_before`.
7. Respond with a JSON object containing `current_performance`, `metrics`, `patterns`, `knowledge_threshold`, and `metric_before`.

### improvement-step3-target

1. Declare a specific, measurable target condition 1 week to 3 months out, beyond your current knowledge threshold.
2. Identify every obstacle between current and target conditions to create an Obstacles Parking Lot.
3. Select the ONE most consequential obstacle to address first.
4. Define what you do NOT know about the focus obstacle.
5. Respond with a JSON object containing `target_condition`, `obstacles`, `focus_obstacle`, `knowledge_gap`, and `metrics_target`.

### improvement-step4-experiment

1. Design a PDCA experiment against ONE obstacle.
2. Plan your next step: make it specific, actionable, and one change at a time.
3. Plan your expectation: state your prediction and why (the theory you're testing).
4. Do: define how you will execute (tool, parameter, configuration).
5. Check: define how you will measure and what confirms or refutes your prediction.
6. Act: decide what you will do with the result (next obstacle if correct, revised theory if wrong).
7. Determine how quickly you can go and see the result.
8. Respond with a JSON object containing `obstacle`, `next_experiment`, `prediction`, `measurement_method`, `success_criterion`, `learning_commitment`, and `when_to_check`.

### kata-improvement-convergence-check

1. Measure whether the four-step improvement kata has produced a coherent, testable PDCA plan.
2. Check each step's quality (direction, current condition, target condition, experiment).
3. Check cross-step coherence (current↔target gap, experiment alignment, feedback timing).
4. Check Regulation grounding if Regulation counters are available in context.
5. Start at 1.0, subtract for each satisfied check, and clamp to [0, 1].
6. Return JSON only with `convergence_metric`, `convergence_method`, `metric_decomposition`, `rationale`, and `blockers`.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `improvement-step1-direction.j2` | WordAct | Step 1 — Understand the direction. Articulate challenge, target, measurement plan, knowledge threshold. |
| `improvement-step2-current.j2` | WordAct | Step 2 — Grasp the current condition. Collect data, detect patterns, establish baseline metrics. |
| `improvement-step3-target.j2` | WordAct | Step 3 — Establish next target condition. Declare measurable target, detect obstacles, pick ONE focus obstacle. |
| `improvement-step4-experiment.j2` | WordAct | Step 4 — Experiment toward target. Design PDCA experiment with specific prediction and measurement plan. |
| `kata-improvement-convergence-check.j2` | KnowAct | Compute normalized convergence metric for kata-improvement PDCA cycles. Returns convergence_metric plus rationale and blockers.  |

## Constraints

- `improvement-step1-direction.j2`: Public.
- `improvement-step2-current.j2`: Public.
- `improvement-step3-target.j2`: Public.
- `improvement-step4-experiment.j2`: Public.
- `kata-improvement-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
