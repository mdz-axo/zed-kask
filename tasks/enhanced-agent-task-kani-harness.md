# Kani-era test-harness recomposition — program charter

Provenance: derived 2026-09-18 from the operator's request via the prompt-enhance skill
(type `agent-task`, medium effort, decoupled grill verdict **pass**: Recall Solid, Mechanism Solid).
This charter is the spec for the Phase 0–3 work program. Phase 0 output: the decision brief.
Nothing in this charter authorizes test rewrites before the Phase 0 operator gate.

---

# Task: Kani-era test-harness recomposition — decide first, then pilot

## Phase 0 — Decision brief (no code changes)
First verify the premise: the Kani toolchain is installed in this workspace (e.g. `cargo kani --version`). If it is not actually available, the brief must say so.

Answer: SHOULD we review our test harness, testing protocols, and recompose tests across the kask core and MCP server crates now? Deliver:
1. Test inventory: per crate, count tests and give a failure-class histogram (specific-regression / behavior-at-seam / invariant / loop-closure / proof). Classify individual tests exhaustively only for loop-critical areas named below; full per-test classification is deferred to the pilot crate.
2. Loop inventory: enumerate the cybernetic and self-improvement feedback loops from the tree itself — e.g. prediction-market calibration store + Brier scoring, curator memory write→recall round-trips, algedonic alert surfacing, escalation lifecycle, forecast persistence + outcome scoring, skill-use feedback records. For each loop: does any test today verify the loop CLOSES (write → observe → recall), or only assert current values? A write path with no recall-path test is a loop silently dropped.
3. Gradient analysis: where is the gap widest between loop criticality and current coverage? Name the highest-gradient crate.
4. Recommendation: proceed / proceed-pilot-first / defer, each with falsifiable acceptance criteria. The wholesale "convert unit tests to proptest" direction is a hypothesis this brief must confirm or reject from evidence — not an assumption.

Operator gate: present the brief and wait. No test is rewritten before the operator confirms the decision.

## Phase 1 — Protocol design (no code changes)
Design the target architecture as a layered system with a per-layer assignment rule (what a test must be checking to belong there) and a per-layer unprovability statement (what it can never prove):
- Unit / example tests — specific known regressions, behavior at module seams. Not everything converts; assignment is by failure class, not fashion.
- Property tests (proptest) — invariants over input domains: parsers, chunk policies, ledger arithmetic, calibration math, schema validation. Each property stated as a falsifiable hypothesis with its input domain.
- Kani proofs — bounded model checking for panic-freedom and contract enforcement on pure, contract-critical logic (e.g. JSON array extraction, ledger transaction math). Cost-gated: proofs target small pure modules with explicit assumptions, stubs, and bounds — never the whole workspace.
- Loop-closure tests — end-to-end write→observe→recall round-trips per loop from the Phase 0 inventory, asserting degraded paths are SURFACED (a note/status/mode naming the reason), never empty-equals-success.

## Phase 2 — Pilot (one crate: operator-named, or the highest-gradient crate from Phase 0)
Recompose the pilot crate against the protocol, including the exhaustive per-test classification deferred from Phase 0. Done means: `./script/clippy` clean; full test suite green; at least one proptest property and one Kani proof landed; every deleted test has a named replacement in a replaced-by mapping; completion cited by commit hash.

## Phase 3 — Propagation plan (documented, not executed)
A per-crate migration checklist derived from the pilot, ordered by the Phase 0 gradient. Propagation is a separate, explicitly gated work item.

## Method skills — each routed to a phase with a named deliverable
- hypothesis-framer → Phases 0–1: hypothesis register, FINER-checked, each testing hypothesis falsifiable.
- falsifiability → Phase 1: admissibility gate per property; per-layer unprovability statements.
- metacognition → Phase 0: the harness's blind spots measured, not asserted.
- pragmatic-cybernetics → Phase 0: the loop inventory; Ashby check that test variety matches failure-class variety.
- essentialist → Phases 1–2: every new harness helper/fixture survives the deletion test; no accreted scaffolding.
- grill-me → end of each phase: interrogate that phase's deliverable (recall → mechanism → rationale → edge cases → synthesis).
- refactor-architecture → Phases 2–3: migration planning, strangler-fig per crate, orphan-free deletions.
- lean-prover → only if spec formalization demonstrably improves the Kani contract specs. This is a Rust/Kani workspace, not Lean — establish applicability first, or record the skip with reasons.

## Hard constraints (project rules, non-negotiable)
- No `unwrap()` in tests or harness code; `./script/clippy` is the build oracle.
- Live-mutation probe suites stay `--test-threads=1`; MCP tool tests construct servers with the capability under test; recall-path tests cover the entity_ref JOIN.
- The tree never breaks mid-refactor: a test API change lands complete across crates in one pass or not at all.
- Pathspec-limited commits; completion records cite commit hashes, never intentions.
- Deletions clean up what they orphan. No upstream (non-kask) test edits unless a D-seam entry + test accompany them.

## Termination
This program terminates when the operator's confirmed decision criteria are met, or when Phase 0 returns "defer". It does not continue unbounded; each phase has the done-condition stated above, and no phase starts while its predecessor's gate is open.

---

## Appendix — original operator request (verbatim, pre-enhancement)

"now that we have installed kani toolchain -- should we review our test harness and testing protocols and recompose the tests throughout the kask core and mcp server crates?  this would be a mssive clean up and refactor of the tests from unit tests to proptest to composing a genuine cybernetic test harness.  please use metacognition, grill-me, essentialist, pragmatic cybernetics, lean prover, lasifiability and hypothesis framer and refactor architecture skills in the process of thinking through the implications and how to improve the code through the process into a well composing testing system to support the godel machine evolution and cybernetic feedback loops and  other loops in  the code."