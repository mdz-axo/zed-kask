---
name: verification-compression
description: "Compress and accelerate a software verification workflow without losing expectation coverage, falsifiers, failure visibility, provenance strength, or fault-detection signal. Builds a typed verification graph, composes kata-improvement, falsifiability, refactor-architecture, essentialist, and lean-prover, proves graph-preservation obligations in Lean, and accepts reductions only after fixed-oracle before/after experiments."
---

# Verification Compression

Reduce the cost and code graph of verification while preserving what the
verification can actually distinguish. The unit of preservation is not a test
count or coverage percentage. It is a **signal key**:

`(expectation_id, falsifier_id, oracle_kind, failure_class, provenance_tier)`.

A check is redundant only when every signal key it carries is supplied by a
retained check and representative harmful changes remain detectable.

## Grounding

- **Expectation contracts:** `kask/docs/reference/testing-protocol.md` — start
  from the user-visible outcome, falsifier, independent oracle, and authority.
- **Requirement-preserving reduction:** Harrold, Gupta, and Soffa (1993) — a
  reduced suite is admissible only when the representative subset still covers
  the declared test requirements.
- **Fault signal:** Jia and Harman (2011) — mutation testing is a fault-based
  adequacy instrument. Inozemtseva and Holmes (2014) warn that coverage is not
  a valid quality target by itself.
- **Architecture:** Ousterhout's deep modules and deletion test; the existing
  `refactor-architecture` and `essentialist` skills own the reduction method.
- **Eliminative inference:** Popper, Chamberlin, Platt, and Pearl/Halpern; the
  `falsifiability` skill owns admissibility and discriminating tests.
- **Formal preservation:** Curry-Howard and Lean's propositions-as-types. Lean
  proves the finite graph relation only; it does not prove that a test is a
  good oracle.
- **Improvement loop:** Rother's Toyota Improvement Kata supplies direction,
  current condition, target condition, and one-change experiments.
- **Ontology:** PKO models the procedure and step verification; SEPIO grounds
  evidence and provenance. Project term resolution currently falls back to
  `5w1h_core`; do not invent a private ontology meaning.

## Composed skills

| Skill | Responsibility |
|---|---|
| `kata-improvement` | Direction, baseline, target, bounded experiment |
| `falsifiability` | Admissibility, alternative hypotheses, counterfactuals, discriminating faults |
| `refactor-architecture` | Verification graph friction, deepening candidates, duplication, migration |
| `essentialist` | Exist → Surface → Contract elimination gates |
| `lean-prover` | Machine-checked coverage-preservation obligations |

## When to Use

- A test/check/proof pipeline is slow, duplicated, or hard to understand.
- Verification commands repeat the same oracle or compile graph.
- The user asks to minimize, compress, simplify, prioritize, or accelerate
  verification without lowering quality.
- A proposed refactor needs evidence that removed checks carried no unique
  signal.

## When NOT to Use

- The product behavior is currently broken or undiagnosed; use `diagnose`.
- No expectation contract or harmful counterexample exists.
- The only proposed metric is test count, annotation count, or code coverage.
- A formal claim concerns I/O, timing, floating point, or external effects that
  the Lean model does not represent; retain empirical tests for those claims.
- The caller wants a one-off faster command but not a reusable verification
  improvement.

## Inputs

Record before starting:

- `target`: verification workflow, paths, and commands;
- `mode`: `analyze` or `execute`;
- `authorized_contract`: user decision or canonical specification;
- `required_expectations`: expectation IDs and falsifiers;
- `sample_count`: at least 2 comparable timed runs per claimed timing state; 3 is preferred;
- `timing_scope`: `cold`, `warm`, or `both`; select before measuring;
  unselected states are `not_run` and earn no speedup claim;
- `compression_target` and `speed_target`: caller-selected, never hidden;
- `max_cycles`: 1–3, default 2.

`execute` mode requires explicit operator approval. `analyze` mode never edits
verification code.

## Instructions

### PLAN — Direction and current condition

1. Call `skill` for `kata-improvement`. Execute its direction and current-
   condition steps over the target. Measure rather than estimate:
   - verification-workflow command/test/script/fixture/proof/checker nodes;
   - production code crate/module/function/type nodes in the chosen scope;
   - `invokes`, `depends_on`, `verifies`, `detects`, `duplicates`, and
     production `calls` / `uses` edges, with file:line evidence;
   - wall time, toolchain, cache state, environment, and sample count;
   - every signal key and its producing artifact.
2. Call `read_file` for
   `kask/registry/templates/verification-compression/inventory.j2` and produce
   its JSON shape. Keep observations separate from interpretations. Populate
   `measurement_context` with toolchain, environment/cache identity, source and
   oracle hashes, sample count, and raw evidence paths.
