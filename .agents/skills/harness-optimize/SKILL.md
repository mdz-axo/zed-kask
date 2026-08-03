---
name: harness-optimize
visibility: public
description: "Suite-level test harness proposer. Reads raw execution traces from the trace filesystem (test results, mutation scores, bug-hunt findings), identifies under-tested areas, and proposes test improvements. Dispatches to proptest in generate_only mode for specific functions. Cannot run tests — enforced by the agent profile (terminal tool disabled). Composes with bug-hunt (exploratory) and proptest (per-function) in the harness-evolve-cycle loop."
---

# Harness Optimize

Suite-level test harness proposer. Reads raw execution traces from the trace
filesystem (`kask/traces/`), identifies under-tested areas by analyzing
mutation scores and failure patterns, and proposes test improvements. Dispatches
to `proptest` in `generate_only` mode for specific under-tested functions.

## When to Use

- When invoked by the `harness-evolve-cycle` orchestrator after the stability
  gate returns `proceed`
- When a human wants to improve the test suite based on trace data
- When mutation testing reveals surviving mutants that need property tests

## OCAP Enforcement

This skill is a **proposer**, not an evaluator. It must run under an agent
profile with the `terminal` tool **disabled** (`profile.is_tool_enabled("terminal")
= false`). This is enforced at `crates/agent_settings/src/agent_profile.rs` L114.
With `terminal` disabled, the proposer cannot run `cargo test` or `cargo nextest`
— it can only read files (`read_file`, `list_directory`, `grep`, `find_path`)
and invoke skills (`skill`). CI is the independent evaluator that runs the
proposed tests.

Alternatively, run as a swarm delegate (`hkask-mcp-swarm` `agent_executor.rs`),
which has no built-in tools at all — only MCP tools from `declared_tools`.

## Instructions

1. **Identify** — Read the trace filesystem for revision N and N−1:
   - `kask/traces/<run-id-N>/metrics.json` — mutation_score, coverage_pct, pass_rate
   - `kask/traces/<run-id-N>/failures/*/output.txt` — raw failure output
   - `kask/traces/<run-id-N>/failures/*/shrunk.txt` — proptest shrunk counterexamples
   - `kask/traces/<run-id-N>/failures/*/classifier.json` — qa-triage classifier output
   - `kask/traces/<run-id-N>/bug-hunt-report.json` — bug-hunt expedition findings (if present)
   - Compare N vs N−1: what new failures appeared? What mutants still survive? What
     did the previous revision miss that this one should catch? (Pairwise refinement —
     Recursive Self-Improvement paper.)

2. **Propose** — Based on the gap analysis, propose a test improvement diff:
   - For under-tested functions with surviving mutants: dispatch to `proptest` in
     `generate_only` mode, passing `surviving_mutants` from the mutation report.
   - For bugs found by bug-hunt: propose tests that would catch those bugs.
   - For flaky tests (classifier `is_real_bug: false`): propose fixes to the test
     (stronger `prop_assume!`, better oracle, deterministic setup).
   - The proposal cites specific trace files as evidence (Meta-Harness: the proposer
     reads raw traces, not compressed summaries).
   - The proposal does NOT run tests. It returns test code + file paths for CI to
     evaluate.

3. **Report** — Structured report:
   - `proposed_diff`: new/modified test files with rationale
   - `evidence`: trace files cited (at least 3 specific files)
   - `proptest_dispatches`: functions dispatched to proptest with surviving_mutants
   - `expected_impact`: which mutants should be killed, which bugs should be caught
   - `cost_estimate`: approximate CI runtime for the new tests

## Relationship to Other Skills

- **proptest**: per-function property test generator. `harness-optimize` dispatches
  to `proptest` in `generate_only` mode for specific under-tested functions, passing
  `surviving_mutants` from the mutation report.
- **bug-hunt**: exploratory bug finder. Bug-hunt writes findings to the trace
  filesystem; `harness-optimize` reads them and proposes tests for the bugs found.
  Bug-hunt runs with `terminal` enabled (evaluator side); `harness-optimize` runs
  with `terminal` disabled (proposer side).
- **harness-evolve-cycle**: the orchestrator that invokes `harness-optimize` after
  the stability gate returns `proceed`. The cycle handles the loop, stability gate,
  and algedonic escalation.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `harness-optimize/harness-optimize-identify.j2` | KnowAct | Read traces, identify under-tested areas via mutation analysis + failure patterns |
| `harness-optimize/harness-optimize-propose.j2` | KnowAct | Propose test improvement diff with evidence from traces |
| `harness-optimize/harness-optimize-report.j2` | KnowAct | Structured report of proposed changes with rationale and expected impact |

## Constraints

- All templates are KnowAct (inference + JSON parse). rJoule cap: 2.
- Gas cap: 120,000. Convergence: single-pass (no loop — convergence is at the
  `harness-evolve-cycle` level).
- `ledger.span_namespace: reg.skill.harness-optimize` (CI-enforced, no `spans:` list).
- The skill does NOT run tests. It proposes test code for CI to evaluate.
- The skill reads raw traces, not compressed summaries (Meta-Harness paper).
- Proposals must cite at least 3 specific trace files as evidence.
- Proposals are pairwise: compare revision N vs N−1, not against an undefined
  "optimal" (Recursive Self-Improvement paper).