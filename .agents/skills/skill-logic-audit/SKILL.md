---
name: skill-logic-audit
core: true
description: "Bounded dual-layer logic audit of .j2 templates and manifest.yaml files against their stated goals. Loads the annotated goal block, generates adversarial critique, filters for soundness, composes a revised artifact with unified diff, and drives a user-review loop."
---

# Skill Logic Audit

Bounded logic audit of .j2 templates against their annotated goals (and legacy manifest.yaml files as inert artifacts, not runtime skill definitions). Unfolded from skill-maintenance (originally folded 2026-07-25, unfolded 2026-08-14).

## The composition law (context for every audit)

A kask skill's core process — including at least one PDCA
(Plan→Do→Check→Act) self-improvement loop — lives in the SKILL.md body.
Templates are leaves: steps of that loop, rendered by `render_template`
at the points the SKILL.md directs. A template may contain its own
internal loop, but it is never the carrier of the skill's core loop.
Audit a template as a step-leaf: its goal must serve the SKILL.md phase
that invokes it, its inputs must match what that phase passes, and its
outputs must feed the phase that consumes them.

## When to Use

- Auditing a .j2 template's logic against its stated `{# goal: ... #}` annotation
- Auditing an explicitly requested legacy manifest.yaml's annotated text, without treating it as a live skill contract
- Composing a revised artifact with a unified diff from calibrated concerns
- Driving a user-review loop for accept/reject/counter-proposal

## When NOT to Use

- SKILL.md bodies — not valid audit targets (this skill's own constraint); use `skill-maintenance`.
- Skill health scoring, staleness signals, retirement thresholds — `skill-maintenance-audit`.
- Coverage-gap mapping against the corpus — `skill-maintenance-coverage`.

## Instructions

### logic-load-goal

Parse the annotated goal block from the target file. For .j2 files, look for `{# goal: ... #}`. For manifest.yaml files, look for `# goal: ...`. Strip comment markers, preserve exact goal text.

### logic-critique-template

Adversarial critique of the template body against its stated goal. For each flaw, provide location, claim, anchor to goal, severity, and suggested fix.

### logic-critique-critique

Review the critique for soundness — separate valid, goal-anchored concerns from spurious ones.

### logic-compose-proposal

Compose a concrete revised artifact and unified diff from the calibrated concerns.

### logic-user-choice

1. Present the proposed artifact, unified diff, and rationale to the human user. A model critique or template output is a recommendation, **not** the user's choice.
2. Stop and wait for the user's actual accept, reject, or counter-proposal response; if absent or ambiguous, leave the target unchanged and ask. Never populate `user_choice` from model inference.
3. On reject, stop without editing. On counter-proposal, compose a revised proposal and show its new diff for another explicit choice (maximum 3 rounds; then report unresolved). On accept, confirm the accepted diff still matches the current file before writing; if the file drifted, re-present the diff for fresh approval. Only then edit the target. No tool automates this gate.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `logic-load-goal.j2` | Parse the annotated goal: block from a .j2 or manifest.yaml file and return it as a normalized string. Verify that a goal exists and is non-empty. |
| `logic-critique-template.j2` | Adversarial critique anchored to the extracted goal. For each flaw provide the location, claim, anchor to goal, severity, and suggested fix. |
| `logic-critique-critique.j2` | Review a critique for soundness and goal-anchoring. Separate valid goal-anchored concerns from spurious ones. |
| `logic-compose-proposal.j2` | Compose a concrete revised artifact and unified diff from the calibrated concerns. |
| `logic-user-choice.j2` | Present the proposal and diff to the human; wait for an actual response before any edit. Never generate the user's choice. |

To render a template, call the `render_template` tool with the template ref (e.g., `skill-logic-audit/logic-load-goal`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `logic-load-goal.j2`: `target_path`,`target_content`
- `logic-user-choice.j2`: `target_path`,`goal`,`proposal`,`diff`,`rationale`,`confidence` (presentation inputs only; no user decision input)

## Constraints

- `logic-load-goal.j2`: Operates on .j2 templates and .yaml manifests ONLY. SKILL.md files are NOT valid audit targets.
- `logic-critique-template.j2`: Be adversarial but grounded. Reject purely stylistic complaints that do not affect logical efficiency or correctness.
- `logic-critique-critique.j2`: A concern is valid only if it explicitly links a concrete template defect to the goal.
- `logic-compose-proposal.j2`: Make the minimal set of changes that resolves the valid concerns while preserving the goal.
- `logic-user-choice.j2`: Only the human's actual response can select accept, reject, or counter-proposal. Missing/ambiguous response is no authorization; a template must not emit `user_choice` or `next_action: write` on the user's behalf. Verify the approved diff against the current target before writing.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
