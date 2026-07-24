---
name: tdd
visibility: public
description: "Test-driven development with red-green-refactor loop, MDS spec-anchored functional testing, and gap analysis. Builds features or fixes bugs one vertical slice at a time. Enforces behavior testing through public interfaces, spec traceability via // REQ: tags (anchored to MDS spec/goal/capture outputs), anti-horizontal-slicing, and minimal-implementation discipline. Five MDS categories: domain, composition, trust, lifecycle, curation.
"
---

# Tdd

Test-driven development with red-green-refactor loop, MDS spec-anchored functional testing, and gap analysis. Builds features or fixes bugs one vertical slice at a time. Enforces behavior testing through public interfaces, spec traceability via // REQ: tags (anchored to MDS spec/goal/capture outputs), anti-horizontal-slicing, and minimal-implementation discipline. Five MDS categories: domain, composition, trust, lifecycle, curation.


## When to Use

- Planning a TDD cycle before any code is written — extracting functional requirements from specifications with goal-principle anchoring, identifying public interfaces, classifying behaviors by MDS category, and prioritizing by risk.
- Writing a single tracer bullet: one contract for one behavior, one failing test verifying the contract, then minimal implementation to satisfy it — contract-first, vertical-slice discipline.
- Refactoring while all tests are GREEN — extracting duplication, deepening modules, strengthening contracts, and applying SOLID principles while preserving contract metadata and verifying tests pass after each step.
- Verifying TDD cycle completion — checking all tests pass, clippy is clean, no `todo!()`/`unimplemented!()` stubs remain, contract structure is complete, and spec traceability is intact.
- Performing functional gap analysis — comparing specification requirements against tested behaviors, scoring expectation quality (0–3), cross-referencing goal-principle alignment against MDS category defaults, and producing deferral recommendations for `OPEN_QUESTIONS.md`.
- Computing a normalized convergence metric for TDD PDCA cycles — measuring completeness fraction across RED→GREEN→REFACTOR phases with blocker tracking.

## Instructions

### tdd-plan

