# Test Harness Skill Evolution — Implementation Plan

**Status:** Implemented (Slices 1–6 complete)
**Date:** 2026-08-03
**Design parent:** `kask/docs/plans/evolving-test-harness.md` (§3.3–3.8, §10)
**Scope:** `hkask-test-harness` crate, `kask/scripts/test`, `proptest` skill, `bug-hunt` skill

---

## 1. Objective

Evolve the `proptest` and `bug-hunt` skills to integrate with the new test
harness infrastructure (trace filesystem, oracle taxonomy, mutation-guided
targeting), and implement Slice 1 of the harness design (the foundation: `Oracle`
trait, `write_trace`, trace flag).

## 2. Changes

### 2.1 `hkask-test-harness` crate (Slice 1 foundation)

Add 6 new public items to `kask/crates/hkask-test-harness/src/hkask_test_harness.rs`:

| Item | Kind | Purpose |
|------|------|---------|
| `Oracle` | trait | `verify(&self, input: &JsonValue, output: &JsonValue) -> OracleVerdict` |
| `OracleVerdict` | enum | `Pass`, `Fail(String)`, `Inconclusive` |
| `oracle_hardcoded` | fn | Oracle 1: compare against a fixed expected value |
| `oracle_reference` | fn | Oracle 2: compare against a reference implementation |
| `oracle_invariant` | fn | Oracle 3: check an invariant predicate |
| `write_trace` | fn | Write a `TraceEntry` to `kask/traces/<run-id>/` |
| `TraceEntry` | struct | Structured trace record (test name, result, duration, shrunk counterexample, oracle type, kind) |

No new dependencies — JSON constructed via `serde_json::json!`.

### 2.2 `kask/scripts/test` (trace flag)

Add `--trace` flag: when set, run nextest with `--message-format json` and write
`kask/traces/<run-id>/nextest-output.json` + `manifest.json` + `metrics.json`.

### 2.3 `proptest` skill evolution

| Change | Template / file | Detail |
|--------|-----------------|--------|
| Generate-only mode | SKILL.md + `proptest-analyze.j2` | Add `execution_mode: "standalone" \| "generate_only"` input. In `generate_only`, skip Analyze — return test code for CI to run. |
| Oracle-trait-aware Write | `proptest-write.j2` | For `reference` and complex `invariant` oracles, emit `oracle_reference` / `oracle_invariant` calls instead of inline `prop_assert!`. Keep inline for `panic_freedom` and simple invariants. |
| Mutation-guided Identify | `proptest-identify.j2` + SKILL.md | Add `surviving_mutants` input (optional). When present, prioritize properties that would kill those mutants. |
| Trace emission in Analyze | SKILL.md | In standalone mode, write the test result + shrunk counterexample to `kask/traces/` via `write_trace`. |

### 2.4 `bug-hunt` skill evolution

| Change | Template / file | Detail |
|--------|-----------------|--------|
| Mutation-guided Charter | `bug-hunt-charter.j2` + SKILL.md | Add `mutation_report` input (optional). When present, prioritize `target_area` toward functions with surviving mutants. |
| Trace emission in Report | `bug-hunt-report.j2` + SKILL.md | Write the expedition report to `kask/traces/<run-id>/bug-hunt-report.json` for consumption by `harness-optimize`. |

### 2.5 `.gitignore`

Add `kask/traces/` (trace artifacts are ephemeral, like `target/`).

### 2.6 `tdd` skill evolution (orchestration)

TDD is the spec-driven conductor of the testing-skill DAG. It keeps its
contract-first red-green-refactor discipline and orchestrates `proptest` and
`bug-hunt` as delegates at specific phases, with the orchestration enforced as
manifest steps (not template prose) so the feedback loop closes mechanically.

| Change | Template / file | Detail |
|--------|-----------------|--------|
| Strengthen step (NEW) | `tdd-strengthen.j2` + `tdd.yaml` step 3 | After the tracer bullet is GREEN, dispatch to `proptest` (standalone mode) for the universal test of property-shaped contracts (`reference`/`invariant` oracle). A proptest fail is a second source of RED routing back to the tracer (impl wrong) or the plan (contract wrong). Skips cleanly for `hardcoded` contracts. |
| Explore step (NEW) | `tdd-explore.j2` + `tdd.yaml` step 7 | After gap-check, dispatch to `bug-hunt` (scoped charter) when coverage is thin OR the slice touches Trust (P0). Findings not covered by an existing tracer bullet become new functional requirements routing back to the plan. |
| Oracle selection in tracer | `tdd-tracer.j2` | Select `oracle_type` (hardcoded/reference/invariant) from the contract layers. New `test_style: "property"` branch. Emits `oracle_type` + `proptest_dispatch` output fields driving step 3. |
| Trace-producing verify | `tdd-verify.j2` | Switch test command to `./scripts/test --trace` so the run is captured in the trace filesystem for `harness-optimize`. Consume `proptest_verdict`; a `fail` forces `all_tests_pass: false`. Check property contracts have a universal test. |
| Harness-fed gap-check | `tdd-gap-check.j2` | Consume `bug_hunt_findings` (spec blind spots) and `surviving_mutants` (weak universal tests) as additional gap sources. |
| Convergence gating | `tdd.yaml` step 8 | Convergence requires the artifact triple to stabilize AND `proptest_violations == 0` AND `bug_hunt_new_gaps == 0` — false convergence with open violations/gaps is prevented. |
| Optional harness inputs | `tdd.yaml` inputs | `surviving_mutants` and `bug_hunt_findings` (consumed when available from prior harness-evolve-cycle runs). |
| SKILL.md orchestration docs | `.agents/skills/tdd/SKILL.md` | "Relationship to Other Skills" + "Oracle Mapping" sections documenting TDD as conductor of proptest + bug-hunt, producer for harness-optimize, optional consumer of harness-optimize outputs. |

