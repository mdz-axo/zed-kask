---
name: verification-compression
description: "Compress and accelerate a software verification workflow without losing current expectation coverage, falsifiers, failure visibility, provenance strength, or fault-detection signal. First retires historical tombstones that lack a live disturbance channel; then builds a typed verification graph, composes kata-improvement, falsifiability, refactor-architecture, essentialist, and lean-prover, proves graph-preservation obligations in Lean, and accepts reductions only after fixed-oracle before/after experiments."
---

# Verification Compression

Reduce the cost and code graph of verification while preserving what the
verification can actually distinguish. The unit of preservation is not a test
count or coverage percentage. It is a **signal key**:

`(expectation_id, falsifier_id, oracle_kind, failure_class, provenance_tier)`.

A check is redundant only when every signal key it carries is supplied by a
retained check and representative harmful changes remain detectable. Before
that preservation analysis, a relevance gate separates current signals from
historical tombstones: an existing test is not automatically a requirement.

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
- Ontology: SEPIO grounds evidence and provenance (`provenance_tier` is a
  signal-key field; `downgraded_provenance` a preservation-gate field).

## D/P labelling

Typing relevance status, verification graphs, expectations, falsifiers, oracle
kinds and signal keys is P — judgment over the workflow's structure, critiqued
by the authorized contract, falsifiability and essentialist deletion tests. The
VC-0 arithmetic and execution evidence are D. A tombstone retirement is accepted
only when the decision record/authority are present, the artifact and stale
references are gone, and the owning build plus retained tests pass. For current-
signal compression, the stronger existing path remains: Lean graph-preservation
obligations compile or fail, and the fixed-oracle before/after experiment must
show representative harmful changes remain detectable. A current-signal
reduction without that experiment is a proposal, not accepted compression.

## Initial and target condition

- **Initial condition:** the selected executable verification artifacts, their
  authorized expectation contracts (if current), candidate retirement rulings
  (if historical), live disturbance channels, oracle boundaries, invocation
  graph, and measured cost only for dimensions the caller intends to claim.
- **Target condition:** every artifact is classified; every current signal
  retains authorized expectation/falsifier/oracle/channel/action/return-path
  coverage; every tombstone's authority survives outside the executable suite;
  and the cycle terminates in one named Convergence state within `max_cycles`.

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
- A historical tombstone test has no current expectation signal but still
  executes or links on every check; retire it through the VC-0 fast path below,
  preserving its decision authority outside the executable suite.
- A proposed refactor needs evidence that removed checks carried no unique
  signal.

## When NOT to Use

- The product behavior is currently broken or undiagnosed; use `diagnose`.
- No expectation contract or harmful counterexample exists **and** the artifact
  is not a classified historical tombstone with a durable removal ruling.
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
- `sample_count`: at least 2 comparable timed runs per **claimed** timing state;
  3 is preferred. Tombstone retirement needs no timing experiment unless the
  report claims speed/resource improvement;
- `timing_scope`: `cold`, `warm`, `both`, or `not_run`; select before measuring.
  `not_run` earns no speedup claim;
- `compression_target` and `speed_target`: caller-selected, never hidden;
- `max_cycles`: 1–3, default 2.

`execute` mode requires explicit operator approval. `analyze` mode never edits
verification code.

## Instructions

### PLAN — Direction and current condition

