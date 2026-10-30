---
name: coding-guidelines
core: true
description: "Behavioral guardrails for LLM coding based on Karpathy's four principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution. An Encoded Preference skill: constrains HOW the agent codes, not WHAT it codes."
audience: platform
---


# Coding Guidelines

Behavioral guardrails for LLM coding based on Karpathy's four principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution. An Encoded Preference skill: constrains HOW the agent codes, not WHAT it codes.

## Reference model

The four principles are from forrestchang's `CLAUDE.md` (github.com/forrestchang/andrej-karpathy-skills), derived from Andrej Karpathy's public observations on LLM coding failure modes — the wording is forrestchang's, the diagnosis Karpathy's. The seven anti-patterns are this skill's synthesis: six trace to the same CLAUDE.md's bullets; unrequested logging/telemetry is a kask-side addition.

## D/P labelling

The assessment (step 1), the constrained plan (step 2) and the audit's four principle scores with their violations report (step 3) are P — judgment, critiqued by the operator and by the correction loop. The overall score and the gate are D (`lisp_eval`: the arithmetic mean `(/ (+ s1 s2 s3 s4) 4)` and the critical-violations/overall gate in step 4); a score the model states but does not supply to the form is not the mean — the form's env is the record.

## Initial and target condition

- **Initial condition:** the user's coding requirement, the actual code/tree or proposed diff, binding constraints, and the assumptions and success criteria produced before implementation.
- **Target condition:** the pre-implementation `CG-1` plan gate passes and the post-implementation `CG-2` gate returns true — zero critical violations and mean principle score at least 0.7 — with every changed line traced to the request. A failing audit applies its concrete corrections and re-runs once; a second failure is delivered with its violations and score rather than silently accepted.

## When to Use

- Before implementing a coding task, when you need to surface hidden assumptions, simplicity risks, and scope creep warnings
- When transforming a vague coding request into verifiable success criteria with a minimum-step plan
- When generating implementation directives that require surgical file-level scope and explicit guardrails
- After writing code or producing a diff, when you need to audit compliance against the four principles
- When the agent is about to touch code and must be constrained to minimum, surgical, goal-driven changes
- When multiple interpretations of a task exist and silent selection would violate Think Before Coding

## When NOT to Use

