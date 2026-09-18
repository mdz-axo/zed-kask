---
title: "kask Testing Protocol: Expectation Contracts and Evidence"
audience: [developers, architects, agents, operators]
last_updated: 2026-09-18
version: "1.2.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, trust]
---

# kask Testing Protocol — Expectation Contracts and Evidence

This protocol starts with **what the user expects the product to accomplish in
a particular context**, not what the current implementation happens to do.
Its quality signal is whether it enables useful code evolution while detecting
violations of those expectations. More tests, proofs, or pinned constants are
not themselves improvement. This is the operator's direction of 2026-09-18.
It extends — does not replace — the MCP tool-behavior standard
(`kask/docs/reference/mcp-servers/README.md` §Testing standard), which already
governs the loop-closure layer's construction rules (real tool seam over
in-memory stores, error-envelope specificity).

The earlier Phase 0 inventory is a historical sample, not evidence that all
feedback loops are closed. In particular, reading `coverage_pct` or
`mutation_score` from a metrics file does not establish a completed evaluation
of the candidate (`kask/crates/hkask-regulation/src/sensor_provider.rs`,
`TestCoverageSensor::observe` at :232–277 and `MutationScoreSensor::observe`
at :381–426). Test-layer counts must not substitute for inspection of functional
outcomes and evidence.

## Expectation contract and change authority

For each migration, record these fields beside the tests or in the existing
domain plan; no new manifest or framework is required:

| Field | Required question |
| --- | --- |
| Context and expectation | Who expects what product outcome, under which conditions? |
| Functional role | Why does this behavior matter to that outcome? |
| Relevant variables | Which inputs, states, identities, timing, and failures can change the outcome? |
| Allowed variation | Which names, representations, algorithms, private calls, or ordering are incidental and may change? |
| Falsifier and oracle | What observation contradicts the expectation, and how is it measured independently of the implementation? |
| Authority | Which user decision establishes the contract, and is any curator delegation explicitly scoped? |

Only the user, or a curator explicitly delegated that responsibility by the
user, changes an expectation contract/invariant. Candidate generation cannot
grant itself that authority. Record scope and applicability of delegation;
ambiguous, expired, or revoked authority is not permission. This is a review
protocol, **not a claim that runtime delegation/promotion enforcement exists**.
Internal implementation changes preserving the contract do not require freezing
incidental structure. Obsolete tests may be deleted with a reason: either the
obligation survives elsewhere, or its retirement has the required authority.

Challenge both sides: would a harmful outcome fail, and would an equivalent or
better implementation still pass? Vary irrelevant inputs in metamorphic tests
where possible. Do not equate a copied production algorithm with an independent
oracle, or weaken generators/assumptions to hide counterexamples.

## Assignment procedure

1. State the expectation contract and its falsifier before choosing a technique.
2. Evaluate each layer below independently. Properties and proofs can support
   the same obligation; neither replaces tests of callers establishing its
   preconditions or consumers preserving its guarantees.
3. Use Kani on suitable production logic. Extract a decision core only when it
   improves the production boundary too; do not create a disconnected proof
   implementation or distort the product to accommodate a solver budget.
4. Keep a representative regression witness, then challenge it with generated
   cases, fault injection, or a controlled harmful mutation. Record survivors
   and unsupported cases, not just passing counts.

## The four layers

### 1. Unit / example tests

- **Belongs when:** the check is a specific known input-to-output behavior or
  a module-seam contract — a regression pin. Naming convention stays the
  existing one: sentence names that state the contract
  (`record_resolution_feeds_calibration_read`, `extracts_array_of_objects`).
- **Cannot prove:** behavior on unseen inputs; domain-wide invariants;
  absence of panics anywhere in a domain.

### 2. Property tests (proptest)

- **Belongs when:** the check is an invariant over an input domain — parser
  contracts, arithmetic conservation, normalization, serialization
  round-trips over generated values. Each property is stated as a falsifiable
  hypothesis with its declared input domain; counterexamples must shrink.
  Dependency: the workspace `proptest` (1.10.0, git-pinned) as a dev-dependency.
- **Cannot prove:** exhaustive coverage (proptest samples; a passing property
  is evidence, not proof). Generated tests can exercise cross-crate wiring and
  degradation when their fixtures actually include those capabilities.

### 3. Proof layer (Kani, pinned 0.68.0 / CBMC 6.11.0)

- **Belongs when:** the check is panic-freedom or a structural contract of an
  **allocation-free pure core** — no String formatting, no Vec allocation,
  no I/O. Harnesses live in `#[cfg(kani)] mod proofs` beside the code;
  `cfg(kani)` is already whitelisted in the workspace check-cfg list, and the
  crates.io `kani` crate is never added as a dependency
  (Gödel plan R2 mandate). Pinned budgets per harness, matching
  `kask/scripts/check-bounded-proofs.sh`: `--default-unwind 12`, 2 GiB
  address-space limit, 120-second wall limit, all default safety and
  unwinding checks on. Evidence: exit code, per-harness logs, source
  identity (sha256), manifest. Classify outcomes explicitly: a counterexample
  is a failure; resource exhaustion,
  unsupported analysis, or an incomplete run is `Unknown`, never a pass.
- **Pilot exclusions, not inherent Kani limitations:** allocation and formatting
  surfaces — the R2 record shows
  four harnesses over formatting code exhaust the 2 GiB budget; the extracted
  pure core verifies within it. Report input restrictions and unwind settings;
  retained unwinding assertions must succeed, rather than silently truncating
  reachable execution. No result establishes behavior outside its assumptions.

### 4. Loop-closure tests

- **Belongs when:** the check is a feedback loop closing (write → observe →
  recall) or a degraded path being surfaced — a degradation is asserted as a
  note/status/mode naming the reason, never as empty-equals-success.
  Construction follows the MCP testing standard: real tool seam with the
  capability under test. Use file-backed stores for reopen/recovery obligations
  and real child processes for transport failure; in-memory substitutes cannot
  establish those boundaries.
- **Cannot prove:** input-domain invariants or formal contracts.

## Worked example: the JSON extraction multi-layer pin

The `.rules` corpus-batching trap (an array-parsing drop that silently
discards all but the first object) is pinned at three layers in
`kask/crates/hkask-types/src/json_extract.rs` (example at :110, property
at :197, bounded harnesses at :240 and :255):

| Layer | Pin |
| --- | --- |
| Unit | `extracts_array_of_objects` — the exact regression case |
| Property | `array_with_preamble_extracts_the_full_array` — the generalized hypothesis over generated arrays and reasoning preambles |
| Proof | `balanced_result_is_container_delimited` + `balanced_scan_never_panics_on_valid_utf8` — the allocation-free scan core `find_balanced_json`, bounded symbolic inputs |

The fence-stripping wrapper (`strip_json_fences`, String allocation) stays
at the property and example layers by rule 3 of the procedure.

## Worked example: uncertain MCP effects

`lost_reply_does_not_replay_effect_across_restart` in
`kask/crates/hkask-mcp/tests/reconnect_integration.rs:152–206` exercises the expectation:
**a lost reply must not silently repeat my change**. The fixture appends request
arguments to a file before exiting without a reply
(`kask/crates/hkask-mcp/src/bin/mcp_test_fixture.rs:104–124`, `FIXTURE_EFFECTS_FILE`).
The oracle is the independently observed effect journal, not the runtime's
private connection maps or per-process counter. Two explicit operations across
child restarts under the same runtime must leave exactly those two effects,
each reported as unknown.
The contract permits changes to executor adapters, registry representation,
and reconnection implementation. It does not promise exactly-once execution
by arbitrary providers or reversal of remote effects after cancellation.

## Evidence-loop acceptance protocol

This is the acceptance standard, **not an automatic promotion system**.
The local boundary below implements one test-evidence path; the editor's
periodic coverage/mutation sensors are not wired to it. Each bounded experiment
must retain:

- The user expectation, authorized contract version, objective, baseline,
  candidate identity, and allowed implementation variation.
- Evaluator/test identities, toolchain/features/dependencies, seeds or replay
  traces, proof assumptions/bounds, raw logs, and actual measurement time.
- Completed/failed/unknown/not-run status, denominators, exclusions, resource
  use, and the disposition with its decision authority.