3. Call `lisp_eval` to ensure every required expectation has at least one
   baseline signal:
   - form: `(and (= (assoc "missing_expectations" gate) 0) (= (assoc "unfalsifiable_expectations" gate) 0))`
   - env: `{ "gate": <inventory coverage gate> }`
   - A false result blocks compression and returns to step 1.

### DO — Falsify, architect, and eliminate

4. Call `skill` for `falsifiability`. Test the claim “artifact X carries unique
   verification signal.” Generate at least these alternatives: unique signal,
   duplicate signal, weak/oracle-substitution signal, and orchestration-only
   overhead. Design discriminating harmful changes or fault injections; plain
   coverage does not discriminate.
5. Call `skill` for `refactor-architecture`. Explore the verification graph,
   rank deepening candidates, audit duplicated operations, and select one
   candidate only. Do not refactor multiple domains in one cycle.
6. Call `skill` for `essentialist`. Run Exist → Surface → Contract in advisory
   mode unless the operator explicitly authorized autonomous simplification.
   A removal candidate survives only when its signal-key set is a subset of
   retained signal and its discriminating faults are still detected.
7. Call `read_file` for
   `kask/registry/templates/verification-compression/elimination.j2` and produce
   a candidate graph plus an explicit removed→retained signal mapping.
   No mapping means retain the artifact.

### CHECK — Prove graph preservation and run the experiment

8. Call `skill` for `lean-prover`, then call `read_file` for
   `kask/registry/templates/verification-compression/proof.j2`. Encode the
   finite preservation claim in Lean: every required signal present before is
   present after, and every removed artifact's signal set is covered by retained
   artifacts. Model each `SignalKey` as the complete expectation, falsifier,
   oracle-kind, failure-class, and provenance-tier tuple; an opaque ordinal is
   insufficient. Store proof files under `target/verification-compression/<run-id>/`.
   Run `lean` or `lake build` as the extrinsic oracle. Any `sorry`, missing
   toolchain, timeout, unsupported proposition, or compile error is
   `proof_unavailable`, never pass; no reduction may proceed.
9. Call `skill` for `kata-improvement` and execute one bounded experiment.
   Hold the contract, oracle inputs, toolchain, environment, cache state, harmful
   cases, and allowed-change control fixed. Run before/after timings in the
   declared `timing_scope`. If both states are selected, measure them separately;
   never substitute a warm sample for a cold claim. Re-run every discriminating fault from step 4 and
   require the allowed-change control to keep passing.
10. Call `read_file` for
    `kask/registry/templates/verification-compression/experiment.j2` and produce
    the result. Validate recorded evidence paths with `terminal` (`sha256sum -c`
    over the immutable receipt, plus raw before/after source and oracle hashes).
    Record `context.hashes_verified=true` only after that command succeeds;
    never let a model-supplied verdict fill it. Then call `lisp_eval` twice:
    - Preservation: `(and (= (assoc "missing_expectations" check) 0) (= (assoc "lost_falsifiers" check) 0) (= (assoc "lost_oracle_kinds" check) 0) (= (assoc "lost_failure_classes" check) 0) (= (assoc "downgraded_provenance" check) 0) (= (assoc "failed_harmful_cases" check) 0) (= (assoc "allowed_change_control_failures" check) 0) (eq (assoc "lean_proof_passed" check) t))`; env `{ "check": <preservation block> }`.
    - Context and samples: `(define equal-present (lambda (a b) (and (> (length a) 0) (member a (list b))))) (define valid-measured (lambda (s) (and (member "measured" (list (assoc "status" s))) (>= (length (assoc "timing_samples_before_ms" s)) 2) (>= (length (assoc "timing_samples_after_ms" s)) 2) (> (assoc "time_before_ms" s) 0) (> (assoc "time_after_ms" s) 0)))) (define valid-not-run (lambda (s) (and (member "not_run" (list (assoc "status" s))) (= (length (assoc "timing_samples_before_ms" s)) 0) (= (length (assoc "timing_samples_after_ms" s)) 0) (= (assoc "time_before_ms" s) 0) (= (assoc "time_after_ms" s) 0)))) (and (member (assoc "timing_scope" context) (list "cold" "warm" "both")) (equal-present (assoc "toolchain_before" context) (assoc "toolchain_after" context)) (equal-present (assoc "environment_before" context) (assoc "environment_after" context)) (equal-present (assoc "source_hash_before" context) (assoc "source_hash_after" context)) (equal-present (assoc "oracle_hash_before" context) (assoc "oracle_hash_after" context)) (eq (assoc "hashes_verified" context) t) (>= (length (assoc "evidence_paths" context)) 4) (if (member (assoc "timing_scope" context) (list "cold" "both")) (valid-measured (assoc "cold" metrics)) (valid-not-run (assoc "cold" metrics))) (if (member (assoc "timing_scope" context) (list "warm" "both")) (valid-measured (assoc "warm" metrics)) (valid-not-run (assoc "warm" metrics))))`; env `{ "context": <raw before/after identities, timing_scope, terminal-verified receipt>, "metrics": <cold/warm blocks> }`.
    - Both calls must return true. Ignore a contradictory
      `preservation.measurement_context_equal` summary field. Any failure
      rejects/reverts the candidate regardless of speedup.
