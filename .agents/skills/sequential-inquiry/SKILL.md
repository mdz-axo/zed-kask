---
name: sequential-inquiry
description: "Dynamic chain-of-thought reasoning engine following the Toyota Improvement Kata. Grasps current understanding, establishes a target, predicts which delegation closes the gap, runs the engine, and measures convergence deterministically (gap + Brier)."
---

# Sequential Inquiry

Dynamic chain-of-thought reasoning engine following the Toyota Improvement Kata.
The skill runs actual PDCA: grasp the current understanding, establish a target
understanding, predict which deep-dive delegation will close the gap, run the
inquiry engine with delegation, measure the gap, and score the prediction via
Brier. Convergence is detected deterministically.

## When to Use

- When an agent needs to reason through a complex problem with branching, revision, and hypothesis testing.
- When an agent needs to delegate to specialized skills (hypothesis-framer, mcda, diagnose, falsifiability) based on the problem's needs.
- When the convergence decision should be deterministic (gap + Brier) rather than an LLM self-grade.

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

### Convergence (Steps 6-10: Check + Act — deterministic compute, no LLM)

1. Compute object-space gap (thought chain completeness).
2. Compute process-space gap (delegation resolution).
3. Compute hypotenuse.
4. Score the prediction via Brier.
5. Check convergence: gap, Cauchy, or calibration.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `sequential-inquiry-grasp.j2` | KnowAct | Kata Step 1: measure the current understanding. |
| `sequential-inquiry-target.j2` | KnowAct | Kata Step 2: declare the target understanding. |
| `sequential-inquiry-predict.j2` | KnowAct | Kata Step 3: predict which delegation will close the gap. |
| `sequential-inquiry-engine.j2` | KnowAct | Kata Step 4: run the inquiry engine with delegation. |
| `sequential-inquiry-delegate-hypothesis-framer.j2` | KnowAct | Delegation: FINER+PICO hypothesis framing. |
| `sequential-inquiry-delegate-mcda.j2` | KnowAct | Delegation: multi-criteria decision analysis. |
| `sequential-inquiry-delegate-diagnose.j2` | KnowAct | Delegation: disciplined diagnosis loop. |
| `sequential-inquiry-delegate-falsifiability.j2` | KnowAct | Delegation: eliminative inference (Popper/Platt/Chamberlin/Pearl). |

## Constraints

- All flow templates are KnowAct type with Public visibility.
- Energy caps: grasp (6144), target (4096), predict (4096), engine (9000).
- Gas cap: 120,000 per invocation. Maximum 10 iterations (safety valve).
- The convergence decision is deterministic (compute steps) — no LLM convergence-check template.
- Registry is authoritative.
