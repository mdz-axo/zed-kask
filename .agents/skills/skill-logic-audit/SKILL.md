---
name: skill-logic-audit
visibility: public
description: "Runtime templates for skill-logic-audit: load a goal annotation, critique the artifact against that goal, filter the critique for soundness, compose a concrete revised artifact and diff, and drive a bounded user-review loop.
"
---

# Skill Logic Audit

Runtime templates for skill-logic-audit: load a goal annotation, critique the artifact against that goal, filter the critique for soundness, compose a concrete revised artifact and diff, and drive a bounded user-review loop.


## When to Use

- When you need to parse and extract the annotated `goal:` block from a `.j2` or `manifest.yaml` file.
- When you need to perform an adversarial critique of a template or manifest body against its stated goal.
- When you need to review a critique list for soundness, separating valid, goal-anchored concerns from spurious ones.
- When you need to compose a concrete revised artifact body and a unified diff from calibrated concerns.
- When you need to present a composed proposal to the user and capture an accept, reject, or counter-proposal choice.
- When you need to plan the bounded audit cascade routing and apply branching rules on accept/reject/counter-proposal.
- When you need to compute a normalized convergence metric for audit iterations to determine if material flaws remain.

## Instructions

### load-goal

1. Locate the annotated `goal:` block in the provided file content and return it verbatim, stripped of comment markers.
2. For `.j2` files, look for a Jinja2 comment of the form `{# goal: ... #}` near the top of the file.
3. For `manifest.yaml` files, look for a YAML comment of the form `# goal: ...` near the top of the file.
4. If multiple goal comments exist, use the first one.
5. Strip leading/trailing whitespace and comment markers, but preserve the exact goal text.
6. If no goal is found, set `goal_found` to false and return an empty `goal` string.
7. Ensure `target_content` is the raw content of a `.j2` template or `.yaml` manifest; if it is from a `SKILL.md` file, set `goal_found: false` and return `file_type: "unsupported"`.

### critique-template

1. Judge whether the template body is the most logically efficient, correct, and minimal way to achieve the stated goal.
2. For each flaw, provide the location, claim, anchor to goal, severity, and suggested fix.
3. Be adversarial but grounded.
4. Reject purely stylistic complaints that do not affect logical efficiency or correctness.
5. Flag hidden assumptions, duplicated logic, vague contracts, missing error paths, and violations of the dual-layer model.

### critique-critique

1. Read the prior critique and decide which concerns are genuinely grounded in the goal, and which are spurious stylistic complaints, over-generalizations, or unsupported claims.
2. Classify a concern as valid only if it explicitly links a concrete template defect to the goal.
3. Classify a concern as spurious if it is purely stylistic, model-specific, or unsupported by the template text.
4. Classify a concern as downgraded if it is real but overstated; provide the corrected severity.
5. If the prior critique failed to surface any flaws and the goal is genuinely satisfied, preserve `no_material_flaws`.

### compose-proposal

1. Produce a concrete revised template body and a line-oriented diff given the goal, the original template, and the calibrated valid concerns.
2. Make the minimal set of changes that resolves the valid concerns while preserving the goal.
3. Keep the `[inference]` frontmatter structure intact if revising a `.j2` file.
4. Do not introduce new vocabulary terms unless they are in `crates/h

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `load-goal.j2` | WordAct | Parse the annotated `goal:` block from a .j2 or manifest.yaml file and return it as a normalized string. Verify that a goal exists and is non-empty.  |
| `critique-template.j2` | KnowAct | Adversarial critique of a template or manifest body against its stated goal. List concrete flaws, redundancies, ambiguities, and missing cases, each anchored to the goal.  |
| `critique-critique.j2` | KnowAct | Soundness review of a critique list. Separate valid, goal-anchored concerns from spurious or stylistic ones. Produce a filtered list with rationale.  |
| `compose-proposal.j2` | KnowAct | Compose a concrete revised artifact body and a unified diff against the original, given the goal and the valid concerns. Do not weaken the contract or add stubs.  |
| `user-choice.j2` | WordAct | Present the composed proposal to the user and capture a discrete choice: accept, reject, or counter-proposal. Enforce the configured loop depth limit and emit the next action.  |
| `convergence-check.j2` | KnowAct | Compute a normalized convergence metric for audit iterations. Uses calibrated verdict, valid concern count, and loop depth to output `convergence_metric` in [0,1], where 0 means no material flaws remain.  |

## Constraints

- `load-goal.j2`: Public.
- `critique-template.j2`: Public.
- `critique-critique.j2`: Public.
- `compose-proposal.j2`: Public.
- `user-choice.j2`: Public.
- `convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