- Reviewing a change against its stated spec — use `code-review`; it owns adjudication with severity, falsifiers, and file:line citations.
- Non-coding work — the four principles and seven anti-patterns are defined over code.
- When a task constraint outranks a principle — the task wins, the shape stays (the skill's own override logic).

## Instructions

1. **Assess and generate constrained directives before implementing (`guidelines-assess`).** Analyze the coding task against the four Karpathy principles before any implementation begins. Surface every hidden assumption, flag every over-engineering risk, define 2–5 verifiable success criteria, and produce a `constrained_plan` where each step specifies `files_to_touch` (no more, no less), `files_not_to_touch`, `lines_estimate` (flag steps exceeding 50 lines as a simplicity risk), and a verification checkpoint, plus `guardrails` and `assumption_log`. Treat the seven canonical anti-patterns (`anti-patterns.j2`) as explicitly forbidden: unsolicited docstring/formatting changes, single-use abstractions, unrequested flexibility, adjacent-code refactoring, impossible-scenario error handling, unrequested logging/telemetry, and style changes outside task scope. Do not implement anything in this step. Gate the constrained plan with `lisp_eval` (`CG-1`): `(and (>= criteria_count 2) (<= criteria_count 5) (<= max_step_lines 50) (= untraceable_files 0))`.

2. **Verify the implementation against all four principles (`guidelines-verify`).** Audit the implementation or proposed diff. Check that assumptions were stated explicitly, multiple interpretations were presented, unclear points were questioned, and simpler approaches were considered. Confirm every feature was explicitly requested, no single-use abstractions exist, the solution is minimum code, and no speculative features or impossible-scenario error handling remain. Verify every changed line traces to the user's request, adjacent code is untouched, existing style is matched, orphan imports from your changes are removed, and pre-existing dead code was left alone (mentioned, not deleted). Confirm success criteria are defined and verifiable, tests exist for stated goals, and each criterion can be verified independently. Produce a violations report with principle, severity, location, and correction, and score compliance per principle (1.0 = full compliance, 0.0 = severe violation).

3. **Close the loop (`CG-2`).** On a failing audit (any critical violation, or
   overall < 0.7), apply the violations report's corrections to the
   diff and re-run this audit step. Gate — call `lisp_eval` with:
   - form: `(let ((overall (/ (+ s1 s2 s3 s4) 4))) (and (= (length critical_violations) 0) (>= overall 0.7)))`
   - env: `{ "critical_violations": <the violations report's critical entries as a list>, "s1": <think_before_coding>, "s2": <simplicity_first>, "s3": <surgical_changes>, "s4": <goal_driven_execution> }`
   The four principle scores and the violation list are P (the audit's judgment, critiqued by the operator); the mean, the critical-count length, and the gate are D — every gate input is derived, never model-asserted.
   Bound: max 2 audit cycles (initial + one correction pass); a second
   failing audit is delivered with the violations report and the score — the
   failure is surfaced, never silently passed.

## Interaction with output-shaping skills

When an output-shaping skill (e.g., adhd-mode) is active in the session:

1. The content obligations above (assumptions, risks, goals, guardrails,
   violations, scores) are always delivered. Shape rules arrange that content; they
   never delete it.
2. Lead with the verdict or the decision request; the structured report
   follows.
3. When a shape cap would drop required content, split and rank using the
   shape skill's own escape (adhd-mode rule 9's must/nice split) or group by
   anti-pattern — never truncate. The shape gate checks caps and shape
   fields, not absence: truncated content simply isn't there to count, so
   truncation is caught by your content-obligation check, never by the gate.
4. The shape skill's override conditions and the harness system prompt
   outrank these guidelines' presentation defaults.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `anti-patterns.j2` | Shared Jinja2 fragment listing the seven canonical Karpathy anti-patterns. Included by coding-guidelines/guidelines-assess via {% include %}. Not a standalone inference template — no `[inference]` header or contract; rendering it directly yields the bare fragment with no task context. |
| `guidelines-assess.j2` | Assess a coding task against four behavioral principles before implementation and generate constrained implementation directives (`assumptions`, `simplicity_risks`, `scope_creep_warnings`, `success_criteria`, `constrained_plan`, `guardrails`, `assumption_log`). |
| `guidelines-verify.j2` | Verify an implementation or diff against all four principles. Produces a violations report, per-principle compliance scores, and corrective recommendations. |

To render a template, call the `render_template` tool with the template ref (e.g., `coding-guidelines/guidelines-assess`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `guidelines-assess.j2`: `task_description`, `code_context`
- `guidelines-verify.j2`: `task_description`, `assumptions`, `success_criteria`, `constrained_plan`, `diff_preview`
- `anti-patterns.j2` is a shared Jinja2 fragment (no contract, not standalone-renderable) — do not render it directly.

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, batch-11 audit):

- Mean: `{s1: 1.0, s2: 0.8, s3: 0.9, s4: 0.7}` over `(/ (+ s1 s2 s3 s4) 4)` →
  `0.8500000000000001` (the float artifact is the engine's honest arithmetic;
  the gate compares it against 0.7 with `>=`).
- Gate, pass: `{critical_violations: [], overall_score: 0.85}` → `true`.
- Gate, critical violation: `{critical_violations: [<one listed critical
  entry>], overall_score: 0.9}` → `false` — the length is derived from the
  report's own list, never a model-asserted count (the pre-repair form took
  the count as a bare number, the one gate input with no deterministic
  derivation).
- Gate, below floor: `{critical_violations: [], overall_score: 0.6}` → `false`.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- Every file in a constrained plan's `files_to_touch` must trace directly to the task description
- Line estimates over 50 per step trigger a simplicity warning