0. Run the semantic/cybernetic relevance classification from
   `kask/docs/reference/testing-protocol.md` before treating existing artifacts
   as required signals. First read and render
   `kask/registry/templates/verification-compression/inventory.j2` with the
   selected artifacts, authorized contract, known expectations, and any timing
   observations (an empty timing array is valid when no speed claim is made).
   Its `relevance_classification` rows are the evidence input to VC-0; no
   separately model-authored counters or disposition are accepted. For every
   test/check/proof classify:
   - the current authorized expectation and falsifier;
   - the independently observed product boundary;
   - the live disturbance channel that can still produce the harmful state;
   - the corrective action and return path;
   - the retirement trigger and its observed status (`pending` or `fired`); and
   - for a tombstone, the durable non-executing decision record and retirement
     authority.

   Keep negative tests whose disturbance channel is mutable registry/config,
   stale or persisted input, untrusted input, generated output, provider or
   transport behavior, migration/cleanup, or upstream/rebase. A source-text
   absence check with no **supported or automated** producer beyond deliberately
   typing a retired name is not a current signal. Source-structure tests remain
   current when upstream/rebase, code generation, or a repeated production
   pattern is the live disturbance channel. A current signal whose retirement
   trigger has fired must be reclassified in this same run: retain it only if
   another live channel/current expectation remains; otherwise classify it as a
   tombstone. Classify only the unsupported case as `tombstone` and retire it
   after the ruling is preserved outside the executable suite. If any artifact
   is unclassified, or a claimed current signal lacks a live
   expectation/falsifier/channel/oracle/action/return path/retirement trigger,
   stop; do not launder it into the required set.

   Derive the branch mechanically from the rows and the exact selected artifact
   inventory with VC-0. Every reference-bearing value uses a typed, machine-safe
   nonblank string (`artifact:`, `expectation:`, `authority:`, `falsifier:`,
   `boundary:`, `channel:`, `correction:`, `return:`, `trigger:`, `record:`).
   The suffix starts immediately after the prefix and its first character is
   admissible (no leading whitespace or invisible character — the closed
   allowlist enforced by the `boundary-ok` builtin, not a space/tab/newline
   blocklist; the blocklist passed CR, VT, FF, NBSP and ZWSP, live 2026-10-18).
   It contains no template angle brackets, so an unfilled placeholder,
   whitespace, or wrong JSON type cannot masquerade as evidence:

   ```lisp
   (begin
     (define typed (lambda (prefix value) (and (stringp value) (starts-with prefix value) (> (length value) (length prefix)) (boundary-ok prefix value) (not (string-contains "<" value)) (not (string-contains ">" value)))))
     (define blank (lambda (value) (or (is_null value) (and (stringp value) (= (length value) 0)))))
     (define status-is (lambda (row status) (and (stringp (assoc "status" row)) (string= (assoc "status" row) status))))
     (define current-row (lambda (row) (and (status-is row "current_signal") (typed "artifact:" (assoc "artifact_id" row)) (typed "expectation:" (assoc "expectation_id" row)) (typed "authority:" (assoc "expectation_authority" row)) (typed "falsifier:" (assoc "falsifier_id" row)) (typed "boundary:" (assoc "observed_boundary" row)) (typed "channel:" (assoc "live_disturbance_channel" row)) (typed "correction:" (assoc "corrective_action" row)) (typed "return:" (assoc "return_path" row)) (typed "trigger:" (assoc "retirement_trigger" row)) (string= (assoc "retirement_trigger_status" row) "pending") (blank (assoc "decision_record" row)) (blank (assoc "retirement_authority" row)))))
     (define tombstone-row (lambda (row) (and (status-is row "tombstone") (typed "artifact:" (assoc "artifact_id" row)) (blank (assoc "expectation_id" row)) (blank (assoc "expectation_authority" row)) (blank (assoc "falsifier_id" row)) (blank (assoc "observed_boundary" row)) (blank (assoc "live_disturbance_channel" row)) (blank (assoc "corrective_action" row)) (blank (assoc "return_path" row)) (typed "trigger:" (assoc "retirement_trigger" row)) (string= (assoc "retirement_trigger_status" row) "fired") (typed "record:" (assoc "decision_record" row)) (typed "authority:" (assoc "retirement_authority" row)))))
     (define valid-row (lambda (row) (or (current-row row) (tombstone-row row))))
     (define count-invalid (lambda (rows) (if (is_null rows) 0 (+ (if (valid-row (car rows)) 0 1) (count-invalid (cdr rows))))))
     (define count-status (lambda (rows status) (if (is_null rows) 0 (+ (if (status-is (car rows) status) 1 0) (count-status (cdr rows) status)))))
     (define ids (lambda (rows) (if (is_null rows) nil (cons (assoc "artifact_id" (car rows)) (ids (cdr rows))))))
     (define unique (lambda (xs) (if (is_null xs) t (and (not (member (car xs) (cdr xs))) (unique (cdr xs))))))
     (define all-in (lambda (xs ys) (if (is_null xs) t (and (member (car xs) ys) (all-in (cdr xs) ys)))))
     (let ((expected (ids artifact_inventory)) (actual (ids relevance_classification)) (invalid (count-invalid relevance_classification)) (current (count-status relevance_classification "current_signal")) (tombstones (count-status relevance_classification "tombstone")))
       (let ((bijection (and (> (length expected) 0) (= (length expected) (length actual)) (unique expected) (unique actual) (all-in expected actual) (all-in actual expected))))
         (let ((gate (and bijection (= invalid 0))))
           (list (list "gate_passed" gate) (list "row_bijection" bijection) (list "invalid_rows" invalid) (list "disposition" (if gate (cond ((= current 0) "tombstone_fast_path") ((= tombstones 0) "full_preservation") (t "partition_required")) "blocked")))))))
   ```

   Env: `{ "artifact_inventory": <selected artifact rows>,
   "relevance_classification": <inventory output rows> }`; call `lisp_eval`
   with `max_steps: 1000000` and default `max_depth`. `blocked` returns to
   classification. `partition_required` splits the selection into tombstone and
   current-signal inventories and reruns step 0 on each; it never sends
   zero-signal tombstones through the current-signal proof path.
   `full_preservation` admits only current signals to `required_expectations`;
   `tombstone_fast_path` admits no invented signal keys.

     **Tombstone fast path:** when every selected artifact is a tombstone and each
     has a durable decision record, do not manufacture a harmful mutation, signal
     mapping, Lean proof, or before/after oracle for an empty current-signal set.
     Preserve/move the ruling, delete the executable artifact plus stale
     references/fixtures/dependencies, run the owning package's build and relevant
     retained suite, and sweep the repository for deleted identifiers/paths. Record
     removed test binaries and workflow nodes/edges. Report speed/resource change
     only from observed comparable samples; otherwise mark it `not_measured`.
     Converge as `tombstone_retired`. Mixed selections are partitioned by VC-0;
     only the current-signal partition executes the full steps 1–15. For a
     tombstone-only selection, execute this fast path and jump directly to
     Convergence/reporting; steps 1–13's graph proof, mutation and timing
     experiment are not applicable.

