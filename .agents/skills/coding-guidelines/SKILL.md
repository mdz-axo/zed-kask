---
name: coding-guidelines
visibility: public
description: "Behavioral guardrails for LLM coding based on Karpathy's four principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution. An Encoded Preference skill: constrains HOW the agent codes, not WHAT it codes."
---


# Coding Guidelines

Behavioral guardrails for LLM coding based on Karpathy's four principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution. An Encoded Preference skill: constrains HOW the agent codes, not WHAT it codes.

## When to Use

- Before implementing a coding task, when you need to surface hidden assumptions, simplicity risks, and scope creep warnings
- When transforming a vague coding request into verifiable success criteria with a minimum-step plan
- When generating implementation directives that require surgical file-level scope and explicit guardrails
- After writing code or producing a diff, when you need to audit compliance against the four principles
- When the agent is about to touch code and must be constrained to minimum, surgical, goal-driven changes
- When multiple interpretations of a task exist and silent selection would violate Think Before Coding

## Instructions

1. **Assess before implementing.** Analyze the coding task against the four Karpathy principles before any implementation begins. Surface every hidden assumption, flag every over-engineering risk, and define verifiable success criteria. State assumptions explicitly with confidence levels and alternative interpretations. Identify simplicity risks with severity and concrete simplifications. Flag scope creep. Transform vague tasks into 2–5 verifiable goals. Outline minimum implementation steps, each with a verification checkpoint. Do not implement anything — this step is purely diagnostic.

2. **Generate constrained implementation directives.** Take the assessment and produce a constrained plan. For each step, specify exactly which files to touch (no more, no less), which files not to touch, an estimated line count (flag steps exceeding 50 lines as a simplicity risk), and a verification method. Produce guardrails covering maximum scope, forbidden patterns, and style matching rules. Maintain an assumption log of resolved assumptions and how they were decided. Treat the seven canonical anti-patterns as explicitly forbidden: unsolicited docstring/formatting changes, single-use abstractions, unrequested flexibility, adjacent-code refactoring, impossible-scenario error handling, unrequested logging/telemetry, and style changes outside task scope.

3. **Verify the implementation against all four principles.** Audit the implementation or proposed diff. Check that assumptions were stated explicitly, multiple interpretations were presented, unclear points were questioned, and simpler approaches were considered. Confirm every feature was explicitly requested, no single-use abstractions exist, the solution is minimum code, and no speculative features or impossible-scenario error handling remain. Verify every changed line traces to the user's request, adjacent code is untouched, existing style is matched, orphan imports from your changes are removed, and pre-existing dead code was left alone (mentioned, not deleted). Confirm success criteria are defined and verifiable, tests exist for stated goals, and each criterion can be verified independently. Produce a violations report with principle, severity, location, and correction. Score compliance per principle (1.0 = full compliance, 0.0 = severe violation); overall is the arithmetic mean. Mark passed only if there are zero critical violations and overall ≥ 0.7. Be strict — a 200-line solution that could be 50 lines is a critical violation.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `anti-patterns.j2` | `RenderAct` | Shared Jinja2 fragment listing the seven canonical Karpathy anti-patterns. Included by coding-guidelines/guidelines-apply via {% include %}. Not a standalone renderable template — no inference header or contract. |
| `guidelines-assess.j2` | `KnowAct` | Assess a coding task against four behavioral principles before implementation. Surfaces assumptions, simplicity risks, scope creep warnings, and success criteria. |
| `guidelines-apply.j2` | `KnowAct` | Generate constrained implementation directives from the assessment. Produces file-level guardrails, forbidden patterns, and style matching rules. |
| `guidelines-verify.j2` | `KnowAct` | Verify an implementation or diff against all four principles. Produces a violations report, compliance scores, and corrective recommendations. |

## Constraints

- Visibility is Public across all KnowAct templates; the anti-patterns fragment has no standalone contract
- Energy caps: 5120 for guidelines-assess and guidelines-verify; 4096 for guidelines-apply
- Safety mode, when enabled, enforces no file system access, no network calls, no environment variable access, and strict Jinja2 sandbox enforcement
- Do not execute arbitrary Python code in Jinja2 expressions — sandboxed execution only
- Preserve original prompt structure and formatting; handle missing variables gracefully
- Every file in a constrained plan's `files_to_touch` must trace directly to the task description
- Line estimates over 50 per step trigger a simplicity warning
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins