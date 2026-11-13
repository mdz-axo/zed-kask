---
name: verification-compression
description: "Compress and accelerate a software verification workflow without losing current expectation coverage, falsifiers, failure visibility, provenance strength, or fault-detection signal. First retires historical tombstones that lack a live disturbance channel; then builds a typed verification graph, composes kata-improvement, falsifiability, refactor-architecture, essentialist, and lean-prover, proves graph-preservation obligations in Lean, and accepts reductions only after fixed-oracle before/after experiments."
audience: platform
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
  signal-key field; `downgraded_provenance` a preservation-gate field) —
  anchored: `onto_anchor` → derived `scientific_evidence_ontology` (operator
  ruling 2026-09-28; Brush, Shefchek and Haendel, ICBO 2016; verified 2026-11-02).

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
   `lisp_eval` gate `VC-1` over the elimination render and the step-0 inventory,
   both as returned:
   - form: `(begin (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define strings (lambda (xs) (if (is_null xs) (list) (if (stringp (car xs)) (cons (car xs) (strings (cdr xs))) (strings (cdr xs)))))) (define ids (lambda (rows) (if (is_null rows) (list) (cons (text-of (car rows) "artifact_id") (ids (cdr rows)))))) (define unique (lambda (xs) (if (is_null xs) t (and (not (member (car xs) (cdr xs))) (unique (cdr xs)))))) (define not-in (lambda (xs ys) (if (is_null xs) (list) (if (member (car xs) ys) (not-in (cdr xs) ys) (cons (car xs) (not-in (cdr xs) ys)))))) (define removed (strings (assoc "removed_artifacts" candidate))) (define retained (strings (assoc "retained_artifacts" candidate))) (define merged (strings (assoc "merged_artifacts" candidate))) (define partition (append retained merged removed)) (define inventory (ids artifact_inventory)) (define rows (if (listp signal_mapping) signal_mapping (list))) (define bad-row (lambda (row) (not (and (string= (text-of row "status") "covered") (member (text-of row "removed_artifact") removed) (member (text-of row "retained_artifact") retained))))) (define unpreserved (lambda (rs) (if (is_null rs) (list) (if (bad-row (car rs)) (cons (text-of (car rs) "removed_artifact") (unpreserved (cdr rs))) (unpreserved (cdr rs)))))) (define unmapped (lambda (rs) (if (is_null rs) (list) (if (is_null (keep-rows (car rs) rows)) (cons (car rs) (unmapped (cdr rs))) (unmapped (cdr rs)))))) (define keep-rows (lambda (id rs) (if (is_null rs) (list) (if (string= (text-of (car rs) "removed_artifact") id) (cons (car rs) (keep-rows id (cdr rs))) (keep-rows id (cdr rs)))))) (define problems (append (if (> (length inventory) 0) (list) (list "empty_inventory")) (if (unique partition) (list) (list "artifact_in_two_classes")) (if (is_null (not-in inventory partition)) (list) (list "unclassified_artifacts")) (if (is_null (not-in partition inventory)) (list) (list "artifacts_outside_inventory")) (if (is_null (unpreserved rows)) (list) (list "unpreserved_signals")) (if (is_null (unmapped removed)) (list) (list "removed_without_mapping")))) (list (list "verdict" (if (is_null problems) "pass" "blocked")) (list "problems" problems) (list "unclassified" (not-in inventory partition)) (list "outside_inventory" (not-in partition inventory)) (list "unpreserved" (unpreserved rows)) (list "removed_without_mapping" (unmapped removed)) (list "counts" (list (list "inventory" (length inventory)) (list "retained" (length retained)) (list "merged" (length merged)) (list "removed" (length removed))))))`
   - env: `{ "artifact_inventory": <the step-0 selected artifact rows>, "candidate": <the render's candidate object>, "signal_mapping": <the render's signal_mapping array> }`
   - `pass`: every inventory artifact is in exactly one of `retained_artifacts`, `merged_artifacts`, `removed_artifacts`; every removed artifact has at least one mapping row; every mapping row is `covered`, names a removed artifact and a retained one. `blocked` lists `problems` (`empty_inventory`, `artifact_in_two_classes`, `unclassified_artifacts`, `artifacts_outside_inventory`, `unpreserved_signals`, `removed_without_mapping`) with the artifacts behind each; return to the render. The form checks the mapping's shape and completeness; whether a mapped retained artifact really carries the signal is step 8's proof and step 9's fault re-run.

### CHECK — Prove graph preservation and run the experiment

8. Call `skill` for `lean-prover`. From the ONE
   observed inventory and candidate, publish two graph JSON artifacts: the
   before graph and a candidate graph. Write this run's graphs, proof, logs
   and `manifest.json` under the artifacts route
   `~/Documents/zk-data/skills/verification-compression/{date}-{run}/`,
   where `{run}` is a readable subject slug naming the workflow under
   compression — never a UUID, hash or random run id (the
   standardized-artifact-storage Readable names rule, as `skill-maintenance`
   enforces) — via `terminal` (the built-in file tools are project-confined),
   never under `/tmp` or the project tree. Each uses the current schema:
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
   never substitute a warm sample for a cold claim. Re-run every discriminating fault from step 4
   and require the allowed-change control to keep passing; their outcomes
   enter the experiment render's `preservation` block
   (`failed_harmful_cases`, `allowed_change_control_failures`) and are gated
   there in step 10 — there is no separate count to type here, and speed is
   never a preservation condition.
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
    - env: `{ "metrics": <the render's metrics object as returned>, "context": <the same verified receipt context from step 10> }` — the form reads `code_graph_status` itself; do not zero or edit fields by hand.
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

`bash kask/scripts/check-skill-lisp-forms.sh` evaluates `VC-1`
(`scientific_gate_vc_1_`) over the authored form: a three-artifact
inventory with one covered removal (pass); an artifact left out of every
class; one in two classes; a mapping row `missing`; a removed artifact with
no mapping row; a mapping to a retained artifact outside the candidate; a
removal outside the inventory; an all-retained candidate (pass). Run the
other pinned `lisp_eval` forms (VC-0 relevance, inventory gate, preservation
gate, context/samples gate, deltas) through the live tool with representative
stub envs, BOTH directions per gate, whenever a form changes. The inline
forms are the authority for expected outcomes — this section does not
restate them. Stub the discriminating cases the compiled pins do not
cover:

- **VC-0**: one-to-one complete current rows, one-to-one fired tombstones,
  mixed rows, and malformed rows (missing/duplicate/extra IDs, wrong-typed or
  missing references, an unfilled placeholder, a current signal with a fired
  trigger, a tombstone whose trigger is not fired), with `max_steps: 1000000`.
  The typed-reference predicate and its invisible-leader bypass classes are
  pinned compiled in `kask/crates/hkask-lisp/src/bin/check_skill_lisp_forms.rs`
  (`vc0_typed_predicate_admits_content_and_rejects_invisible_leaders`) —
  re-stub only the branches that pin does not cover.
- **Tombstone branch**: pair a source-text absence test with no live channel
  but a durable decision record against a stale-persisted-input negative
  test that must stay `current_signal` and cannot enter the fast path.

The Lean proof pipeline (`generate-verification-preservation-proof.sh` +
the pinned Lean binary; `test-generate-verification-preservation-proof.sh`,
1 positive + 11 negative cases) and the receipt checker
(`check-verification-compression-receipt.sh`, pinned by
`check-verification-compression-receipt-selftest.sh` controls) are
script-level regression surfaces. They need the pinned toolchain and are a
recorded scope boundary, not run here.
