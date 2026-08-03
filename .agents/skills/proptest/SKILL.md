---
name: proptest
visibility: public
description: "Property-based testing skill. Identifies testable properties from a target function's contract, designs input strategies using the shared hkask-test-harness crate, generates proptest code, executes it, analyzes shrunk counterexamples, and reports verified properties. Complements TDD by writing the universal test that covers the full input space."
---

# Proptest

Property-based testing skill. Identifies testable properties from a target function's contract, designs input strategies using the shared `hkask-test-harness` crate, generates proptest code, executes it, analyzes shrunk counterexamples, and reports verified properties. Complements TDD (which writes the first test per behavior) by writing the universal test that covers the full input space.

## When to Use

- After implementing a function with a clear invariant (e.g., compression never expands)
- When retrofitting tests for an untested pure function (e.g., `compute_budget`, `unwrap_tool_envelope`)
- When TDD's `tdd-gap-check` flags a `prob:` field gap (a probabilistic contract with no property test)
- When bug-hunt finds a class of bugs that a property test would catch (e.g., integer overflow in budget calculation)

## Instructions

1. **Identify** — Read the target's contract (`/// expect:`, `/// post:`, `/// inv:`) and source code. Classify each testable property by oracle type: `panic_freedom` (P4 — never panics on any input), `invariant` (P1 — a property holding for all inputs), `round_trip` (P1 — `deserialize(serialize(x)) == x`), `reference` (P1 — output matches an independent implementation), `idempotency` (P1 — `f(f(x)) == f(x`).
2. **Strategize** — For each property, check `hkask-test-harness` first: `arb_json_value()` for JSON/YAML surfaces, `test_token_for_tool()` for governance tests, `NoopToolPort` for ToolPort stubs. If the harness doesn't have it, design a custom strategy: `select` for enums, `prop_recursive!` for recursive types, `any::<T>()` for primitives, `prop_filter` for constraints, tuple composition for structs. Use `prop_assume!` only for relational properties (never for panic-freedom).
3. **Write** — Generate the complete `proptest!` block with principle grounding comments, descriptive failure messages including the failing values, and oracle-appropriate assertions. For `round_trip`: compare field-by-field with per-field messages. For float comparisons: use the re-parse trick (serialize to string, re-parse, compare the re-parsed values).
4. **Analyze** — Execute `cargo test`. If it passes: all properties hold. If it fails: proptest automatically shrinks the failing input to a minimal counterexample. Classify: real bug (flag for `diagnose` skill — the shrunk counterexample is a pre-minimized reproducer) or test bug (fix the property and re-run). Handle compilation issues: `select` needs `&[...]`, `prop_assert_eq!` moves values (use `&t1.id` for repeated comparisons), missing `proptest` or `hkask-test-harness` in dev-dependencies.
5. **Report** — Structured report: properties verified (with oracle type and principle), failures found (with shrunk counterexample), coverage gained (what the property tests that hardcoded tests don't), harness usage, next steps.

## Relationship to Other Skills

- **TDD**: writes the first test per behavior (one input, one assertion). Proptest writes the universal test (all inputs, one invariant). TDD's `tdd-gap-check` flags `prob:` field gaps; proptest fills them.
- **bug-hunt**: explores for unknown bugs via charter-driven probing. Proptest systematically verifies known properties. Bug-hunt's `pattern_signatures` feed into proptest's Identify phase.
- **diagnose**: when proptest finds a real bug, the shrunk counterexample is a pre-minimized reproducer — exactly what diagnose's Phase 2 needs.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `proptest/proptest-identify.j2` | KnowAct | Identify properties from target's contract, classify by oracle type |
| `proptest/proptest-strategize.j2` | KnowAct | Design input strategies — harness-first, then custom |
| `proptest/proptest-write.j2` | KnowAct | Generate complete proptest code with principle grounding |
| `proptest/proptest-analyze.j2` | KnowAct | Execute test, analyze shrunk counterexamples |
| `proptest/proptest-report.j2` | KnowAct | Report verified properties, failures, coverage |

## Constraints

- All templates are KnowAct (inference + JSON parse). rJoule cap: 3.
- Gas cap: 80,000. Convergence: Cauchy, epsilon 0.03, window 3, max 10 iterations, min 2.
- `ledger.span_namespace: reg.skill.proptest` (CI-enforced, no `spans:` list).
- The skill does not implement code — it tests existing code's properties. For new code, use TDD first.
- For `panic_freedom` oracle: no `prop_assume!` filtering (accept all inputs). For other oracles: `prop_assume!` is allowed for relational constraints.