1. Call `skill` for `kata-improvement` only on the `full_preservation` path.
   Execute its direction and current-
   condition steps over the target. Measure rather than estimate:
   - verification-workflow command/test/script/fixture/proof/checker nodes;
   - production code crate/module/function/type nodes in the chosen scope;
   - `invokes`, `depends_on`, `verifies`, `detects`, `duplicates`, and
     production `calls` / `uses` edges, with file:line evidence;
   - wall time, toolchain, cache state, environment, and sample count;
   - every signal key and its producing artifact.
2. Reuse the inventory rendered in step 0. On `full_preservation`, enrich its
   `measurement_context`, verification graph and timing observations after
   measurement; re-render if those inputs changed. On `tombstone_fast_path`,
   the step-0 classification receipt is the inventory receipt and no graph
   proof rows are invented. Only `current_signal` rows enter `signal_keys`; every
   `tombstone` row names its durable decision record, retirement authority,
   retirement trigger and fired status.
3. Call `lisp_eval` to ensure every required expectation has at least one
   baseline signal:
   - form: `(and (= (assoc "missing_expectations" gate) 0) (= (assoc "unfalsifiable_expectations" gate) 0))`
   - env: `{ "gate": <inventory coverage gate> }`
   - A false result blocks compression and returns to step 1.

### DO — Falsify, architect, and eliminate

4. Call `skill` for `falsifiability`. For `current_signal` artifacts, test the
   claim “artifact X carries unique verification signal.” Generate at least
   these alternatives: unique signal, duplicate signal, weak/oracle-substitution
   signal, and orchestration-only overhead. Design discriminating harmful
   changes or fault injections; plain coverage does not discriminate. A VC-0
   tombstone has no current falsifier by definition and uses the fast path; do
   not invent a mutation solely to justify deleting it.
5. Call `skill` for `refactor-architecture`. Explore the verification graph,
   rank deepening candidates, audit duplicated operations, and select one
   candidate only. Do not refactor multiple domains in one cycle.
6. Call `skill` for `essentialist`. Run Exist → Surface → Contract in advisory
   mode unless the operator explicitly authorized autonomous simplification.
   A current-signal removal candidate survives only when its signal-key set is a
   subset of retained signal and its discriminating faults are still detected.
   A tombstone fails EXIST because no current behavior is lost; its historical
   rationale must already survive in the named decision record.
