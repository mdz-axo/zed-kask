---
name: sequential-inquiry
description: "Dynamic chain-of-thought reasoning engine following the Toyota Improvement Kata. Grasps current understanding, establishes a target, predicts which delegation closes the gap, runs the engine, and evaluates convergence (gap + Brier)."
---

# Sequential Inquiry

Dynamic chain-of-thought reasoning engine following the Toyota Improvement Kata.
The skill runs actual PDCA: grasp the current understanding, establish a target
understanding, predict which deep-dive delegation will close the gap, run the
inquiry engine with delegation, measure the gap, and score the prediction via
Brier. Evaluate convergence after each iteration.

## When to Use

- When an agent needs to reason through a complex problem with branching, revision, and hypothesis testing.
- When an agent needs to delegate to specialized skills (hypothesis-framer, mcda, diagnose, falsifiability) based on the problem's needs.

## Instructions

### sequential-inquiry-grasp (Kata Step 1: Grasp Current Condition)

1. Measure the current understanding — what thoughts exist, what delegations are resolved, what's the confidence.
2. Produce current_artifacts and current_procedure for gap computation.

### sequential-inquiry-target (Kata Step 2: Establish Target Condition)

1. Declare the target understanding — what "sufficient understanding" looks like.
2. Produce target_artifacts and target_procedure.

### sequential-inquiry-predict (Kata Step 3: Make a Prediction)

1. Predict which deep-dive delegation will close the gap most.
2. Carry a confidence for Brier scoring.

### sequential-inquiry-engine (Kata Step 4: Experiment / Do)

1. Run the inquiry engine with the predicted delegation.
2. Generate, branch, revise, hypothesize, and verify thoughts.
3. Re-measure the current condition after the experiment.

### skill-router-match (Kata Step 5: Skill-Router Dispatch — cross-skill template_ref, conditional)

1. Cross-skill reuse: dispatches `skill_match_queries` emitted by the engine to the `skill-router/skill-router-match` template (`kask/registry/templates/skill-router/skill-router-match.j2`).
2. Conditional on the result of step 4's `skill_match_queries` — if the engine emitted no queries, this step returns an empty result.
3. Returns up to 3 ranked skill recommendations for follow-up delegations.

### Convergence (Steps 6-10: Check + Act — model-evaluated)

1. Step 6 (`kata.object_gap`): compute Dublin Core object-space gap between current thought chain artifacts and the target spec.
2. Step 7 (`kata.process_gap`): compute PKO process-space gap between current inquiry procedure state and target.
3. Step 8 (`kata.hypotenuse`): compute total distance to target in combined space.
4. Step 9 (`kata.prediction_vs_result`): Brier score for this cycle's prediction.
5. Step 10 (call `lisp_eval`): convergence signal — the hypotenuse value from step 8. Lower signal variance across iterations = convergence (stability check).
6. Step 11 (re-enter the cycle): re-enter the Kata cycle at step 1 if not converged.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `sequential-inquiry-grasp.j2` | KnowAct | Measure the agent's current understanding of the problem. Produces current_artifacts and current_procedure for gap computation. |
| `sequential-inquiry-target.j2` | KnowAct | Declare the target understanding — what sufficient understanding looks like for this problem. Produces target_artifacts and target_procedure. |
| `sequential-inquiry-predict.j2` | KnowAct | Predict which deep-dive delegation will close the gap and by how much. Carry a confidence for Brier scoring. |
| `sequential-inquiry-engine.j2` | KnowAct | Core reasoning engine — advances the chain-of-thought with the predicted delegation. Generates, branches, revises, hypothesizes, and verifies. Re-measures the current condition after the experiment. |
| `sequential-inquiry-delegate-hypothesis-framer.j2` | KnowAct | Delegation target — frames a research question / testable hypothesis via FINER + PICO when the engine detects a question-framing subproblem. |
| `sequential-inquiry-delegate-mcda.j2` | KnowAct | Delegation target — multi-criteria decision analysis when the engine detects a choice among alternatives requiring structured tradeoff. |
| `sequential-inquiry-delegate-diagnose.j2` | KnowAct | Delegation target — disciplined diagnosis loop when the engine detects a bug or regression requiring reproduce → anchor → hypothesize → fix. |
| `sequential-inquiry-delegate-falsifiability.j2` | KnowAct | Delegation target — eliminative inference engine when the engine branches on a counterfactual scenario or needs to rule out the untestable. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- All flow templates are KnowAct type with Public visibility.
- Evaluate convergence after each full iteration using the criteria described above.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
