---
name: kata-improvement
visibility: public
description: "4-step Improvement Kata templates for scientific capability development: Understand Direction, Grasp Current Condition, Establish Target Condition, Experiment (PDCA). Includes beginner_mode drills for foundational scientific thinking habit-building."
---

# Kata Improvement

4-step Improvement Kata templates for scientific capability development. Step 1: Understand Direction. Step 2: Grasp Current Condition. Step 3: Establish Target Condition. Step 4: Experiment (PDCA). Each step references prior outputs. The cycle closes with before/after measurement. Includes beginner_mode drills (folded from kata-starter): Five Questions, PDCA Cycle, and Observation Drill for foundational scientific thinking habit-building; agents graduate when automaticity > 0.5.


## When to Use

- When practicing the Toyota Improvement Kata for scientific capability development.
- When articulating the strategic direction and challenge from the level above.
- When grasping the current condition by gathering facts and data to establish a baseline.
- When establishing a measurable, time-bounded next target condition.
- When designing rapid PDCA experiments with testable predictions toward the target.
- When computing a normalized convergence metric to evaluate the coherence of a PDCA cycle.
- When an agent needs to build foundational scientific thinking habits through beginner_mode drills (folded from kata-starter): Five Questions, PDCA Cycle on a trivial process, or Observation Drill separating facts (IS) from interpretations (OUGHT).

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

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `beginner-selector.j2` | KnowAct | Select appropriate starter drill based on practice history and automaticity. If no history, start with Observation Drill. If automaticity is low in a specific drill, target that drill. If 7+ days since last practice, restart with Observation Drill. |
| `beginner-five-questions.j2` | KnowAct | Five Questions Drill — exercise asking the 5 coaching questions in order on a trivial process (making toast, brewing coffee). |
| `beginner-pdca-cycle.j2` | KnowAct | PDCA Cycle Drill — practice Plan-Do-Check-Act on a trivial, measurable process. |
| `beginner-observation-drill.j2` | KnowAct | Observation Drill — practice separating observed facts (IS) from interpretations (OUGHT). |
| `improvement-step1-direction.j2` | KnowAct | Step 1 of the Improvement Kata — understand the strategic direction and challenge from the level above. |
| `improvement-step2-current.j2` | KnowAct | Step 2 of the Improvement Kata — grasp the current condition by gathering facts and data to establish a baseline. |
| `improvement-step3-target.j2` | KnowAct | Step 3 of the Improvement Kata — establish a measurable, time-bounded next target condition. |
| `improvement-step4-experiment.j2` | KnowAct | Step 4 of the Improvement Kata — define next experiment with testable predictions toward the target. |

## Constraints

- rJoule cap: 3 per invocation. Maximum 10 iterations.
- `improvement-step1-direction.j2`: Public.
- `improvement-step2-current.j2`: Public.
- `improvement-step3-target.j2`: Public.
- `improvement-step4-experiment.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