Hold the acceptance oracle fixed during candidate evaluation; changes to it
are separately reviewed. Preserve a known harmful case and an allowed-change
control so a passing suite demonstrates discrimination, not just compatibility
with the incumbent. Report operational improvement separately from safety
eligibility, test counts, annotation coverage, and proof success.

The local pilot rejects stale, wrong-candidate, incomplete, malformed, or
inconsistent reports; missing evidence cannot mean recovery. Its self-test runs
a real faulty Rust fixture, observes a corrective disposition, corrects the
implementation under an unchanged contract/oracle, and remeasures. It also
accepts an equivalent implementation. This demonstrates the local workflow,
not measured improvement in the production product. See the existing
[bounded improvement lifecycle](../plans/goedel-gap-closure-plan.md#r6--one-bounded-promotion-lifecycle)
for authority, activation, and recovery work that remains distinct from testing.

### Running the local evidence boundary

Build `check_test_evidence` from the trusted checkout, then invoke the runner
with absolute paths. Its interface is:

```text
bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/evaluate-test-evidence.sh CHECKER MANIFEST PACKAGE CONTRACT ORACLE_FILE NEW_OUTPUT WALL_SECONDS INPUT_FILE...
```

- `MANIFEST` is the workspace manifest with its adjacent lockfile; `PACKAGE`
  selects **library tests, default features, offline, serial**, without filters.
- `CONTRACT` is the user expectation document; `ORACLE_FILE` is the reviewed
  test source (or explicit oracle input inventory). Enumerate all relevant
  source/build/config inputs in `INPUT_FILE...`; identity covers that declared
  scope, **not an automatically discovered dependency closure**.
- The runner captures OS user/UID/host, run UUID, source/contract/oracle/checker
  hashes, command, toolchain, an environment fingerprint (not secret values),
  original timestamps, raw log and status. It refuses to overwrite evidence
  and checks input hashes again after execution
  (`kask/scripts/evaluate-test-evidence.sh:35–77`).
- The checker compares the report to the separately captured expected context,
  validates age/order, log digest and a single nonempty libtest summary, then
  writes `evaluation_passed` (exit 0), `corrective_work_required` (exit 1), or
  `evidence_rejected` (exit 2). Compilation failures and timeouts are not
  behavioral failures. Ignored tests remain explicit exclusions; passing does
  not establish complete expectation coverage
  (`kask/crates/hkask-regulation/src/bin/check_test_evidence.rs`, `assess`).
- `evaluate-test-evidence-selftest.sh CHECKER` exercises real failing/passing,
  equivalent-implementation, zero-test, timed-out, input-changing and compile-error
  Cargo runs, plus rejected cross-run, malformed, missing and altered-log evidence.
  The `kask-tests` CI job builds the checker and invokes this self-test
  (`.github/workflows/kask-invariants.yml`, “Test evidence” step); a local run is
  not evidence that remote CI executed.

**Trust and limits:** this is an operator-invoked Linux tool for trusted tests,
not a sandbox for hostile candidates. The invoking OS account is attributed,
not an authenticated curator delegation. Do not let a candidate modify expected
context or the checker. Pre/post hashes cannot detect transient changes restored
before the second hash; use an isolated immutable snapshot when that matters.
The wall limit covers Cargo execution, not the whole script; detached descendants
and external effects require separate containment and fixture teardown. There
is no memory/disk/process quota, automatic corrective edit, periodic-sensor
registration, coverage/mutation measurement, or promotion path.
Changing the oracle requires separate review and a new evaluator identity.

## Provenance

[^pyramid]: Ham Vocke, "The Practical Test Pyramid", 2020, https://martinfowler.com/articles/practical-test-pyramid.html — layered testing.
[^quickcheck]: Koen Claessen and John Hughes, "QuickCheck: A Lightweight Tool for Random Testing of Haskell Programs", ICFP 2000, https://doi.org/10.1145/351240.351266 — property-based testing with shrinking.
[^kani]: Kani contributors, "Kani Rust Verifier", https://github.com/model-checking/kani — bounded model checking.
[^goedel]: Gödel-machine gap closure plan, `kask/docs/plans/goedel-gap-closure-plan.md` §R2 — the allocation-surface cost record and pinned proof budgets.