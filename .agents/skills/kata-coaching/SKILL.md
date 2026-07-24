---
name: kata-coaching
visibility: public
description: "5-question Coaching Kata templates for teaching scientific thinking. Grounded in the learner's actual Improvement Kata storyboard data. Q1: Target Condition. Q2: Actual Condition. Q3: Obstacles. Q4: Next Step/Experiment. Q5: Feedback loop closure.
"
---

# Kata Coaching

5-question Coaching Kata templates for teaching scientific thinking. Grounded in the learner's actual Improvement Kata storyboard data. Q1: Target Condition. Q2: Actual Condition. Q3: Obstacles. Q4: Next Step/Experiment. Q5: Feedback loop closure.


## When to Use

- When a learner needs to articulate a specific, measurable target condition and establish a clear goal.
- When a learner must ground their understanding of the current state in actual data rather than assumptions.
- When a learner needs to identify obstacles preventing progress and prioritize a single obstacle to address.
- When a learner must design a rapid PDCA experiment by defining a specific next step and a testable prediction.
- When a learner needs to close the feedback loop by committing to a specific time and metric to check results.
- When evaluating the overall convergence and effectiveness of a completed kata-coaching PDCA cycle.

## Instructions

### coaching-q1-target

1. Ask the learner what their target condition is.
2. Respond as the learner with specific data from the IK storyboard, including the specific measurable target, timeline, and success criteria.
3. If the target is vague, ask the learner to make it more specific, measurable, and verifiable.

### coaching-q2-actual

1. Ask the learner what the actual condition is right now.
2. Probe whether the learner's statements are based on measured data or assumptions.
3. Challenge interpretations stated as facts by asking what was actually observed.

### coaching-q3-obstacles

1. Ask the learner what obstacles prevent reaching the target and which single obstacle they are addressing now.
2. Prompt the learner to explain why the chosen obstacle is the priority and what solving it enables.
3. If the learner lists multiple obstacles without prioritizing, force them to select the one that moves them furthest toward the target.

### coaching-q4-experiment

1. Ask the learner what their next step is and what they expect to happen.
2. Prompt the learner to specify exactly what they will do and what they predict will occur.
3. If the learner proposes an action without a prediction, require them to state a testable prediction so expectation can be compared to result.

### coaching-q5-learn

1. Ask the learner how quickly they can go and see what they learned.
2. Prompt the learner to specify exactly when they will check the result, what metric they will measure, and what would prove the theory wrong.
3. If the learner is vague about timing, require them to pick a specific time to compare prediction to result.

### kata-coaching-convergence-check

1. Evaluate the coaching cycle inputs for clear target/current gap framing, prioritized obstacle, concrete next experiment, and feedback timing.
2. Measure convergence on a scale of 0 to 1, where 0 indicates fully converged and 1 indicates not converged.
3. Score how much work remains to reach convergence.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `coaching-q1-target.j2` | WordAct | Q1 — What is the Target Condition? Ground learner in their goal. |
| `coaching-q2-actual.j2` | WordAct | Q2 — What is the Actual Condition now? Ground learner in reality with data. |
| `coaching-q3-obstacles.j2` | WordAct | Q3 — What obstacles? Which ONE now? Force prioritization. |
| `coaching-q4-experiment.j2` | WordAct | Q4 — What is your Next Step? What do you expect? Drive action with prediction. |
| `coaching-q5-learn.j2` | WordAct | Q5 — How quickly can we go and see? Close the feedback loop. |
| `kata-coaching-convergence-check.j2` | KnowAct | Compute normalized convergence metric for kata-coaching PDCA cycles. Returns convergence_metric plus rationale and blockers.  |

## Constraints

- `coaching-q1-target.j2`: Public.
- `coaching-q2-actual.j2`: Public.
- `coaching-q3-obstacles.j2`: Public.
- `coaching-q4-experiment.j2`: Public.
- `coaching-q5-learn.j2`: Public.
- `kata-coaching-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
