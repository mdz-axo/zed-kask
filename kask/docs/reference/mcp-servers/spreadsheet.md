---
title: "Spreadsheet MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-09-18
version: "0.40.0"
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