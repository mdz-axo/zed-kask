---
name: skill-logic-audit
core: true
description: "Bounded dual-layer logic audit of .j2 templates and manifest.yaml files against their stated goals. Loads the annotated goal block, generates adversarial critique, filters for soundness, composes a revised artifact with unified diff, and drives a user-review loop."
---

# Skill Logic Audit

Bounded dual-layer logic audit of .j2 templates and manifest.yaml files against their stated goals. Unfolded from skill-maintenance (originally folded 2026-07-25, unfolded 2026-08-14).

## When to Use

- Auditing a .j2 template's logic against its stated `{# goal: ... #}` annotation
- Auditing a manifest.yaml's logic against its stated `# goal: ...` annotation
- Composing a revised artifact with a unified diff from calibrated concerns
- Driving a user-review loop for accept/reject/counter-proposal

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

Present the proposal to the user and capture accept/reject/counter-proposal.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `logic-load-goal.j2` | WordAct | Parse the annotated goal: block from a .j2 or manifest.yaml file and return it as a normalized string. Verify that a goal exists and is non-empty. |
| `logic-critique-template.j2` | KnowAct | Adversarial critique anchored to the extracted goal. For each flaw provide the location, claim, anchor to goal, severity, and suggested fix. |
| `logic-critique-critique.j2` | KnowAct | Review a critique for soundness and goal-anchoring. Separate valid goal-anchored concerns from spurious ones. |
| `logic-compose-proposal.j2` | KnowAct | Compose a concrete revised artifact and unified diff from the calibrated concerns. |
| `logic-user-choice.j2` | KnowAct | Present the proposal to the user and capture accept, reject, or counter-proposal choice. |

## Constraints

- `logic-load-goal.j2`: Operates on .j2 templates and .yaml manifests ONLY. SKILL.md files are NOT valid audit targets.
- `logic-critique-template.j2`: Be adversarial but grounded. Reject purely stylistic complaints that do not affect logical efficiency or correctness.
- `logic-critique-critique.j2`: A concern is valid only if it explicitly links a concrete template defect to the goal.
- `logic-compose-proposal.j2`: Make the minimal set of changes that resolves the valid concerns while preserving the goal.
- `logic-user-choice.j2`: The allowed choices are accept, reject, or counter-proposal.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
