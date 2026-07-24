---
name: kata-starter
visibility: public
description: "Starter Kata practice templates for building foundational scientific thinking habits. Training wheels — agents graduate when automaticity > 0.5. Three drills: Five Questions, PDCA Cycle, Observation Drill.
"
---

# Kata Starter

Starter Kata practice templates for building foundational scientific thinking habits. Training wheels — agents graduate when automaticity > 0.5. Three drills: Five Questions, PDCA Cycle, Observation Drill.


## When to Use

- When an agent needs to build foundational scientific thinking habits through starter kata drills.
- When selecting the appropriate starter drill based on practice history and automaticity scores.
- When practicing the Five Questions coaching pattern on a trivial process.
- When running a Plan-Do-Check-Act (PDCA) experiment on a trivial, measurable process.
- When training to separate observed facts (IS) from interpretations (OUGHT).
- When evaluating the convergence of kata-starter PDCA cycles to determine habit stability.

## Instructions

### starter-selector

1. Select the appropriate starter drill based on the learner's current automaticity and practice history.
2. If there is no practice history, start with the Observation Drill.
3. If automaticity in a specific drill is low (< 0.3), target that drill.
4. If automaticity is balanced across drills, rotate through all three.
5. If 7+ days have passed since last practice, restart with the Observation Drill.

### starter-five-questions

1. Pick a trivial process (e.g., making toast, brewing coffee) so the content does not distract from the question pattern.
2. Ask yourself the 5 coaching questions in order, answering as the learner.
3. Define the Target Condition.
4. Define the Actual Condition now.
5. Identify Obstacles and choose ONE to focus on.
6. Determine your Next Step and what you expect to happen.
7. Determine how quickly you can go and see the result.

### starter-pdca-cycle

1. Pick a trivial, measurable process where you can compare numbers.
2. Plan: State what you will do and predict the measurable outcome.
3. Do: Execute the plan and record exactly what happened with measurements.
4. Check: Compare the result to the prediction and identify the delta.
5. Act: Determine what you will change next time based on this learning.

### starter-observation-drill

1. Pick a recent interaction, log entry, or system event.
2. Record Observations (FACTS only): What you literally saw, including numbers, messages, and timestamps, with no adjectives or judgments.
3. Record Interpretations (CONCLUSIONS): What you think these mean, explicitly labeled as interpretations.
4. Perform a discrimination check: Scan observations for hidden interpretations and move them to the interpretations list.
5. Define a knowledge threshold: For each interpretation, identify what additional measurement would confirm or refute it.

### kata-starter-convergence-check

1. Measure convergence on a scale of [0,1] where 0 indicates stable foundational habit signals and low ambiguity.
2. Score how much work remains based on the starter drill outcomes.
3. Return the convergence metric, method, rationale, and any blockers.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `starter-selector.j2` | KnowAct | Select appropriate starter drill based on history and automaticity. |
| `starter-five-questions.j2` | KnowAct | Five Questions Drill — exercise asking the 5 coaching questions in order. |
| `starter-pdca-cycle.j2` | KnowAct | PDCA Cycle Drill — practice Plan-Do-Check-Act on a trivial process. |
| `starter-observation-drill.j2` | KnowAct | Observation Drill — practice separating facts (IS) from interpretations (OUGHT). |
| `kata-starter-convergence-check.j2` | KnowAct | Compute normalized convergence metric for kata-starter PDCA cycles. Returns convergence_metric plus rationale and blockers.  |

## Constraints

- `starter-selector.j2`: Public.
- `starter-five-questions.j2`: Public.
- `starter-pdca-cycle.j2`: Public.
- `starter-observation-drill.j2`: Public.
- `kata-starter-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