1. Read `docs/architecture/core/FUNCTIONAL_SPECIFICATION.md` for the relevant domain before planning any test.
2. Identify the functional requirement (FR#) that each behavior addresses.
3. Run the contract-generator to produce the `expect:` + `[P{N}]` contract annotation for each requirement.
4. Review each contract — the `expect:` field is the ground truth for what the test verifies.
5. Only proceed to write tests after the contract passes quality scoring (≥2).
6. For each requirement in scope, identify the `user_expectation` (verbal, user's voice), the `goal_principle` (exactly one of P1–P12), and `constraining_principles` (zero to many).
7. List specific observable behaviors testable through a public interface; classify each as `unit`, `integration`, `contract`, `fuzz`, or `system`.
8. Describe the public interface changes needed (new, modified, or removed items).
9. Rank behaviors by risk: P0 = security/correctness-critical (Trust), P1 = correctness (Domain, Composition), P2 = ergonomics (Lifecycle, Curation).
10. List every assumption with confidence (high/medium/low) and the alternative interpretation.
11. Do NOT write any code — this is planning only.

### tdd-tracer

1. Write ONE contract for ONE behavior, then ONE failing test verifying the contract, then the minimal implementation to satisfy the contract — contract-first ordering: (1) Contract → (2) Test → (3) Implementation.
2. Author the contract as a `///` doc-comment on the function signature with all layers: `expect:` (verbal, user's voice), `[P{N}] Motivating:` (exactly one goal principle), `pre:`/`post:` (behavioral specification), `inv:` (optional, for types), and `[P{N}] Constraining:` (zero to many; minimum all applicable Magna Carta P1–P4).
3. Select the goal principle whose user-visible guarantee the contract directly serves; if multiple apply, choose the most directly exercised.
4. Determine constraining principle applicability by asking: "Would implementing this contract without respecting this principle violate it?" If yes, annotate it.
5. Write the test — it must fail (RED). Verify preconditions via `prop_assume!` and assert postconditions via `prop_assert!`.
6. Carry a descriptive doc comment on the test function referencing the contract's `expect:` statement.
7. Write the minimal implementation to satisfy the contract (GREEN) — no speculative features, no extra error handling for impossible scenarios.
8. Test through the public interface (the declared seam) only; do not test private methods or internal state.
9. For fuzz tests, accept all inputs with no `prop_assume!` filtering and verify panic-freedom via `catch_unwind`.
10. For system tests, exercise the full vertical slice end-to-end using `hkask-test-harness` for fixtures.

### tdd-refactor

1. Confirm all tests pass (GREEN) before refactoring — never refactor while RED.
2. Identify refactor candidates: extract duplication, deepen modules, strengthen contracts (weaken preconditions, strengthen postconditions, add invariants), apply SOLID where it improves locality, reduce public surface.
3. Execute one refactor step at a time; run `cargo test -p <crate>` after each step.
4. Never change behavior — if tests break, revert.
5. Never add features — refactoring changes structure, not behavior.
6. Preserve all contract layers (`expect:`, `[P{N}] Motivating:`, `[P{N}] Constraining:`, `pre:`/`post:`) during refactoring; contract metadata must travel with the function when moved or renamed.
7. When merging functions sharing a goal principle, merge their contracts — preserve the goal, union the constraints. When splitting a function, each new function gets a complete contract with all layers.
8. Evolve contracts when facts justify it: update the contract annotation on the function signature and verify with existing tests. If tests fail under a stricter contract, treat it as a new tracer bullet, not a refactor.
9. After each step, run `grep -rn "/// expect:" crates/ --include="*.rs" | wc -l` and `grep -rn "/// \[P[0-9]*\]" crates/ --include="*.rs" | wc -l`; compare against pre-refactor counts — any decrease means contract metadata was lost; revert.
10. Flag contract evolution requiring P2 consent (changed `expect:` or goal principle) with `severity: high` and do not merge without human approval.

### tdd-verify

1. Verify each test describes behavior, not implementation, and uses the public interface (seam) only.
2. Confirm each test would survive an internal refactor and that no horizontal slicing occurred.
3. Classify implementation-coupled tests carrying `// TEST-DEBT:` comments as medium-severity test-debt, not violations.
4. Verify each test carries a contract annotation (`expect:` + `[P{N}]`) referencing a valid functional requirement.
5. Confirm no specification requirement in scope is missing a tracer bullet.
6. Verify every contract carries the full structure: `expect:` field present, `[P{N}] Motivating:` present (exactly one), `[P{N}] Constraining:` annotations present (minimum P1–P4 where applicable), `pre:`/`post:` present.
7. Reject vacuous `expect:` fields that restate the function name or the postcondition verbatim — the `expect:` must express what the user needs.
8. Cross-check the contract's `[P{N}]` goal principle against the MDS category default; flag mismatches requiring rationale.
9. Validate semantic alignment between `expect:` natural language and `pre:`/`post:` formal specification — check for ambiguity, contradiction, and vacuous equivalence.
10. Confirm no `todo!()` or `unimplemented!()` stubs exist.
11. Run `cargo test -p <crate>`, `cargo clippy -p <crate> -- -D warnings`, and `cargo check -p <crate>`.
12. Emit `reg.contract.violated` spans for missing `expect:` (critical), missing `[P{N}] Motivating:` (critical), missing `[P{N}] Constraining:` when Magna Carta applies (high), and expectation-postcondition mismatches.

### tdd-gap-check

1. Match each tested behavior to a functional requirement via its contract annotation (`expect:` + `[P{N}]`); flag behaviors without annotations as unanchored.
2. Identify gaps: functional requirements with no matching tested behavior.
3. Derive priority from MDS category: P0 = Trust, P1 = Domain/Composition, P2 = Lifecycle/Curation.
4. For specs where `is_complete()` returns false, include unsatisfied `Criterion` items as additional gaps.
5. Verify probabilistic contracts governing non-deterministic behavior include a `prob:` field; absence is a coverage gap.
6. Score each contract's `expect:` field on a 0–3 scale: 0 = empty/missing, 1 = vacuous, 2 = functional, 3 = anchored with principle rationale. Contracts scoring 0 or 1 must appear as gaps.
7. Verify the `[P{N}]` goal principle matches the spec's declared goal principle; correct mismatches.
8. Check constraining principle completeness: for each of P1–P12, ask whether implementing the contract without that principle would violate it; if yes and it is missing, flag as a gap.
9. Verify each `[P{N}]` Constraining annotation has an enforcement test; declarative-only constraints are coverage gaps.
10. Cross-reference MDS category alignment: confirm the contract's goal principle matches the MDS default; flag deviations with rationale.
11. Check Magna Carta completeness: list which of P1–P4 are missing from constraining annotations per covered requirement.
12. Ensure every requirement appears in exactly one of: `covered_requirements`, `gaps`, or `deferrals`.
13. P0 gaps MUST recommend `tracer-bullet`; P1 gaps SHOULD recommend `tracer-bullet` (deferrals require explicit rationale); P2+ gaps MAY defer to `OPEN_QUESTIONS.md`.

### tdd-convergence-check

1. Check each TDD phase: RED (failing test written) → +0.33 if missing; GREEN (test passing) → +0.33 if incomplete; REFACTOR (cleanup done) → +0.20 if incomplete.
2. Check test quality: missing behavioral contract (`pre:`/`post:`) → +0.10; coverage below threshold → +0.10; test doesn't reproduce reported bug (if fixing) → +0.10.
3. Check process adherence: tests written after implementation (not TDD) → +0.15; no regression check on existing tests → +0.05.
4. Start at 1.0, subtract for each completed/passing item; clamp to [0, 1].
5. Compare the resulting metric against the convergence threshold (default 0.05) — converged when metric ≤ threshold.
6. Track specific blockers (incomplete phases or failing checks) in the output.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `tdd-plan.j2` | KnowAct | Plan a TDD cycle: extract functional requirements from specifications with goal-principle anchoring (each requirement names its P{N} principle), identify public interfaces, classify behaviors by MDS category with default goal-principle mapping, prioritize by risk (P0-P2+), and get user approval before writing any code.  |
| `tdd-tracer.j2` | KnowAct | Execute a tracer bullet: write ONE failing test for ONE behavior anchored to a spec requirement AND goal principle via expect: field with [P{N}] tag (per PRINCIPLES.md §1.6). Contract-first: full v0.28.0 structure with expect:, [P{N}] Constraining: annotations, and pre:/post:. Then minimal code to satisfy the contract.  |
| `tdd-refactor.j2` | KnowAct | Refactor while GREEN: extract duplication, deepen modules, apply SOLID principles. Preserve full v0.28.0 contract structure (expect:, [P{N}] Constraining:, pre:/post:) during refactoring. Contract metadata must travel with the function (Rule 6bis). Post-refactor grep verification for expect: and [P{N}] annotations (Rule 8bis). Flag contract evolution requiring P2 consent. Verify tests still pass after each refactor step.  |
| `tdd-verify.j2` | KnowAct | Verify TDD cycle completion: all tests pass, clippy clean, no todo!/unimplemented! stubs. Contract completeness audit including expect: user expectation, [P{N}] goal-principle anchoring, and [P{N}] Constraining: annotations per v0.28.0 extended syntax. Emits reg.contract.violated spans for missing/malformed contracts. Tests describe behavior not implementation, spec traceability via // REQ: tags, functional coverage gaps identified.  |
| `tdd-gap-check.j2` | KnowAct | Functional gap analysis: compare specification requirements against tested behaviors including goal-principle alignment cross-reference against MDS category defaults, constraining principle completeness (Magna Carta P1-P4), and expectation quality scoring (0-3 scale). Identify uncovered requirements (gaps) and produce deferral recommendations for OPEN_QUESTIONS.md. P0 gaps MUST have tracer bullets.  |
| `tdd-convergence-check.j2` | KnowAct | Compute normalized convergence metric for TDD PDCA cycles. Returns convergence_metric plus rationale and blockers.  |

## Constraints

- `tdd-plan.j2`: Public.
- `tdd-tracer.j2`: Public.
- `tdd-refactor.j2`: Public.
- `tdd-verify.j2`: Public.
- `tdd-gap-check.j2`: Public.
- `tdd-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
