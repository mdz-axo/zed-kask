---
title: "Spreadsheet MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-10-07
version: "0.40.3"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, lifecycle, trust]
---

# Spreadsheet MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-spreadsheet`
**Tools:** 2 — `spreadsheet_apply`, `spreadsheet_operation_get`
**Auto-start:** Yes by default with the full built-in set; `kask.mcp.load_default=false` disables the fleet and `kask.mcp.overrides.spreadsheet=false` disables this server.

The spreadsheet server is the **central mutation owner** of the LogiSheets-backed
spreadsheet capability (`kask/docs/plans/logisheets-spreadsheet-capability-plan.md`
§4): one server owns persisted spreadsheet edits so mutation behavior never
duplicates across analytical servers. Analytical servers remain producers of
domain data — they publish tables into immutable workbook revisions through
`hkask-spreadsheet` (`kask/crates/hkask-spreadsheet`); this server is the only
surface that persists spreadsheet mutations.

## Architecture

The LogiSheets engine `Workbook` is `!Send + !Sync` (Phase 0 admission
record), so `WorkbookService` runs it on a dedicated thread inside this
process: tool calls exchange `Send` commands with the actor and await
futures-oneshot responses. Nothing about the engine crosses an await.

Every mutation is applied against a **base revision**: the caller names the
base artifact + revision + content digest; the server verifies the digest
against the stored revision (a mismatch is a `failed_precondition`
conflict — never a silent overwrite of a stale base), applies the typed edits,
and publishes a **new immutable revision** beneath
`~/Documents/zk-data/spreadsheet-mcp/workbooks/`. Base revision files are
never rewritten; publication is atomic (temp + fsync + rename).

Spreadsheet edits modify derived workbook revisions only — no domain ledger,
cache, or journal is ever touched (the plan's authoritative-state boundary,
enforced architecturally: this crate links no domain server, and its writes
are pinned contained beneath the artifact root by
`writes_are_contained_beneath_the_artifact_root`).

## Tools

### `spreadsheet_apply`

Applies an [`EditTransaction`](../../plans/logisheets-spreadsheet-capability-plan.md)
against a base revision and returns the new workbook block as a
server-authoritative ` ```spreadsheet ` display hint (opaque artifact and
revision identity, content digest, bounded initial viewport, analytical
origin, and the server-authored mutation endpoint — plan §6).

- **Idempotent:** the same `idempotency_key` returns the recorded result.
- A key reused for a different base is `invalid_argument`.
- Unknown base revision → `not_found`; stale base digest → `failed_precondition`.
- Malformed edits (invalid coordinates, unparseable formulas, path-escaping
  ids) are rejected at the wire contracts before any filesystem access.

### `spreadsheet_operation_get`

Reconciles an interrupted mutation by its idempotency key (plan §7).
`status: "completed"` carries the recorded base and result revisions;
`status: "unknown"` means the outcome is **unknown** — the operation may or
may not have been applied. Callers must not blindly retry: re-open the base
revision and inspect state, or retry under a new idempotency key if
duplication is acceptable. The widget never auto-replays an operation whose
outcome is unknown.

## Configuration

No credentials; no database; no provider feeds. Reads only
`HKASK_ARTIFACTS_DIR` (the artifact root for workbook revisions, resolved via
`hkask_spreadsheet::artifact_store::production_root`). The allowlists are
pinned by `spreadsheet_allowlist_matches_actual_reads`
(`kask/crates/kask_bridge/src/mcp_servers.rs`).

## Formula support (probe-verified 2026-10-06)

The engine gates formulas at apply time (`check_formula`, `engine.rs`) and
surfaces unevaluable formulas as visible `#` error values — the spreadsheet
convention (pinned by `unsupported_formula_surfaces_as_error`). The
inventory below is **probe-verified** against the pinned `logisheets-rs
=1.15.1` by `kask/scripts/probe-spreadsheet-formula-inventory.sh` (a
scratch harness built outside the workspace; 96 candidate functions × 5
simple invocation shapes; a function is listed as supported when at least
one shape evaluates without an error value).

**Evaluates (82):** ABS, AND, AVERAGE, AVERAGEIF, AVERAGEIFS, CEILING,
COLUMN, CONCAT, CONCATENATE, COUNT, COUNTA, COUNTBLANK, COUNTIF, COUNTIFS,
DATE, DAY, EOMONTH, EXACT, EXP, FIND, FLOOR, FV, HLOOKUP, IF, IFERROR,
INDEX, INT, ISBLANK, ISERROR, ISNUMBER, ISTEXT, LARGE, LEFT, LEN, LN, LOG,
LOG10, LOWER, MATCH, MAX, MEDIAN, MID, MIN, MOD, MODE, MONTH, NOT, NPV,
OFFSET, OR, PERCENTILE, PMT, POWER, PV, RANK, RATE, REPLACE, RIGHT, ROUND,
ROUNDDOWN, ROUNDUP, ROW, SEARCH, SIGN, SMALL, SQRT, STDEV, STDEVP,
SUBSTITUTE, SUM, SUMIF, SUMIFS, SUMPRODUCT, TEXT, TRIM, TRUNC, UPPER,
VALUE, VAR, VARP, VLOOKUP, YEAR.

