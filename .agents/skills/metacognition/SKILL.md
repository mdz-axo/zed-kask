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

## Reference models

- Improvement Kata — Rother, *Toyota Kata* (2010); `onto_anchor` → derived `improvement_kata`.
- PDCA — Shewhart (1939), Deming; `onto_anchor` → derived `pdca_cycle`.
- Brier score — Brier (1950); `onto_anchor` → derived `brier_score`.
- "Metacognition" itself anchors only at the 5W1H core (coarse); the skill's fidelity claim is to the Kata and Brier models above, not to a metacognition literature.

## When to Use

- When an agent needs to reflect on its own metacognitive state and identify what it knows and doesn't know.
- When an agent needs to establish a measurable target condition for its meta-knowledge.
- When an agent needs to make a calibrated prediction about which intervention will improve its understanding.
- When an agent needs to run an experiment (apply a calibration) and measure whether it closed the gap.

## When NOT to Use

- Coaching a human or agent through the kata — use `kata-coaching`; this skill is the practitioner's own reflection loop.
- Executing a specific improvement — `kata-improvement` owns the act; this skill measures the gap and scores the prediction.
- Forecast-calibration tracking in the prediction-market domain — `calibration-stewardship` and `superforecasting`'s stage 6 own that loop.

## Instructions

### Step 0 — Read prior calibration (execute)

1. Read this skill's own prior calibration with `kanban_goal_list`: resolved goals whose `goal_text` begins `metacognition:` carry the intake prediction and the operator's scored outcome (Brier-scored by `kanban_goal_score`). Forecast-market calibration (`scenario_calibration`) is a different reference class and is not this skill's calibration.
2. The resolved predictions feed the grasp-current step. "No resolved metacognition goals" is an expected empty state: record calibration as unavailable and continue. Other failures remain visible, and the Kata cycle proceeds without calibration context.

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
3. Record the prediction before the experiment: `kanban_goal_create` with `goal_text` `metacognition: <the predicted outcome>`, one observable criterion (the measured gap reduction), and the confidence as `prediction`. The operator scores it later (`kanban_goal_score`), which Brier-scores the confidence; this session never scores its own prediction.

### meta-experiment (Kata Step 4: Experiment / Do)

1. Apply the predicted calibration — Falstaffian perspective rotation, ellipsis analysis, strategy adjustment, or an **inquiry experiment** for a problem that needs branching, revision and deep-dive delegation (below).
2. Re-measure the current condition after the experiment (the experiment changed the system).
3. Produce new current_artifacts and current_procedure for the gap computation. Carry the Step 3 `gap_before` and reduction threshold unchanged; never recompute the starting gap from post-experiment data.

### Inquiry experiment (branching chain of thought with delegation)

Use when the gap is in understanding a problem that needs several dependent reasoning steps, not in calibrating a known one. It runs inside Step 4; the Kata steps around it are unchanged.

1. Render `metacognition/inquiry-engine` with `problem`, `domain`, `constraints`, `max_thoughts`, `thinking_budget` and the prior cycle's `prior_chain`, `prior_delegation_results` and `prior_skill_match_results`. It generates, branches, revises and verifies thoughts and emits `delegation_requests` and `skill_match_queries`.
2. For each `delegation_requests` entry, render the matching delegation template with it: `metacognition/inquiry-delegate-hypothesis-framer` (question framing, FINER + PICO), `metacognition/inquiry-delegate-mcda` (choice among alternatives), `metacognition/inquiry-delegate-diagnose` (a bug or regression), or `metacognition/inquiry-delegate-falsifiability` (a counterfactual or an untestable claim). For `skill_match_queries`, render `skill-discovery/skill-discovery-route` for up to three follow-up skills.
3. Feed the delegation and skill-match results back into the next engine render. The engine's `final_answer` and `hypothesis_verified` become the post-experiment current condition.

### Convergence (Steps 5-9: Check + Act)

The counts are probabilistic (P — model-produced, critiqued by the operator when the recorded goal is scored); the arithmetic over them is deterministic (D — `lisp_eval`).