7. Call `read_file` for
   `kask/registry/templates/verification-compression/elimination.j2` and produce
   a candidate graph plus an explicit removed→retained signal mapping.
   No mapping means retain the artifact. The candidate graph must enumerate
   every retained artifact's full five-field signal rows, not just totals.
   In `execute` record the approved source scope and exact candidate diff;
   in `analyze` source snapshots must remain identical. Then run deterministic
   `lisp_eval` gate `VC-1`:
   - form: `(and (= total_steps (+ (length keep_ids) (length compress_ids) (length delete_ids))) (= (length unbacked_expectations) 0) (= (length unpreserved_falsifiers) 0))`
   - env: `{ "total_steps": <count>, "keep_ids": <retained ids>, "compress_ids": <merged/compressed ids>, "delete_ids": <removed ids>, "unbacked_expectations": <list>, "unpreserved_falsifiers": <list> }`

### CHECK — Prove graph preservation and run the experiment

8. Call `skill` for `lean-prover`. From the ONE
   observed inventory and candidate, publish two graph JSON artifacts: the
   before graph and a candidate graph. Write this run's graphs, proof, logs
   and `manifest.json` under the artifacts route
   `~/Documents/zk-data/skills/verification-compression/{date}-{run}/`
   (via `terminal`; the built-in file tools are project-confined), never
   under `/tmp` or the project tree. Each uses the current schema:
   `schema_version:1`, `required` (all distinct baseline signal keys),
   `before` and `after` (artifact_id + full five-field signal), and
   `removed_mappings` (removed_artifact + retained_artifact + same signal).
   The candidate graph keeps the same `required` and `before` arrays as the
   before graph. Do not select a convenient key subset. Run the deterministic
   script `kask/scripts/audit/generate-verification-preservation-proof.sh`
   (`bash kask/scripts/audit/generate-verification-preservation-proof.sh
   <candidate-graph.json> <new-proof.lean>`): it rejects incomplete keys,
   missing mappings, and lost signals and deterministically emits the only
   admissible Lean instance directly without rendering a template. Never
   hand-author or edit the resulting proof. Check the generated `.lean` file
   via `lean_check` (or the configured pinned Lean binary, e.g. a wrapper
   running `lean +leanprover/lean4:v4.34.0`) and preserve exit code,
   stdout/stderr, version, graph hash, and proof hash. Run
   `bash kask/scripts/test-generate-verification-preservation-proof.sh
   <lean-binary>` to check both the valid graph and a missing-signal proof
   fail case. Any `sorry`, missing toolchain, timeout, graph rejection,
   failed negative control, or compile error is `proof_unavailable`, never a
   passed reduction. Lean proves only finite declared coverage; empirical
   oracle quality remains a separate gate.
9. Call `skill` for `kata-improvement` and execute one bounded experiment.
   Hold the contract, oracle inputs, toolchain, environment, cache state, harmful
   cases, and allowed-change control fixed. `analyze` requires identical source
   hashes; `execute` permits differing before/after source snapshots ONLY when
   their complete scope and computed diff match the operator-authorized change.
   A changed source is not a changed oracle. Run before/after timings in the
   declared `timing_scope`. If both states are selected, measure them separately;
   never substitute a warm sample for a cold claim. Re-run every discriminating fault from step 4,
   require the allowed-change control to keep passing, and run deterministic `lisp_eval` gate `VC-2` at `experiment.j2`:
   - form: `(and (= baseline_oracle_matches proposed_oracle_matches) (= regressions_detected 0) (= seeded_faults_caught baseline_faults_caught) (< proposed_duration_ms baseline_duration_ms))`
   - env: `{ "baseline_oracle_matches": <count>, "proposed_oracle_matches": <count>, "regressions_detected": <count>, "seeded_faults_caught": <count>, "baseline_faults_caught": <count>, "proposed_duration_ms": <ms>, "baseline_duration_ms": <ms> }`
