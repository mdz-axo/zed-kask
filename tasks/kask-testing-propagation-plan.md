# kask testing-protocol propagation plan — Phase 3

Status: **drafted, not executed.** Per the program charter (`tasks/enhanced-agent-task-kani-harness.md`), propagation is a separate, explicitly gated work item: each batch below starts only on operator confirmation. Ordering follows the Phase 0 gradient (loop-criticality × coverage gap; 2026-09-18 inventory: ~2,377 example tests, 5 property sites, 0 proofs at the time of inventory; 9/10 enumerated loops closed).

Anchors: layer assignment rules live in `kask/docs/reference/testing-protocol.md` (commit 84f0ee2321). The pilot precedent (hkask-types, commit 553f7d38bc) sets the per-batch done-condition template: scoped `./script/clippy` green, full crate tests green, falsifiable tests added, pathspec-limited commit hash cited, and any deleted test carries a replaced-by mapping.

## Batch 1 — hkask-mcp-companies: close the dropped forecast loop leg (loop-critical)

**Gap (Phase 0 loop inventory, loop 5, DROPPED LEG):** `forecast_persist` → `forecast_record` → read has no closure test. The tool surface exists (`src/tools/valuation.rs:1094-1307`, `src/forecast.rs:3-66`, `src/types.rs:328-379`); the compute leg is well value-tested in `hkask-forecast`; price resolution is unit-tested (`src/forecast.rs` tests). Nothing drives the persistence loop end-to-end. The crate: 159 tests, 0 property, 0 proof.

**Layer:** loop-closure — real tool seam over in-memory stores per the MCP testing standard (`kask/docs/reference/mcp-servers/README.md:72`), following the curator/portfolio `tool_behavior.rs` pattern.

**Work:**
1. `forecast_persist_records_and_reads_back` — persist, read back every field via the read tool.
2. `forecast_record_closes_the_loop` — persist → record outcome → read shows the outcome recorded (scoreable).
3. Degradation surfaced: `forecast_record` on an unknown forecast returns a typed error naming it (never a silent no-op); reading a symbol with no forecasts returns surfaced empty, never fabricated zeros.

**Acceptance (falsifiable):** the three tests pass; all existing companies tests green; `./script/clippy -p hkask-mcp-companies` green; commit hash cited. Falsifier: making `forecast_record` a silent no-op must fail tests 2-3. **If the closure test reveals a live bug in the loop, the bug is the deliverable** — report it, do not reshape the test around it.

**Effort:** small (one test file; the construction pattern is established).

## Batch 2 — hkask-forecast: property suite over the calibration math

**Gap:** 53 value-assert tests, 0 property sites. The math is exactly property-shaped (Phase 0 gradient rank 2).

**Layer:** property (proptest dev-dep; precedent: hkask-types @ 553f7d38bc).

**Properties (finalize signatures against the crate at execution):**
1. Brier penalty: quadratic and bounded for generated p in [0,1], o in {0,1}; multi-item mean exactness over generated vectors (nonempty, matching lengths).
2. Wilson bounds: bracket the observed rate; width weakly decreases as n grows at fixed rate.
3. Fermi aggregation: result stays within the [min, max] of the estimates; all-zero confidences → neutral prior (existing unit pin, generalized).
4. Isotonic apply: monotone non-decreasing, piecewise constant (existing unit pins, generalized).

**Acceptance:** at least 4 properties pass with declared domains and shrinking; no existing test deleted; clippy green; commit hash cited. Falsifier: any shrunk counterexample is a finding — report it, do not suppress it.

**Effort:** small-medium.

## Batch 3 — hkask-mcp-portfolio: ledger conservation properties

**Gap:** 59 tests, 0 property. The loop-closure layer is already this crate's strength (`create_apply_batch_seed_returns_materialize_loop`, attribution reconciliation, the `unwrap_or(0)` degradation pin) — do not touch it. The deficit is invariant coverage over generated transaction sequences.

