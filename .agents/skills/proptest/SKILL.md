---
name: proptest
visibility: public
description: "Property-based testing skill. Identifies testable properties from a target function's contract, designs input strategies, generates proptest code, executes it, analyzes shrunk counterexamples, and reports verified properties. Complements TDD."
---

# Proptest

Property-based testing skill. Identifies testable properties from a target function's contract, designs input strategies using the shared `hkask-test-harness` crate, generates proptest code, executes it, analyzes shrunk counterexamples, and reports verified properties. Complements TDD (which writes the first test per behavior) by writing the universal test that covers the full input space.

## When to Use

- After implementing a function with a clear invariant (e.g., compression never expands)
- When retrofitting tests for an untested pure function (e.g., `compute_budget`, `unwrap_tool_envelope`)
- When TDD's `tdd-gap-check` flags a `prob:` field gap (a probabilistic contract with no property test)
- When bug-hunt finds a class of bugs that a property test would catch (e.g., integer overflow in budget calculation)

## Instructions

1. **Identify** — Read the target's contract (`/// expect:`, `/// post:`, `/// inv:`) and source code. Classify each testable property by oracle type: `panic_freedom` (P4 — never panics on any input), `invariant` (P1 — a property holding for all inputs), `round_trip` (P1 — `deserialize(serialize(x)) == x`), `reference` (P1 — output matches an independent implementation), `idempotency` (P1 — `f(f(x)) == f(x`). If `surviving_mutants` is provided (from the `harness-optimize` skill's mutation report), prioritize properties that would kill those mutants — each surviving mutant is a concrete signal about what the test suite is missing.
2. **Strategize** — For each property, check `hkask-test-harness` first: `arb_json_value()` for JSON/YAML surfaces, `test_token_for_tool()` for governance tests, `NoopToolPort` for ToolPort stubs. If the harness doesn't have it, design a custom strategy: `select` for enums, `prop_recursive!` for recursive types, `any::<T>()` for primitives, `prop_filter` for constraints, tuple composition for structs. Use `prop_assume!` only for relational properties (never for panic-freedom).
3. **Write** — Generate the complete `proptest!` block with principle grounding comments, descriptive failure messages including the failing values, and oracle-appropriate assertions. For `round_trip`: compare field-by-field with per-field messages. For float comparisons: use the re-parse trick (serialize to string, re-parse, compare the re-parsed values). For `reference` and complex `invariant` oracle types, prefer the `hkask_test_harness` `Oracle` trait constructors (`oracle_reference`, `oracle_invariant`) over inline `prop_assert!` — this gives the `harness-optimize` skill a structured way to assess oracle variety across the suite. Keep inline `prop_assert!` for `panic_freedom` and simple one-line invariants where the `Oracle` trait adds unnecessary indirection.
4. **Analyze** — Execute `cargo test`. If it passes: all properties hold. If it fails: proptest automatically shrinks the failing input to a minimal counterexample. Classify: real bug (flag for `diagnose` skill — the shrunk counterexample is a pre-minimized reproducer) or test bug (fix the property and re-run). Handle compilation issues: `select` needs `&[...]`, `prop_assert_eq!` moves values (use `&t1.id` for repeated comparisons), missing `proptest` or `hkask-test-harness` in dev-dependencies. **Skip this phase when `execution_mode` is `generate_only`** (delegated by `harness-optimize` with `terminal` disabled — the proposer cannot run tests; CI evaluates separately). In `standalone` mode (default), write the test result and shrunk counterexample to the trace filesystem as a `{kind}-{name}.json` file per the schema at `kask/docs/architecture/test-harness-trace-schema.md` so the run is visible to `harness-optimize`.
5. **Report** — Structured report: properties verified (with oracle type and principle), failures found (with shrunk counterexample), coverage gained (what the property tests that hardcoded tests don't), harness usage, next steps.

## Execution Modes

- **`standalone`** (default): full 5-phase cascade (Identify → Strategize → Write → Analyze → Report). The agent has the `terminal` tool and runs `cargo test` in the Analyze phase. Cauchy convergence on per-function properties. Use when a human invokes proptest directly.
- **`generate_only`**: 3-phase cascade (Identify → Strategize → Write). Skip Analyze and Report. The agent does NOT run tests — it returns `test_code` + `test_file_path` + `cargo_test_command` to the caller. Use when delegated by `harness-optimize` (which has `terminal` disabled — the proposer/evaluator separation). CI runs the generated tests and writes results to the trace filesystem; `harness-optimize` reads them on the next iteration.

## Relationship to Other Skills

- **TDD**: writes the first test per behavior (one input, one assertion). Proptest writes the universal test (all inputs, one invariant). TDD's `tdd-gap-check` flags `prob:` field gaps; proptest fills them.
- **bug-hunt**: explores for unknown bugs via charter-driven probing. Proptest systematically verifies known properties. Bug-hunt's `pattern_signatures` feed into proptest's Identify phase. Bug-hunt writes findings to the trace filesystem; `harness-optimize` reads them and dispatches to proptest.
- **diagnose**: when proptest finds a real bug, the shrunk counterexample is a pre-minimized reproducer — exactly what diagnose's Phase 2 needs.
- **harness-optimize**: the suite-level proposer. Reads traces (including proptest's trace emissions) and dispatches to proptest in `generate_only` mode for under-tested functions, passing `surviving_mutants` from the mutation report.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `proptest-identify.j2` | KnowAct | Read the target function's contract (expect:, post:, inv:) and source code. Classify each testable property by oracle type: panic_freedom, invariant, round_trip, reference, or idempotency. Output properties with principle grounding (P1 or P4) and strategy hints. |
| `proptest-strategize.j2` | KnowAct | For each property, check hkask-test-harness first (arb_json_value, test_token_for_tool, NoopToolPort). If the harness doesn't provide what's needed, design a custom strategy using select, prop_recursive, any, prop_filter, and tuple composition. Output strategy code with imports and prop_assume requirements. |
| `proptest-write.j2` | KnowAct | Generate the complete proptest! block: file header with principle grounding, all arb_*() strategy functions, all test functions with oracle-appropriate assertions and descriptive failure messages. Handles the re-parse trick for float precision and per-field messages for round-trip tests. |
| `proptest-analyze.j2` | KnowAct | Execute cargo test, parse results. If the test fails, analyze the shrunk counterexample: classify as real bug (flag for diagnose skill) or test bug (fix the property). Handle common compilation issues (missing imports, select needs &[...], prop_assert_eq! moves values). |
| `proptest-report.j2` | KnowAct | Produce a structured report: properties verified with oracle type and principle grounding, failures found with shrunk counterexamples, coverage summary, harness usage, and recommended next steps. |

## Constraints

- rJoule cap: 3 per invocation. Maximum 10 iterations.
- `ledger.span_namespace: reg.skill.proptest` (CI-enforced, no `spans:` list).
- The skill does not implement code — it tests existing code's properties. For new code, use TDD first.
- For `panic_freedom` oracle: no `prop_assume!` filtering (accept all inputs). For other oracles: `prop_assume!` is allowed for relational constraints.