10. Call `read_file` for
    `kask/registry/templates/verification-compression/experiment.j2` and produce
    the result. Capture the exact graph, all before/after source snapshots,
    contract, oracle, compiled proof source, and raw logs in a tab-delimited
    receipt: `role<TAB>phase<TAB>id<TAB>sha256<TAB>absolute_canonical_path`.
    Roles: graph, source, contract, oracle, proof, log, timing, candidate_diff;
    `proof` and `candidate_diff` are after-only, the others have before/after.
    Each `timing` file is the trusted command runner's JSON output with a
    selected `scope` and cold/warm `status` plus integer-millisecond
    `samples_ms`; never synthesize durations from a narrative log.
    Give each source snapshot pair the same unique ID; hash the exact authorized
    diff with `diff -u --label source/ID --label source/ID BEFORE AFTER`,
    concatenated by byte-sorted source ID. Capture the receipt SHA-256 from a
    trusted operator-controlled snapshot BEFORE accepting candidate output;
    a candidate-supplied manifest and candidate-supplied pin are NOT independent
    evidence. In `execute`, obtain the SHA-256 of the exact proposed source diff
    from the operator's explicit approval BEFORE applying the candidate;
    never derive approval from the candidate, its receipt, or this agent's own
    judgment. Run `bash kask/scripts/audit/check-verification-compression-receipt.sh
    MODE RECEIPT TRUSTED_PIN APPROVED_DIFF_SHA256_OR_DASH` against immutable
    snapshots outside candidate write access (`analyze` passes `-`). The checker
    re-derives the diff from source snapshots and compares it to that
    independent approved digest; no approval digest blocks `execute`. It also
    checks the pinned graph, regenerates the proof, and validates the pinned
    per-sample timings. Its JSON response is the ONLY source for `mode`,
    `authorized_diff_verified`, `source_changed`, selected timing scope, and
    `metrics.cold|warm.timing_samples_before_ms|after_ms`; never copy
    model-supplied booleans or arrays into the gate. Derive summary times
    from those arrays. Record `context.hashes_verified=true` only after
    the checker succeeds.
    A file changed
    after checking requires re-verification; no attacker with access to both
    trusted pin and snapshots is covered by this boundary. Then call `lisp_eval`: 
    - Preservation: `(and (= (assoc "missing_expectations" check) 0) (= (assoc "lost_falsifiers" check) 0) (= (assoc "lost_oracle_kinds" check) 0) (= (assoc "lost_failure_classes" check) 0) (= (assoc "downgraded_provenance" check) 0) (= (assoc "failed_harmful_cases" check) 0) (= (assoc "allowed_change_control_failures" check) 0) (eq (assoc "lean_proof_passed" check) t))`; env `{ "check": <preservation block> }`.
    - Context and samples: `(define positive (lambda (xs) (if (is_null xs) t (and (> (car xs) 0) (positive (cdr xs)))))) (define same (lambda (a b) (and (> (length a) 0) (member a (list b))))) (define measured (lambda (s) (and (member "measured" (list (assoc "status" s))) (>= (length (assoc "timing_samples_before_ms" s)) 2) (>= (length (assoc "timing_samples_after_ms" s)) 2) (positive (assoc "timing_samples_before_ms" s)) (positive (assoc "timing_samples_after_ms" s)) (> (assoc "time_before_ms" s) 0) (> (assoc "time_after_ms" s) 0)))) (define idle (lambda (s) (and (member "not_run" (list (assoc "status" s))) (= (length (assoc "timing_samples_before_ms" s)) 0) (= (length (assoc "timing_samples_after_ms" s)) 0) (= (assoc "time_before_ms" s) 0) (= (assoc "time_after_ms" s) 0)))) (and (member (assoc "timing_scope" context) (list "cold" "warm" "both")) (same (assoc "toolchain_before" context) (assoc "toolchain_after" context)) (same (assoc "environment_before" context) (assoc "environment_after" context)) (same (assoc "oracle_hash_before" context) (assoc "oracle_hash_after" context)) (same (assoc "contract_hash_before" context) (assoc "contract_hash_after" context)) (or (and (member "analyze" (list (assoc "mode" context))) (same (assoc "source_hash_before" context) (assoc "source_hash_after" context))) (and (member "execute" (list (assoc "mode" context))) (eq (assoc "authorized_diff_verified" context) t))) (eq (assoc "hashes_verified" context) t) (if (member (assoc "timing_scope" context) (list "cold" "both")) (measured (assoc "cold" metrics)) (idle (assoc "cold" metrics))) (if (member (assoc "timing_scope" context) (list "warm" "both")) (measured (assoc "warm" metrics)) (idle (assoc "warm" metrics))) (or (and (member "measured" (list (assoc "code_graph_status" metrics))) (or (and (member "analyze" (list (assoc "mode" context))) (= (assoc "code_nodes_before" metrics) (assoc "code_nodes_after" metrics)) (= (assoc "code_edges_before" metrics) (assoc "code_edges_after" metrics))) (and (member "execute" (list (assoc "mode" context))) (eq (assoc "authorized_diff_verified" context) t) (eq (assoc "source_changed" context) t)))) (and (member "not_run" (list (assoc "code_graph_status" metrics))) (= (assoc "code_nodes_before" metrics) 0) (= (assoc "code_nodes_after" metrics) 0) (= (assoc "code_edges_before" metrics) 0) (= (assoc "code_edges_after" metrics) 0))))`; env `{ "context": <raw identities, mode, trusted checker result>, "metrics": <raw graph counts and cold/warm blocks> }`. `authorized_diff_verified` is set only from a successful checker run with an operator-approved diff, not from the model.
    - Both calls must return true. Ignore a contradictory
      `preservation.measurement_context_equal` summary field. Any failure
      rejects/reverts the candidate regardless of speedup.