**Two time scales, deliberately not merged:** TDD orchestrates proptest/bug-hunt
*within* a single vertical slice's red-green-refactor cycle (per-slice).
`harness-optimize` reads the traces TDD's `--trace` runs produce and proposes
suite improvements *between* feature work (suite-level, via
`harness-evolve-cycle`). TDD does not orchestrate harness-optimize within a slice.

**Robustness to the harness data-flow breaks** (see evolving-test-harness.md
§9.5): TDD's orchestration of proptest/bug-hunt is live (both skills work
today). TDD's trace production is live (raw `nextest-output.json` is written by
`--trace`; harness-optimize reads raw traces). `surviving_mutants` consumption
is optional — not a required dependency, so the broken local-path mutation
scoring does not block TDD. TDD's per-slice loop closes on live signals
(proptest verdicts, bug-hunt findings) even while the suite-level stability gate
is broken.

## 3. Slice mapping

| Slice (from design doc) | This plan | Status |
|--------------------------|-----------|--------|
| Slice 1: trace FS + oracle taxonomy | §2.1, §2.2, §2.5 | ✅ Done |
| proptest oracle-aware Write | §2.3 | ✅ Done |
| proptest generate-only mode | §2.3 | ✅ Done |
| proptest mutation-guided Identify | §2.3 | ✅ Done |
| bug-hunt mutation-guided Charter | §2.4 | ✅ Done |
| bug-hunt trace emission | §2.4 | ✅ Done |
| TDD orchestration (strengthen + explore) | §2.6 | ✅ Done |
| Slice 2: stability gate | `kask/scripts/stability-gate.sh` | ✅ Done |
| Slice 3: harness-evolve-cycle | `kask/scripts/harness-evolve-cycle.sh` + `kask/registry/manifests/harness-evolve-cycle.yaml` | ✅ Done |
| Slice 4: harness-optimize skill | `.agents/skills/harness-optimize/` + `kask/registry/templates/harness-optimize/` + `kask/registry/manifests/harness-optimize.yaml` | ✅ Done |
| Slice 5: CyberneticsLoop sensors | `TestCoverageSensor` + `MutationScoreSensor` in `hkask-regulation` | ✅ Done |
| Slice 6: CI evaluator | `.github/workflows/kask-ci.yml` (mutation testing + trace upload) | ✅ Done |

## 4. Acceptance criteria

1. `cargo test -p hkask-test-harness` passes with the new items.
2. A test using `oracle_invariant(|input, output| { ... })` compiles and runs.
3. `write_trace("test-run", &entry)` produces a file at `kask/traces/test-run/`.
4. `./scripts/test --trace` produces `kask/traces/<run-id>/nextest-output.json`.
5. `proptest-identify.j2` input contract includes `surviving_mutants` (optional).
6. `proptest-analyze.j2` input contract includes `execution_mode`.
7. `proptest-write.j2` includes oracle-trait guidance for reference/invariant.
8. `bug-hunt-charter.j2` input contract includes `mutation_report` (optional).
9. `bug-hunt-report.j2` includes trace emission instruction.
10. `tdd.yaml` has 9 steps (plan → tracer → strengthen → refactor → verify → gap-check → explore → convergence → loop).
11. `tdd-strengthen.j2` dispatches to proptest in standalone mode for `reference`/`invariant` oracle types and skips for `hardcoded`.
12. `tdd-explore.j2` dispatches to bug-hunt only when coverage is thin OR slice touches Trust (P0).
13. `tdd-verify.j2` uses `./scripts/test --trace` and consumes `proptest_verdict`.
14. `tdd-gap-check.j2` consumes `bug_hunt_findings` and `surviving_mutants` as gap sources.
15. `tdd.yaml` step 3 branches: `proceed`→4, `retracer`→2, `replan`→1; step 7 branches: `replan`→1, `converge`→8.
16. No new non-canonical `reg.*` spans introduced in the new templates (reuses existing `reg.contract.*`).