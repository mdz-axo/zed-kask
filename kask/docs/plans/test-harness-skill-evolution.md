# Test Harness Skill Evolution — Implementation Plan

**Status:** In-progress
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

## 3. Slice mapping

| Slice (from design doc) | This plan | Status |
|--------------------------|-----------|--------|
| Slice 1: trace FS + oracle taxonomy | §2.1, §2.2, §2.5 | Implementing now |
| proptest oracle-aware Write | §2.3 | Implementing now |
| proptest generate-only mode | §2.3 | Implementing now |
| proptest mutation-guided Identify | §2.3 | Implementing now (input field; data from Slice 2) |
| bug-hunt mutation-guided Charter | §2.4 | Implementing now (input field; data from Slice 2) |
| bug-hunt trace emission | §2.4 | Implementing now |
| Slice 2: stability gate | — | Future (requires `cargo-mutants`) |
| Slice 3: harness-evolve-cycle manifest | — | Future |
| Slice 4: harness-optimize skill | — | Future |

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