11. Call `lisp_eval` for measured deltas. Compute workflow-graph and code-graph
    changes separately; a workflow-only reduction is **not** code-graph
    compression. Compute means directly from observed samples, never a
    model-supplied summary mean:
    - form: `(define sum (lambda (xs) (if (is_null xs) 0 (+ (car xs) (sum (cdr xs)))))) (define measured-speedup (lambda (s) (if (and (member "measured" (list (assoc "status" s))) (>= (length (assoc "timing_samples_before_ms" s)) 2) (>= (length (assoc "timing_samples_after_ms" s)) 2) (> (sum (assoc "timing_samples_after_ms" s)) 0)) (/ (/ (sum (assoc "timing_samples_before_ms" s)) (length (assoc "timing_samples_before_ms" s))) (/ (sum (assoc "timing_samples_after_ms" s)) (length (assoc "timing_samples_after_ms" s)))) nil))) (let ((wb (+ (assoc "nodes_before" metrics) (assoc "edges_before" metrics))) (wa (+ (assoc "nodes_after" metrics) (assoc "edges_after" metrics))) (cb (+ (assoc "code_nodes_before" metrics) (assoc "code_edges_before" metrics))) (ca (+ (assoc "code_nodes_after" metrics) (assoc "code_edges_after" metrics)))) (list (list "workflow_graph_compression" (if (= wb 0) nil (- 1 (/ wa wb)))) (list "code_graph_compression" (if (and (> cb 0) (member "measured" (list (assoc "code_graph_status" metrics))) (member "execute" (list (assoc "mode" context))) (eq (assoc "authorized_diff_verified" context) t) (eq (assoc "source_changed" context) t)) (- 1 (/ ca cb)) nil)) (list "cold_speedup" (measured-speedup (assoc "cold" metrics))) (list "warm_speedup" (measured-speedup (assoc "warm" metrics)))))`
    - env: `{ "metrics": <measured workflow/code graph counts plus cold/warm samples; use code_nodes_before=0 and code_edges_before=0 when code graph was not measured>, "context": <the same verified receipt context from step 10> }`.
    - A missing or `nil` speedup or graph measure satisfies no target in
      that dimension. Never infer acceleration from fewer commands; use
      observed time. A no-edit pilot may demonstrate workflow compression
      but cannot claim production code-graph compression.

### ACT — Converge, retain, or revert

12. Accept a candidate only when the preservation gate is true and at least one
    caller-selected target is met (`compressed_preserved`). If preservation
    passes but targets miss, retain the result only with operator approval
    (`retained_targets_missed`); otherwise revert (`reverted`). If the
    contract, falsifier, Lean binary, or empirical oracle was unavailable
    this cycle, report `blocked` — do not route an unavailable oracle through
    the revert path; `blocked` names the missing prerequisite, `reverted`
    names a candidate that was tested and failed.
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

- `tombstone_retired`: VC-0 classified the selected artifacts as tombstones,
  every ruling is preserved outside the executable suite, deletion/reference
  sweeps are clean, and the owning build/retained tests pass;
- `compressed_preserved`: preservation gate true and a target met;
- `retained_targets_missed`: preservation passed but no target met; retained
  only with operator approval;
- `essential_no_safe_reduction`: no candidate survives all gates;
- `blocked`: contract, falsifier, Lean, or empirical oracle unavailable —
  the missing prerequisite is named, not routed through revert;
- `reverted`: a candidate was tested and failed (lost signal, or targets
  missed without operator approval to retain).