**Layer:** property. Candidate invariants (finalize against the crate's math at execution): batch seed equals the sum of single seeds (holdings conservation); buy-then-sell nets quantities exactly with no orphans; weight_adjust preserves total value under fixed prices; daily returns reconcile to the period return (compounding conservation).

**Acceptance:** at least 3 properties pass; loop-closure tests untouched and green; clippy green; commit hash cited.

**Effort:** medium.

## Batch 4 — hkask-mcp-corpus chunk policies + hkask-mcp-media documented omission

**Gap:** corpus: 181 tests, 0 property (chunk policies, the Lisp query parser). media: 286 tests with a *documented intentionally omitted* proptest property (`tests/schema_compliance.rs:12`) — the informal omission register's only entry.

**Layer:** property. Corpus candidates: chunk word-budgets honored over generated text (min/max bounds, overlap preserved); `parse_lisp_query` never panics over generated inputs (well-formed parses; malformed surface typed errors — unit pins exist, generalize). Media: close the omission — add the deserialization-totality property the file documents as deliberately missing.

**Acceptance:** at least 3 corpus properties plus the media omission closed; all existing tests green; clippy green per crate; commit(s) cited.

**Effort:** medium.

## Batch 5 — generalize check-bounded-proofs.sh beyond the training crate

**Gap:** `kask/scripts/check-bounded-proofs.sh` hardcodes the hkask-mcp-training sources and five harnesses. The hkask-types pilot reproduced its discipline by hand (evidence: `~/.local/state/zed-kask/verification/hkask-types-pilot-2026-09-18/`).

**Layer:** protocol infrastructure (bash; project rule — no Python tooling).

**Work:** parameterize per crate (crate name + sources + harness list), keeping the pinned version gate (Kani 0.68.0), per-harness budgets (2 GiB address space / 120 s wall / unwind 12), evidence fields (logs, exits, started/finished, source sha256, manifest, SHA256SUMS), and the refuse-to-overwrite evidence directory. Default invocation stays training-compatible.

**Acceptance:** the script runs the hkask-types harnesses producing the same evidence fields as the pilot's manual run; shellcheck passes; the training invocation is unchanged; commit hash cited.

**Effort:** small-medium.

## Batch 6 — kask-core small property pins (pilot classification candidates)

From the exhaustive hkask-types classification delivered with the pilot, generalized to sibling modules:

1. `hkask-types/agent_paths`: sanitize property — generated names: no `/`, no `..`, traversal blocked; every sanitized name resolves under one root (unit pin `sanitize_name_blocks_path_traversal` exists — generalize).
2. `hkask-types/spreadsheet`: generated-value cell coordinates round-trip beyond the fixed examples (`cell_coordinate_round_trips_exactly` exists — generalize over ranges).
3. `hkask-types/ytdlp`: candidate retention is max-preserving over generated version lists (`equal_versions_retain_the_first_candidate` exists — generalize).
4. `hkask-lisp`: budget totality — generated programs: evaluation never panics and always terminates in result-or-budget-error (step/depth budget unit pins exist — generalize). **Proof layer explicitly not proposed here:** the step budget is far beyond the pinned unwind 12, so Kani coverage would be vacuous; the property layer is the correct assignment (the protocol's unprovability statement in action).

**Acceptance:** per crate: properties pass, existing tests green, clippy green, commit hash cited.

**Effort:** medium, splittable per crate.

## Gates, non-goals, scope

- **Gates:** each batch is a separately gated work item — the operator confirms the start; done-conditions cite commit hashes. No batch starts while a predecessor's gate is open.
- **Non-goals:** wholesale unit→property conversion (rejected by Phase 0 evidence); API changes; raising proof budgets; deletions without replaced-by mappings; weakening any check to make a target fit.
- **Out of scope pending operator ruling:** the 12 zed-side widget crates (282 tests, `crates/hkask-*`) — D-seam territory.
- **Home:** this plan lives in `tasks/` because `kask/docs/` is at 69/70 under the fewer-than-70 cap; promoting it into `kask/docs/plans/` requires an operator-approved fold or cap ruling.