# Continuation Prompt: Validate GSR + CFR Skills with skill-maintenance

**Purpose**: Run `skill-maintenance-validate` on the two newly scaffolded skills to check them against R1-R12, Z1-Z8, X1-Z4, E1-E11. Fix any validation failures. This is the one remaining unchecked task from the cleanup plan.

## Context

Two skills were scaffolded and reviewed through 4 passes (bug-hunt, graph-audit, grill-me, essentialist). All blockers from those passes are fixed. The formal registry validator has not yet run. The skills are:

### Skill 1: gradient-seeded-recombination (GSR)

- **Process manifest**: `kask/registry/manifests/gradient-seeded-recombination.yaml`
- **Template manifest**: `kask/registry/templates/gradient-seeded-recombination/manifest.yaml`
- **Templates** (7 KnowAct): `gsr-inventory.j2`, `gsr-prior.j2`, `gsr-map.j2`, `gsr-detect.j2`, `gsr-hypothesize.j2`, `gsr-prioritize.j2`, `gsr-select-seeds.j2`
- **SKILL.md**: `.agents/skills/gradient-seeded-recombination/SKILL.md`
- **Span namespace**: `reg.skill.gradient-seeded-recombination`
- **Convergence**: Cauchy on `1.0 - field_coverage` (hypotenuse bound to Map step's field_coverage output)
- **Gas cap**: 80,000; rjoule cap: 2; max iterations: 3

### Skill 2: constraint-forces-recast (CFR)

- **Process manifest**: `kask/registry/manifests/constraint-forces-recast.yaml`
- **Template manifest**: `kask/registry/templates/constraint-forces-recast/manifest.yaml`
- **Templates** (7 KnowAct): `cfr-represent.j2`, `cfr-violate.j2`, `cfr-project.j2`, `cfr-control.j2`, `cfr-three-criterion.j2`, `cfr-compare.j2`, `cfr-frontier.j2`
- **SKILL.md**: `.agents/skills/constraint-forces-recast/SKILL.md`
- **Span namespace**: `reg.skill.constraint-forces-recast`
- **Convergence**: `lisp.eval` with `form: "frontier_changed_flag + 0.05 * new_non_dominated"` and `env` containing `frontier_changed_flag` and `new_non_dominated`; threshold 0.10; min iterations 2
- **Gas cap**: 150,000; rjoule cap: 4; max iterations: 5

## Fixes already applied (from bug-hunt + graph-audit + grill-me + essentialist)

1. GSR convergence: `hypotenuse` bound to `1.0 - field_coverage` (was hardcoded `0.0`); `kata_hypotenuse` reads `step_8_result.hypotenuse` (was reading nonexistent `convergence_metric`); `next_prior_focus` derived from `step_6_result.priority_ranking[0].location` (was reading nonexistent field from `kata.convergence_check`).
2. CFR convergence: `kata_hypotenuse` reads `step_8_result` directly (was reading nonexistent `.result`); frontier carried via `carried_frontier` loop variable (was reading nonexistent `_convergence.frontier_history`); seed index uses top-level `seed_index` (was using nonexistent `_loop.seed_index`).
3. Both skills: `type: object` corrected to `type: array` for collection inputs (`ontology_registry`, `seed_concepts`).
4. Both skills: dead RenderAct templates deleted (7/7 surfaces each, ≤7 rule satisfied).
5. CFR: `rater` input wired into `cfr-three-criterion.j2` procedure body.
6. CFR: T1' and "not yet enforced" language added to manifest description.
7. CFR: all steps (1-5) use `seed_concepts[seed_index | default(0)]` consistently.
8. CFR: `lisp.eval` input shape uses `form` + `env` (matching executor's expected shape).
9. GSR: `gsr-inventory.j2` namespace_id uses `5W1H` (was `5wh1`).
10. GSR: `gsr-prioritize.j2` priority order expanded to full 7-class (was collapsed to 5).

## Task

Invoke the `skill-maintenance` skill (or the `skill-maintenance-validate` template directly) to validate both skills against:
- **R1-R12**: registry format rules (manifest structure, template entries, span namespace format, gas/rjoule/convergence blocks)
- **Z1-Z8**: companion checks (SKILL.md frontmatter, description ≤1024 bytes, template table consistency)
- **X1-X4**: cross-artifact checks (template_ref resolution, input_mapping field consistency, contract type alignment)
- **E1-E11**: extended checks (OCAP/gas posture, delegate existence, convergence block validity)

For each validation failure:
1. Record the rule ID, the failure description, and the file/line.
2. Fix the failure in the manifest, template, or SKILL.md.
3. Re-validate to confirm the fix.

Continue until both skills pass all checks, or until you hit a failure that requires operator decision (escalate with the specific rule ID and failure description).

## Known potential issues to watch for

- The `lisp.eval` compute_ref in CFR step 8 is relatively new — the validator may not have a rule for it. If the validator rejects `compute_ref: lisp.eval`, check whether the rule needs updating or whether the compute_ref should be `kata.convergence_check` with a different input shape.
- The `carried_frontier` loop variable in CFR is a custom state-carry mechanism — the validator may not recognize it. If it's rejected, document that the executor's loop handler binds `input_mapping` keys as top-level context keys (per `executor.rs:775-792`).
- The GSR `ontology_registry` input type is `array` but the description mentions "Each entry carries..." — the validator may flag this as a type/description mismatch. If so, the description is correct (each entry in the array carries the fields); the type is `array` (a list of objects).
- The CFR manifest description is long (>1024 bytes?) — check whether it exceeds the SKILL.md description limit. The manifest `description` field and the SKILL.md `description` frontmatter are different fields with different limits; the validator may check both.

## Acceptance criteria

- [ ] Both skills pass R1-R12, Z1-Z8, X1-Z4, E1-E11 (or failures are fixed)
- [ ] Validation results recorded in `kask/docs/research/tasks/todo.md` (check the `skill-maintenance-validate` checkbox)
- [ ] Any unfixable failures escalated with rule ID and description