Maximum three cycles. A third non-convergent result halts and returns the
measured remainder to the operator.

## Constraints

- Existing verification artifacts are candidates, not authority. Run VC-0
  before preservation; a historical tombstone with no live disturbance channel
  never becomes a required signal merely because it is already a test. Do not
  route an empty current-signal set through proof or fault injection—the
  tombstone fast path verifies decision-record transfer, cleanup, and retained
  suite health instead.
- Signal preservation is a hard gate for current signals; speed and graph size
  cannot compensate.
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
- The context/samples gate's toolchain and environment equality checks are
  model-recorded identities (P-supplied to the D form — the receipt checker
  does not re-derive them); the preservation gate's seven counts are
  model-tallied from the experiment template's preservation block (the
  checker validates graph/proof/timing/diff but re-derives none of the
  seven). Both are honest soft spots in the D chain, disclosed here rather
  than hidden.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `verification-compression/inventory.j2` | Build the expectation-to-oracle verification graph and measured baseline. |
| `verification-compression/elimination.j2` | Synthesize falsifiability, architecture, and essentialist outputs into one safe reduction candidate. |
| `verification-compression/experiment.j2` | Compare fixed-oracle before/after observations and issue the non-compensable verdict. |

To render a template, call the `render_template` tool with the template ref and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `inventory.j2`: `target`, `authorized_contract`, `required_expectations`, `artifact_inventory`, `timing_observations`
- `elimination.j2`: `inventory`, `falsification_result`, `architecture_result`, `essentialist_result`, `mode`
- `experiment.j2`: `baseline`, `candidate`, `lean_result`, `before_observations`, `after_observations`, `targets`, `mode`, `receipt`

## Regression case

Run the five pinned `lisp_eval` forms through the live tool with representative
stub envs, BOTH directions per gate:

- **Relevance/classification form**: run VC-0 with `max_steps: 1000000` over
  at least these branches: exact one-to-one complete current rows →
  `gate_passed=true` + `full_preservation`; exact one-to-one fired tombstone
  rows → `gate_passed=true` + `tombstone_fast_path`; mixed complete rows →
  `partition_required`; missing/duplicate/extra IDs, whitespace or wrong-typed
  references, an unsubstituted `<placeholder>`, missing authority/channel/trigger,
  a current signal with a fired trigger, or a tombstone whose trigger is not
  fired → `blocked`.

- **Tombstone branch**: a source-text absence test with no current expectation
  or live disturbance channel, plus a durable decision record, enters the fast
  path; deleting it and sweeping its path/identifier can reach
  `tombstone_retired` without invented signal mappings or a vacuous Lean proof.
  The paired control is a negative test over stale persisted input or a mutable
  registry: it remains `current_signal` and cannot enter the fast path.
- **Inventory gate**: true when `missing_expectations` and
  `unfalsifiable_expectations` are both 0; FALSE when either is nonzero.
- **Preservation gate**: true when all seven count fields are 0 and
  `lean_proof_passed` is true; FALSE when any count is nonzero (e.g.
  `lost_falsifiers=1`).
- **Context/samples gate**: true for an execute-mode env with matching
  toolchain/environment/oracle/contract hashes,
  `authorized_diff_verified=true`, `source_changed=true`,
  `hashes_verified=true`, a measured cold block with >=2 positive samples,
  an idle warm block, and a measured code-graph block
  (`code_graph_status="measured"` with nonzero code node/edge counts).
  Also true for an ANALYZE-mode env with identical source hashes and
  unchanged code-graph counts. FALSE when: analyze mode with DIFFERENT
  source hashes; execute mode with `source_changed=false` and a measured
  code graph; a cold after-sample containing 0 (per-sample positivity);
  or `hashes_verified=false`.
- **Deltas form**: `workflow_graph_compression`, `code_graph_compression`,
  `cold_speedup` from the sample means, `warm_speedup` null when not_run;
  `code_graph_compression` is null when `code_graph_status="not_run"`
  (a no-edit pilot cannot claim code-graph compression).

The Lean proof pipeline (`generate-verification-preservation-proof.sh` +
the pinned Lean binary) and the receipt checker are separate script-level
regression surfaces: `test-generate-verification-preservation-proof.sh`
(valid + missing-signal proof cases) and
`check-verification-compression-receipt-selftest.sh` (receipt controls).
They need the pinned toolchain and are a recorded scope boundary, not run
here.
