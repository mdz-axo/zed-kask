# kask testing-protocol propagation — completion ledger and remaining batches

**Status:** partially implemented; reconciled against git and source, not a fresh verification run. No tests, Clippy, Kani, or evidence-runner self-tests were run for this documentation pass. “Implemented” below means committed code exists, not that current tests or remote CI are green.

**Governing acceptance:** [testing protocol](../kask/docs/reference/testing-protocol.md), especially expectation contracts and change authority. User-expected outcomes govern, not numeric test/property/proof quotas. Before each remaining batch, record context, functional role, relevant variables, allowed variation, independent oracle/falsifier, and user or explicitly delegated curator authority. Harmful behavior must fail; equivalent implementations must remain free to pass.

**Historical baseline only:** the 2026-09-18 Phase 0 inventory reported ~2,377 example tests, 5 property sites, 0 proofs and 9/10 enumerated loops closed. Its crate counts and the original “dropped leg” diagnosis are not current coverage measurements or evidence of complete functional closure.

## Actual completion ledger

| Slice | Implementation commits | Outcome covered in current source; limits |
| --- | --- | --- |
| Types pilot | `553f7d38bc` | JSON extraction properties plus Kani harnesses for panic freedom and container delimiters over symbolic **8-byte valid-UTF-8 inputs** (`kask/crates/hkask-types/src/json_extract.rs:186–269`). Not arbitrary-length proof, JSON validity, or proof of the allocating wrapper; not completion of Batch 6. |
| Batch 1 — companies forecast loop | `75aa28750c`; fallback extension `aed279f660` | Real tool calls persist → get/list and record → read back outcomes scored against stored probability; missing forecasts error and empty lists remain empty. Missing probability is disclosed and the 0.7 fallback is pinned (`kask/mcp-servers/hkask-mcp-companies/src/forecast_loop_tests.rs:35–275`). This is committed regression coverage, not a claim that every forecast outcome is covered. |
| Batch 2 — forecast calibration | `f137917dbf` | Generated Brier bounds/mean, Wilson bounds/narrowing, Fermi hull/neutral prior, isotonic monotonicity/bounds and empty-fit identity (`kask/crates/hkask-forecast/src/property_tests.rs:12–153`). The originally proposed piecewise-constant property is not in this suite. |
| Batch 3 — portfolio ledger | `6cd9ce8648` | Generated buy/sell cash and quantity conservation, non-cash CMP transactions, ledger recall, snapshot cash/shares and deposits excluded from returns (`kask/mcp-servers/hkask-mcp-portfolio/src/property_tests.rs:41–260`). Batch-seed equivalence and daily/period compounding were proposed, not implemented here. The recall fold reuses `position_delta`; it checks persistence consistency, not an independent oracle for that math. |
| Local test-evidence boundary | `23a73a44d4` | Runner binds declared source inputs, contract, oracle, evaluator and run identity to logs and pre/post hashes (`kask/scripts/evaluate-test-evidence.sh:35–77`). Checker distinguishes `evaluation_passed`, `corrective_work_required`, and `evidence_rejected` (`kask/crates/hkask-regulation/src/bin/check_test_evidence.rs:87–217`). Self-test covers failure → repair → remeasurement, equivalent implementation and invalid-evidence controls (`kask/scripts/evaluate-test-evidence-selftest.sh:17–103`); CI invokes it (`.github/workflows/kask-invariants.yml:137–140`). Wiring is not evidence of execution. |

The evidence runner selects offline, serial **library tests with default features**, not the full crate suite. It is a trusted local evaluation boundary, not hostile-candidate containment, authenticated curator delegation, automatic correction, coverage/mutation measurement, sensor registration, or promotion authority. It does **not** complete Batch 5's Kani-runner generalization; see the protocol's [runner scope and limits](../kask/docs/reference/testing-protocol.md#running-the-local-evidence-boundary).

## Remaining batches (operator-gated)

### Batch 4 — corpus chunk/query behavior and media deserialization

Define the user's chunk-budget/overlap expectations and valid/invalid query behavior before generating inputs. Ground candidates in `kask/mcp-servers/hkask-mcp-corpus/src/helpers.rs:395–431` and `kask/mcp-servers/hkask-mcp-corpus/src/tools/storage.rs:313–380`. For media, address the explicitly omitted deserialization-totality property (`kask/mcp-servers/hkask-mcp-media/tests/schema_compliance.rs:10–14`): accepted requests deserialize, malformed requests surface errors rather than panics. Preserve the distinct schema-compatibility obligation; adding properties is not itself closure.

### Batch 5 — generalize the bounded-proof runner

`kask/scripts/check-bounded-proofs.sh:19–37` still fixes training sources and harnesses. Parameterize crate, sources and harnesses while preserving the training default, pinned Kani 0.68.0, 2 GiB / 120 s / unwind 12 budgets, default safety/unwind checks, refusal to overwrite, source recheck and evidence fields (`kask/scripts/check-bounded-proofs.sh:10–62`). Acceptance requires actual types-pilot evidence and training-compatibility checks plus shellcheck results, not a generic test-runner receipt. The originally cited manual pilot directory (`~/.local/state/zed-kask/verification/hkask-types-pilot-2026-09-18/`) is a historical pointer, not revalidated here.

### Batch 6 — types sibling invariants and Lisp budgets

Candidates remain path confinement after sanitization, cell-coordinate round trips and max-preserving version selection. Existing example anchors: `kask/crates/hkask-types/src/agent_paths.rs:368`, `kask/crates/hkask-types/src/spreadsheet.rs:894`, `kask/crates/hkask-types/src/ytdlp.rs:100`. Define their domains from user expectations rather than freezing incidental representations.

For Lisp, test bounded generated programs for panic freedom and result-or-explicit-error behavior under declared step/depth budgets (`kask/crates/hkask-lisp/src/hkask_lisp.rs:505–527`). **Low Kani unwind is not inherently vacuous.** An insufficient unwind with unwinding assertions enabled fails to establish the proof; unreachable obligations or contradictory assumptions can cause vacuity. Assess bounded production decision cores independently, check reachability and retain safety/unwind checks. A small bounded proof may be useful but does not establish arbitrary-program termination; broader property tests and caller/consumer tests remain complementary.

### Follow-up review — implemented batches

Review the ledger's uncovered candidates (isotonic piecewise constancy, batch-seed equivalence, compounding) against actual user expectations. Implement justified obligations or record an authorized retirement; do not silently mark them done or add tests merely to reach the old quotas. Review oracle independence and allowed-change controls for existing suites before treating them as complete expectation coverage.

## Execution gates and scope

- Each remaining batch requires operator confirmation under `tasks/enhanced-agent-task-kani-harness.md`; re-check current code and evidence before starting. Resolve a predecessor's gate before proceeding unless the operator explicitly authorizes independent work.
- Close a batch with expectation/falsifier evidence, declared domains and exclusions, actual scoped Clippy and full affected-crate test results, and implementation commit references. Report failures, shrunk counterexamples and budget limits; never weaken checks to fit a target. Evidence-runner receipts cover only their declared scope.
- Do not freeze incidental internals or undertake wholesale unit-to-property conversion. Test deletion needs a preserved-obligation mapping or user/explicitly delegated curator authority to retire the obligation. API changes and raised proof budgets require separate scope approval.
- Zed-side `crates/hkask-*` widget work remains out of scope pending operator ruling. This ledger stays in `tasks/`; no new documentation is required.