11. Call `lisp_eval` for measured deltas. Compute workflow-graph and code-graph
    changes separately; a workflow-only reduction is **not** code-graph
    compression. Compute means directly from observed samples, never a
    model-supplied summary mean:
    - form: `(define sum (lambda (xs) (if (is_null xs) 0 (+ (car xs) (sum (cdr xs)))))) (define measured-speedup (lambda (s) (if (and (member "measured" (list (assoc "status" s))) (>= (length (assoc "timing_samples_before_ms" s)) 2) (>= (length (assoc "timing_samples_after_ms" s)) 2) (> (sum (assoc "timing_samples_after_ms" s)) 0)) (/ (/ (sum (assoc "timing_samples_before_ms" s)) (length (assoc "timing_samples_before_ms" s))) (/ (sum (assoc "timing_samples_after_ms" s)) (length (assoc "timing_samples_after_ms" s)))) nil))) (let ((wb (+ (assoc "nodes_before" metrics) (assoc "edges_before" metrics))) (wa (+ (assoc "nodes_after" metrics) (assoc "edges_after" metrics))) (cb (+ (assoc "code_nodes_before" metrics) (assoc "code_edges_before" metrics))) (ca (+ (assoc "code_nodes_after" metrics) (assoc "code_edges_after" metrics)))) (list (list "workflow_graph_compression" (if (= wb 0) nil (- 1 (/ wa wb)))) (list "code_graph_compression" (if (= cb 0) nil (- 1 (/ ca cb)))) (list "cold_speedup" (measured-speedup (assoc "cold" metrics))) (list "warm_speedup" (measured-speedup (assoc "warm" metrics)))))`
    - env: `{ "metrics": <measured workflow/code graph counts plus cold/warm samples; use code_nodes_before=0 and code_edges_before=0 when code graph was not measured> }`.
    - A missing or `nil` speedup or graph measure satisfies no target in
      that dimension. Never infer acceleration from fewer commands; use
      observed time. A no-edit pilot may demonstrate workflow compression
      but cannot claim production code-graph compression.

### ACT — Converge, retain, or revert

12. Accept a candidate only when the preservation gate is true and at least one
    caller-selected target is met. If preservation passes but targets miss,
    retain the result only with operator approval; otherwise revert.
13. Re-enter at step 4 for the next candidate, bounded by `max_cycles`. Stop
    early on zero delta, no discriminating test, or no safe candidate; report
    `essential_no_safe_reduction` rather than manufacturing work.
14. Delete superseded verification wrappers, scripts, fixtures, dependencies,
    comments, and transient plans in the same change. Update the canonical
    testing specification and expectation contracts.
15. Produce a report containing baseline and after graphs, timing samples,
    falsification log, essentialist decisions, Lean command/output, empirical
    fault results, retained signal mapping, deletions, and residual risks.

## Convergence

A cycle converges as one of:

- `compressed_preserved`: preservation gate true and a target met;
- `essential_no_safe_reduction`: no candidate survives all gates;
- `blocked`: contract, falsifier, Lean, or empirical oracle unavailable;
- `reverted`: a candidate lost signal.

Maximum three cycles. A third non-convergent result halts and returns the
measured remainder to the operator.

## Constraints

- Signal preservation is a hard gate; speed and graph size cannot compensate.
- Lean proves only the declared finite graph model. Empirical tests remain the
  authority for runtime behavior, failures, I/O, timing, and integration.
- No `sorry`, axioms introduced for convenience, or uncompiled proof text.
- Coverage and test counts are diagnostics, never quality targets.
- Keep a known harmful case and an allowed-change control fixed across runs;
  either failing blocks the candidate.
- Focused RED/GREEN runs are development evidence; do not repeat them in the
  final pipeline when the full fixed-oracle suite already executes the same
  test identities.
- Missing evidence is `unknown` or `blocked`, never zero or pass.
- Do not add compatibility shims for retired verification paths; delete them.
- All modifications require the program-manager definition of done and the
  canonical project gates.

## Registry Templates

| Template | Purpose |
|---|---|
| `verification-compression/inventory.j2` | Build the expectation-to-oracle verification graph and measured baseline. |
| `verification-compression/elimination.j2` | Synthesize falsifiability, architecture, and essentialist outputs into one safe reduction candidate. |
| `verification-compression/proof.j2` | Convert the removed→retained signal mapping into explicit Lean preservation obligations. |
| `verification-compression/experiment.j2` | Compare fixed-oracle before/after observations and issue the non-compensable verdict. |