1. (P) Count the object-space gap inputs (Dublin Core artifacts missing vs target) and process-space inputs (PKO procedure steps incomplete vs target) from the grasp and target results; normalize each to [0,1].
2. (D) Compute the hypotenuse with `lisp_eval`, form `(sqrt (+ (* og og) (* pg pg)))`, env `{ "og": <object gap>, "pg": <process gap> }`. Never estimate the gap in prose.
3. (D) The reduction is `(- gap_before gap_after)` via `lisp_eval`; `gap_before` is the Step 3 value, unchanged.
4. Judge the recorded goal with the measured gap (`kanban_goal_judge`). The Brier score arrives only when the operator scores the goal; until then calibration is pending, not zero error.
5. Check convergence against a declared epsilon and measured before/after gaps. Stability requires two measured iterations, and Brier calibration requires operator-resolved predictions; missing measurements cannot satisfy a convergence branch. If no branch passes, re-enter grasp-current with the experiment's observed result; stop after three cycles in this session and report the remaining gap and pending measurements.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `meta-grasp-current.j2` | Measure the agent's actual metacognitive state right now. Identify obstacles, surface assumptions, count grounded claims, enumerate options. Produces current_artifacts and current_procedure for gap computation. |
| `meta-establish-target.j2` | Declare the target metacognitive state — what sufficient meta-knowledge looks like for this goal. Produces target_artifacts and target_procedure for gap computation. |
| `meta-predict.j2` | Predict which calibration will close the gap and by how much. Carry a confidence in [0,1]. The Brier score tracks whether the confidence is calibrated. |
| `meta-experiment.j2` | Apply the predicted calibration — Falstaffian perspective rotation, ellipsis analysis, or strategy adjustment. Re-measure the current condition after the experiment. Produces new current_artifacts and current_procedure. |
| `inquiry-engine.j2` | Inquiry experiment: branching, revising chain of thought that emits deep-dive delegation requests and skill-match queries. |
| `inquiry-delegate-hypothesis-framer.j2` | Delegation: frame a research question and testable hypothesis (FINER + PICO). |
| `inquiry-delegate-mcda.j2` | Delegation: weigh and rank alternatives with sensitivity. |
| `inquiry-delegate-diagnose.j2` | Delegation: structured root-cause diagnosis of a bug or regression. |
| `inquiry-delegate-falsifiability.j2` | Delegation: eliminative inference for a counterfactual or an untestable claim. |
| `ellipsis-analysis.j2` | Apply Bloom's five-step method to detect gaps in context, classify them as ellipsis (deliberate) or leak (unintentional), and surface what is not inferable. Used by the experiment step for ellipsis perspective. |

To render a template, call `render_template` with the template ref and these **top-level** context keys (not nested under `variables`). Pass the prior step's result as the named condition, not as `current_grasp` or `predicted_calibration`:

| Template ref | Required context keys | Optional context keys |
|---|---|---|
| `metacognition/meta-grasp-current` | `goal` | `session_history`, `current_state`, `prior_outcomes`, `prev_grasp`, `prior_calibration` |
| `metacognition/meta-establish-target` | `goal`, `current_condition` (Step 1 result) | — |
| `metacognition/meta-predict` | `goal`, `current_condition` (Step 1 result), `target_condition` (Step 2 result), measured `gap_before` (>0) | `prev_prediction` |
| `metacognition/meta-experiment` | `goal`, `current_condition` (Step 1 result), `prediction` (Step 3 result) | `perspectives`, `context_text`, `prev_experiment` |
| `metacognition/ellipsis-analysis` | `text`, `expectations`, `biases`, `assumptions`, `domain` | — |
| `metacognition/inquiry-engine` | `problem`, `domain`, `constraints`, `max_thoughts`, `thinking_budget` | `prior_chain`, `prior_delegation_results`, `prior_skill_match_results` |
| `metacognition/inquiry-delegate-*` | `delegation_requests` | — |

A `render_template` call renders the prompt; it does not execute the inference step or produce that step's result. Use the measured result of each step when supplying the next step's condition.

## Constraints

- All templates are prompt templates with Public visibility.
- Evaluate convergence after each full iteration using the criteria described above.
- Execute the four Kata steps (grasp, target, predict, experiment), then evaluate the gap and Brier score to determine convergence.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
