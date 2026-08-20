---
name: goal-analysis
description: "Goal specification and verification. Extracts structured goals from user intent, judges completion via semantic evaluation or command execution, and produces calibrated verdicts with confidence scoring."
---

# Goal Analysis

Goal specification and verification. Extracts structured goals from user intent, judges completion via semantic evaluation or command execution, and produces calibrated verdicts with confidence scoring. This skill provides the full PDCA lifecycle for goal management — from intention extraction through model-evaluated convergence and resolution routing.

## When to Use

- When a user expresses a natural-language intention that needs to be structured into a clear, actionable goal with observable completion criteria
- When you need to verify whether an agent has achieved a stated goal via semantic evaluation of outcomes and artifacts
- When you need to verify goal completion via executed command results (exit codes, stdout pattern matching)
- When the primary verification system is unavailable and a lightweight fallback judgment is needed
- When you need to compute a normalized convergence metric to assess whether a PDCA cycle has stabilized
- When a goal needs to be activated for Regulation span tracking and execution context preparation
- When a judge verdict needs to be routed to a resolution action (complete, continue, or escalate to human)

## Instructions

### Goal Creation (create.j2)

1. Pledge to clear goal articulation.
2. Commit to observable completion criteria.
3. Undertake the minimal coordination substrate.
4. Promise that shared language + shared goals = collaboration.
5. Extract a structured goal with: `goal_text` (one clear sentence), `criteria` (2–4 observable semantic conditions), `visibility` (`private` | `shared` | `public`), and `priority` (`low` | `medium` | `high`).
6. Keep the goal minimal — just text + criteria + state.
7. Use criteria designed for LLM verification (avoids Goodhart's law).
8. Default visibility to `private` to preserve user sovereignty.

### Goal Activation (goal-activate.j2)

1. Activate a structured goal for tracking.
2. Emit the Regulation create span and record activation.
3. Return activation status, span emission flag, and a derived goal ID.

### Goal Judge — Semantic (judge.j2)

1. Evaluate goal completion against each explicit criterion.
2. Ground the verdict in observable outcome and artifacts.
3. Assert a confidence score.
4. Return `done` only when the outcome satisfies all explicit completion criteria.
5. Return `blocked` when the outcome explains the goal is unachievable or needs user input.
6. Return `continue` otherwise — the agent must continue work.

### Goal Judge — Command (judge_command.j2)

1. For each command-type criterion, compare the actual exit code against the expected exit code.
2. For each state-type criterion, check whether the expected pattern appears in the command's stdout.
3. Mark each criterion as passed or failed.
4. If all criteria pass, return verdict `done`.
5. If any criteria fail, return verdict `continue` with the list of failed criterion indices.

### Goal Judge — Simple Fallback (judge_simple.j2)

1. When the verification system is unavailable, default to a `continue` verdict.
2. Set confidence to 0.5.
3. Instruct the agent to continue toward the goal.

### Goal Resolution (goal-resolve.j2)

1. Resolve the goal based on the judge's verdict.
2. If verdict is `done` and confidence ≥ 0.7, mark complete and emit `reg.goal.complete`.
3. If verdict is `done` but confidence < 0.7, escalate to human and emit `reg.goal.alert.escalate`.
4. If verdict is `continue`, continue the loop and emit `reg.goal.transition`.
5. If verdict is `blocked`, escalate to human and emit `reg.goal.block`.
6. Any verdict with confidence < 0.7 escalates to human — low confidence may still be wrong.
7. Emit the appropriate Regulation spans for the chosen resolution.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `create.j2` | WordAct | Extract a structured goal from raw user intent. Produces goal text, completion criteria, visibility setting, and priority level. |
| `judge.j2` | KnowAct | Verify goal completion via semantic evaluation of outcome summary and produced artifacts against the original goal criteria. |
| `judge_command.j2` | KnowAct | Verify goal completion via executed command results against acceptance criteria. Produces a done/continue/blocked verdict with reasoning. |
| `judge_simple.j2` | KnowAct | Fallback goal verification with minimal evaluation. Produces a continue verdict and default confidence for lightweight judgment. |
| `goal-activate.j2` | KnowAct | Activate a goal for tracking. Registers the goal with the goal management system and returns an activation confirmation. |
| `goal-resolve.j2` | KnowAct | Resolve a goal as completed or blocked. Produces a final resolution record with the verdict, confidence, and reason. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- All templates declare `visibility: Public` at the template level; goal-level visibility defaults to `private` to preserve user sovereignty.
- Criteria are designed for LLM-judged semantic verification, not deterministic checks — this avoids Goodhart's law.
- Low confidence (< 0.7) escalates to human regardless of verdict.
- Evaluate convergence after each full iteration: the iterates have stopped moving. Converged when stable across 3 iterations. Maximum 10 iterations; minimum 2 iterations before declaring convergence.
- Goals coordinate across human, userpod, and bot agents.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.