**Errors under every probed shape (14):** CORREL, FALSE, INDIRECT,
INTERCEPT, IRR, LOOKUP, NA, NOW, RAND, RANDBETWEEN, SLOPE, TODAY,
TRANSPOSE, TRUE. `TRUE`/`FALSE` are literals, not functions; the volatile
family (`TODAY`, `NOW`, `RAND`, `RANDBETWEEN`) and `INDIRECT`, `LOOKUP`,
`TRANSPOSE`, `NA` do not evaluate under the probed shapes.

Caveat: the probe classifies by five simple shapes — an arity-sensitive
function (e.g. `CORREL`, which wants two arrays) may be under-classified.
Re-run the probe before relying on a specific function, and treat an
unevaluable formula's visible `#` error value as the ground truth.

## Testing

`tests/tool_behavior.rs` drives both tools through their public
`Parameters<T>` seam over the real engine actor and a temp-dir artifact
root: immutable-revision creation with display hint, idempotent replay,
interrupted-operation reconciliation (completed and explicitly unknown),
error-kind specificity, write containment, and no-write-through staging.
`tool_names_match_live_router` pins the generated `TOOL_NAMES` set against
the live router (see the fleet [README](README.md) for the count
verification methods).

The property layer lives in the core crate
(`kask/crates/hkask-spreadsheet/src/property_tests.rs`, testing-protocol
layer 2): generated edit chains preserve the full immutable revision
history (every revision digest-matching and readable, base bytes never
changing, replay returning the recorded revision), and the three crash
windows of the apply ordering are pinned directly — an orphan revision
(published, unrecorded) reconciles as unknown and re-applies fresh;
a crash-leftover temp file is never readable as a revision; a same-id
rewrite is rejected with the original bytes unchanged.

## Redesign record (2026-10-06/07)

The capability was reviewed from scratch against its verified reference
models — the ratified plan (§5.1/§6/§7), DIVERGENCE.md D18, the portfolio
what-if precedent, and the engine's actual formula semantics — then rebuilt
per a twelve-proposal set (SP-01..SP-12). Each review phase ran as one
FINER-gated PDCA loop; the coverage table reconciled 54 rows (2 tools + 30
engine public items + 13 contract re-exports + 9 consumers,
`lisp_eval`-checked), and the fidelity phase closed with 16 findings
(F1–F16), every one cited at file:line. The review artifacts (review,
proposals, improvement plan) were consolidated into this reference on
2026-10-07 and live in git history (`b252d88b3d`..`87b8673285`).

**Proposal ledger** — every proposal landed; the code and its pins are the
successors:

- **SP-01** one owner for the display-hint fence and the error classifier;
**SP-03** the typed error kind across the ToolInvoker seam; **SP-02** the
interrupted save surfaces its reconciliation identity — commit
`1a8c500c12` (pins `mcp_kind_classifies_every_variant`,
`display_hint_round_trips_through_hint_body`,
`conflict_detection_is_structural_not_textual`).
- **SP-04** bounded LRU document residency with staged documents pinned;
**SP-11** metadata-first atomic publish; **SP-06** `expected_access`
enforced; **SP-07** the emitter-agnostic display-hint bullet; **SP-10**
the probe-verified formula inventory (above); **SP-12** the tautological
`access` fields deleted — commit `c9a5c3b855` (pins
`document_residency_is_bounded_and_staged_documents_are_pinned`,
`metadata_only_artifact_window_is_unknown_but_consistent`,
`apply_rejects_a_non_workbook_expected_access`).
- **SP-05** InlineTable completed end-to-end — the widget's bounded
read-only sortable table, `evaluate_evidence` the first producer —
commits `f66b136fe2` + `2bcf794dc8` (pin
`evaluate_evidence_inline_table_publishes_the_matrix`).
- **SP-09** the derived-ontology ruling: the six public-contract terms
(`AnalyticalTable`, `SpreadsheetBlock`, `SpreadsheetArtifact`,
`SpreadsheetViewport`, `EditTransaction`, `SpreadsheetError`) anchored on
Dublin Core, SUMO, and PKO in
`kask/crates/hkask-bridge-ontology/src/derived.rs` — commit `87b8673285`
(pins `spreadsheet_contract_terms_resolve_with_authority`,
`spreadsheet_contract_terms_resolve_on_the_derived_rung`).

**Execution record:** Remaining: none — every proposal in the set executed;
the per-slice execution record was consolidated here from the improvement
plan (2026-10-07).

**Live receipts:** 1035 tests green across the 15 touched crates; scoped
clippy + machete clean; `cargo check -p zed` clean; the 548-test
spreadsheet surface re-verified at `770b61c958`; the Lean
revision-invariant spec (`kask/lean/spreadsheet_revision_invariants.lean`
— five theorems plus helper, axioms `[propext]` only, negative control
failing as required) re-checks in CI via
`kask/scripts/check-lean-spec-pins.sh`. The execution goal (`11616a31`)
closed through the batch-record path — its durable record is this section
plus the commits (the curator-memory batch record
`kanban:goal-retention`/`batch_prune_2026-10-07_spreadsheet` cites this
location).