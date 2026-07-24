---
name: skill-bundler
visibility: public
description: "Orchestrate and compose multiple skills into a cohesive bundle. Activates a set of skills together, resolves conflicts, determines application order, and produces a manifest that governs how the skills compose. Re-composes the manifest when skills evolve.
"
---

# Skill Bundler

Orchestrate and compose multiple skills into a cohesive bundle. Activates a set of skills together, resolves conflicts, determines application order, and produces a manifest that governs how the skills compose. Re-composes the manifest when skills evolve.


## When to Use

- Composing a new set of skills into a cohesive bundle governed by a specific goal context.
- Validating an existing bundle manifest for structural correctness, principle compliance, and ontological anchoring.
- Re-composing a bundle when constituent skills have changed or when the bundle failed to converge on its goal (goal_delta > 0).
- Evaluating the convergence of a bundle composition loop by synthesizing compose and validate outputs against goal achievement.

## Instructions

### bundler-compose

1. Analyze the provided set of skills and explicit goal context for conflicts, complementarities, and optimal ordering.
2. Classify each skill by polarity (generative, evaluative, regulative, procedural) and assign a cascade phase (pre-core, core, post-core, cross-phase).
3. Apply composition principles, including phase separation, default cascade ordering (Recognize → Act → Reflect), and domain complementarity.
4. Resolve detected conflicts using the established hierarchy (Domain separation → Phase separation → Specificity wins → Manifest override → User intent wins).
5. Detect and resolve anti-patterns such as cancel-out, contradictory directives, ordering collisions, runaway feedback, scope creep, and dead letters.
6. Enforce depth and term limits (cascade depth ≤ 7, ≤ 10 key terms per skill, ≤ ~30 unique terms per bundle).
7. Produce a PKO-anchored bundle manifest with DC provenance and PROV-O artifact linkage, ensuring dual-axis ontological compliance.

### bundler-validate

1. Check the composed bundle manifest for structural correctness, principle compliance, ontological anchoring, and anti-pattern violations.
2. Enforce validation rules V1-V15 mechanically, including cascade depth limits, skill uniqueness, conflict resolution completeness, and phase separation.
3. Verify the presence of required PKO, DC, and Goal ontological anchors.
4. Produce a binary pass/fail verdict based on the presence of violations.
5. Provide concrete fix suggestions for every violation and recommendations for every warning.
6. List any missing PKO, DC, or PROV-O anchors as ontology gaps.

### bundler-evolve

1. Re-assess the bundle and produce an updated manifest when skills have changed or the bundle failed to converge (goal_delta > 0).
2. Prioritize recomposition decisions by what reduces the goal delta, adding or removing skills as necessary to address unfulfilled criteria.
3. Preserve stability for unchanged skills that contribute to goal achievement, retaining their polarity, phase, and cascade order unless forced otherwise.
4. Re-classify polarity and re-evaluate phase assignment for changed skills, applying the conflict resolution hierarchy if needed.
5. Re-check for new or resolved conflicts and complementarities across the entire bundle.
6. Preserve or tighten the convergence criterion based on the goal delta, and detect any drift from original composition principles.
7. Escalate if the goal delta remains flat or increases after two recomposition cycles.
8. Increment the manifest version and flag only the skills that actually changed.

### bundler-convergence-check

1. Compute a normalized convergence metric in [0,1] accounting for both structural validity and goal achievement.
2. Calculate the structural score based on the number of validation violations (errors, not warnings).
3. Calculate the goal score based on the goal verdict (0.0 for "done", 1.0 for "blocked", 0.5 for "continue").
4. Determine the final convergence metric as the maximum of the structural and goal scores.
5. Calculate the goal delta as 1.0 minus the goal verdict confidence.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `bundler-compose.j2` | KnowAct | Analyze a set of skills for conflicts, complementarities, and optimal ordering. Classify each skill by polarity (generative/evaluative/ regulative/procedural), assign cascade phase, and produce a structured bundle manifest.  |
| `bundler-synthesize.j2` | KnowAct | Synthesize the composed bundle manifest: decimate/fuse RDF graph, resolve ontology anchors, and produce the final PKO/DC/PROV-O-anchored manifest.  |
| `bundler-validate.j2` | KnowAct | Validate a composed bundle manifest: check for contradictory directives in the same cascade phase, cascade depth limits, skill uniqueness, conflict resolution completeness, and convergence criteria.  |
| `bundler-evolve.j2` | KnowAct | Re-assess a bundle when one or more skills have changed. Re-compose the manifest preserving what hasn't changed and updating what has. Detect drift from the original composition principles.  |
| `bundler-convergence-check.j2` | KnowAct | Compute a normalized convergence metric for bundle composition loops. Synthesizes compose/validate outputs into `convergence_metric` in [0,1], where 0 means no blocking composition violations remain.  |

## Constraints

- `bundler-compose.j2`: Public.
- `bundler-validate.j2`: Public.
- `bundler-evolve.j2`: Public.
- `bundler-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
