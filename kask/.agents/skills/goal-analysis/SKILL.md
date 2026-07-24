---
name: goal-analysis
visibility: public
description: "Goal specification and verification. Extracts structured goals from user intent, judges completion via semantic evaluation or command execution, and produces calibrated verdicts with confidence scoring."
---

# Goal Analysis

Goal specification and verification. Extracts structured goals from user intent, judges completion via semantic evaluation or command execution, and produces calibrated verdicts with confidence scoring. This skill provides the full PDCA (Plan-Do-Check-Act) lifecycle for goal management — from intention extraction through convergence-checked verification and resolution routing.

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

### Goal Convergence Check (goal-convergence-check.j2)

1. Measure convergence on a [0, 1] scale where 0 means the verdict is confidently resolved for the current cycle.
2. Score how much work remains — 1 means not converged.
3. Use LLM-assessed saturation detection as the convergence method.
4. Return the convergence metric, method, and rationale.

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
|----------|------|--------|
| `create.j2` | `WordAct` | Extract a structured goal from raw user intent. Produces goal text, completion criteria, visibility setting, and priority level. |
| `judge.j2` | `KnowAct` | Verify goal completion via semantic evaluation of outcome summary and produced artifacts against the original goal criteria. |
| `judge_command.j2` | `KnowAct` | Verify goal completion via executed command results against acceptance criteria. Produces a done/continue/blocked verdict with reasoning. |
| `judge_simple.j2` | `KnowAct` | Fallback goal verification with minimal evaluation. Produces a continue verdict and default confidence for lightweight judgment. |
| `goal-convergence-check.j2` | `KnowAct` | Compute normalized convergence metric for goal-analysis PDCA cycles. |

> **Note:** Two additional template files exist in the crate but are not listed in the manifest's `templates` array: `goal-activate.j2` (KnowAct — emit Regulation span for goal activation) and `goal-resolve.j2` (KnowAct — route goal verdict to resolution action). Their instructions are included above but they lack manifest-level registration. See warnings.

## Constraints

- All templates declare `visibility: Public` at the template level; goal-level visibility defaults to `private` to preserve user sovereignty.
- Energy caps: `create.j2`, `judge.j2`, and `judge_command.j2` at 4096; `goal-activate.j2`, `goal-convergence-check.j2`, `goal-resolve.j2`, and `judge_simple.j2` at 2048.
- Criteria are designed for LLM-judged semantic verification, not deterministic checks — this avoids Goodhart's law.
- Low confidence (< 0.7) escalates to human regardless of verdict.
- Convergence threshold defaults to 0.25; max iterations default to 3; improvement target defaults to 0.05.
- Goals coordinate across human, userpod, and bot agents.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.