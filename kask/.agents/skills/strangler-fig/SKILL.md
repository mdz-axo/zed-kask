---
name: strangler-fig
visibility: public
description: "Incremental architectural migration using Martin Fowler's Strangler Fig pattern. Migrate one domain at a time: create new implementation alongside old, wire consumers one by one, delete only after full verification. System remains fully functional at every intermediate step.
"
---

# Strangler Fig

Incremental architectural migration using Martin Fowler's Strangler Fig pattern. Migrate one domain at a time: create new implementation alongside old, wire consumers one by one, delete only after full verification. System remains fully functional at every intermediate step.


## When to Use

- When planning an incremental architectural migration using the Strangler Fig pattern by mapping domains, classifying consumer overlap, and sequencing migration by risk.
- When executing a single domain migration step (CREATE, WIRE, or DELETE) and requiring immediate build and test verification before proceeding.
- When performing a standalone post-migration verification pass to check old-path integrity, reversibility, and surface completeness against pre-existing migration output.
- When evaluating the convergence of a strangler-fig PDCA migration cycle to determine if migration step integrity is verified and no critical rollback blockers remain.

## Instructions

### strangler-fig-plan

1. Identify every bounded context in the current architecture, noting operations, state, and dependencies for each domain.
2. Map consumers for each domain, capturing the exact code path, return type, error type, and configuration or state passed.
3. Classify overlap for each domain with multiple consumers as Identical, Divergent, Surface-only, or Cross-cutting.
4. Sequence domains by ascending risk: Proof of Concept, Independent domains, Dependent domains, and Cross-cutting infrastructure.
5. Produce a CREATE→WIRE→DELETE migration plan for each domain.
6. Respond with a JSON object containing domains, consumer_map, overlap_classification, migration_sequence, migration_plan, risk_assessment, and assumptions.

### strangler-fig-execute

1. Execute exactly one migration step from the plan: CREATE, WIRE a consumer, or DELETE old code.
2. For CREATE steps, implement the new component with domain types only, write tests in isolation, and run `cargo test -p <new_crate>`.
3. For WIRE steps, modify the consumer to call the new component, keep the old code path intact, and run `cargo test -p <consumer_crate>`.
4. For DELETE steps, confirm all consumers are wired and verified, remove duplicated logic, and run `cargo check --workspace && cargo test --workspace`.
5. After every sub-step, execute `cargo check --workspace` and `cargo test --workspace`.
6. Revert immediately if verification fails; do not proceed with known breakage.
7. Respond with a JSON object containing step_result, verification_results, rolled_back, warnings, next_step, and constraint_classification.

### strangler-fig-verify

1. Perform a standalone post-migration verification pass against pre-existing migration output.
2. Verify build integrity by ensuring `cargo check --workspace` passes with no errors or broken references.
3. Verify test integrity by ensuring `cargo test --workspace` passes with no regressions.
4. Verify old path integrity by confirming old code is still present, compiles, and could be restored with a single git revert.
5. Verify reversibility by checking if the step can be reversed with `git revert` and identifying any irreversible side effects.
6. Verify surface completeness by ensuring all consumers are accounted for and none are partially migrated.
7. Respond with a JSON object containing verification_status, regressions, old_path_intact, reversibility_check, and recommendations.

### strangler-convergence-check

1. Measure convergence on a scale of [0,1] where 0 means migration step integrity is verified and no critical rollback blockers remain.
2. Score how much work remains based on the provided plan, execute, and verify results.
3. Return JSON only containing convergence_metric, convergence_method, rationale, and blockers.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `strangler-fig-plan.j2` | KnowAct | Map domains, identify consumers, classify overlap (Identical/Divergent/ Surface-only), and sequence migration by risk. Produce a prioritized migration plan with per-domain CREATE→WIRE→DELETE steps.  |
| `strangler-fig-execute.j2` | KnowAct | Execute one domain migration step: create new component, wire one consumer at a time, verify each consumer, delete old code only after full wiring. Produces a step completion report with verification results.  |
| `strangler-fig-verify.j2` | KnowAct | Verify system functional at intermediate migration step. Run workspace build + test + lint, detect regressions, confirm old path intact until deletion, verify reversibility of current step.  |
| `strangler-convergence-check.j2` | KnowAct | Compute normalized convergence metric for strangler-fig PDCA cycles.  |

## Constraints

- `strangler-fig-plan.j2`: Public.
- `strangler-fig-execute.j2`: Public.
- `strangler-fig-verify.j2`: Public.
- `strangler-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
