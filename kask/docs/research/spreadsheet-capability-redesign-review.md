---
title: "Spreadsheet Capability — From-Scratch Design Review (Plan and Coverage)"
audience: [operators, developers, architects]
last_updated: 2026-10-06
version: "0.1.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, lifecycle, trust]
kind: research
related:
  - spreadsheet-capability-redesign-proposals.md
  - spreadsheet-capability-redesign-improvement-plan.md
  - ../plans/logisheets-spreadsheet-capability-plan.md
  - ../../lean/spreadsheet_revision_invariants.lean
---

# Spreadsheet Capability — From-Scratch Design Review: Plan and Coverage

**Scope:** the spreadsheet MCP server (`kask/mcp-servers/hkask-mcp-spreadsheet`,
2 registered tools), the engine crate it wraps (`kask/crates/hkask-spreadsheet`),
the D18 widget (`crates/hkask-spreadsheet-widget`), the wire contract
(`kask/crates/hkask-types/src/spreadsheet.rs`), and every consumer discovered
empirically below. **License:** breaking changes permitted; every proposal
enumerates the path it replaces and what it deletes. **Deliverables are
documents only** — no implementation, no commits.

## Method

Each phase ran as one PDCA improvement-kata loop (hypothesis FINER-gated →
current condition grounded at file:line → target condition → experiment →
verification). Three goal tags apply throughout: **fidelity** (to the
capability plan §5.1/§6/§7 and the D18 contract), **engine-efficiency** (the
computation graph: formula evaluation, viewport extraction, revision
publication, digest/idempotency machinery), and **integration** (the other
built-in servers, the widget, and any skills found).

### Reference models (verified, not presumed)

| Model | Verification | Role in the review |
|---|---|---|
| The platform's own ratified plan — `kask/docs/plans/logisheets-spreadsheet-capability-plan.md` (v0.2.1, 2026-09-28) | Read in full (766 lines) during Phases 0–2 | **The primary reference**: §5.1 the deep module (incl. the hidden-complexity list), §6 presentation modes and block constraints, §7 artifact lifecycle, §8 widget, §10 phase records, §11 verification requirements, §13 refused shortcuts |
| DIVERGENCE.md D18 | Read (the widget seam row) | The viz-widget registry contract: the widget renders ```spreadsheet fenced blocks behind the unchanged upstream markdown seam, stages locally through the in-process engine actor, persists only through ToolInvoker→hkask-mcp-spreadsheet |
| The portfolio what-if precedent (plan §10 Phase 5 record) | Read: `kask/mcp-servers/hkask-mcp-portfolio/src/server.rs` | The first `WorkbookWhatIf` consumer; the pattern the companies server mirrored |
| LogiSheets/Excel formula semantics | Verified what `engine.rs` actually supports (the `check_formula` gate and the error-value convention) before citing any external semantics | The formula primitive (P4). The supported-function inventory is engine-inherited and undocumented at the capability level — recorded as evidence gap F11, not fabricated |
| The immutable-revision model (atomic publication, digest verification, idempotency reconciliation) | The plan states its own invariants (§7) and **does not state an external lineage** (event sourcing, CRDT, or similar) | Evidence gap recorded: the review proceeds on the plan's own stated invariants, formalized in `kask/lean/spreadsheet_revision_invariants.lean` under stated assumptions. The loop register's prior audit (row L22) is the existing cybernetic treatment of the reconciliation cycle |

**Ontology anchors (resolved via `onto_anchor`):** `spreadsheet` → coarse
`5w1h_core` rung (no published or derived concept — finding F10); `artifact` →
`sumo:Artifact` ("An Object that is the product of a Making", Merge.kif) — the
`SpreadsheetArtifact` distinction of plan §2 anchors cleanly.

### Phase hypotheses (FINER-gated, one PDCA cycle each)

| Phase | Hypothesis | F | I | N | E | R | Verdict |
|---|---|---|---|---|---|---|---|
| 0 Inventory | The tool surface, engine entry points, and consumers are exhaustively enumerable from live registration + source + grep | 10 | 10 | 8 | 10 | 10 | **Held** — 2 tools, 30 engine public items, 13 contract re-exports, 9 consumers; counts reconciled by `lisp_eval`. The spec's *expected consumer shape* was falsified: one skill consumes the surface (F14) |
| 1 Primitives | A primitive model derived from the plan §5.1/§6/§7 predicts the capability's shape with named gaps | 9 | 10 | 8 | 10 | 10 | **Held** — 10 primitives; 2 with dead arms (publication's InlineTable; the mutation's expected_access) |
| 2 Fidelity | Each tool's and entry point's deviation from the primitive model is evidenced at file:line | 9 | 10 | 7 | 10 | 10 | **Held** — 16 findings, all cited |
| 3 Integration | Each integration passes or fails the essentialist Exist/Surface/Contract gates | 8 | 10 | 7 | 10 | 10 | **Held** — 8 verdicts (5 already-exists, 1 already-exists-with-repair, 1 operator decision, 1 observation) |
| 4 Formal | The revision invariants are provable in core Lean under stated assumptions, and the finite checks pass | 9 | 9 | 8 | 10 | 10 | **Held** — 5 theorems + helper, `lean_check` exit 0, axioms `[propext]` only; the paired negative control failed as required; 4 `lisp_eval` checks true |
| 5 Proposals | The proposal set covers every finding with required fields | 9 | 10 | 7 | 10 | 10 | **Held** — 12 proposals, field completeness verified by `lisp_eval` |
| 6 Plan | The proposals sequence into dependency-ordered verifiable slices | 9 | 10 | 7 | 10 | 10 | **Held** — 5 slices + 3 operator-gated decisions |

Each hypothesis's falsifier was the phase's own check (a missed consumer would
have falsified Phase 0; an unprovable invariant would have falsified Phase 4).
Phase 0's falsifier in fact fired on the *spec's expected shape*, not the
hypothesis: the skill-consumer null-result the spec anticipated was **not
null** (F14).

## Phase 0 — Coverage table

Live surface via `list_mcp_tools` (server id `spreadsheet`): **2 tools**,
pinned by `tool_surface_is_exactly_2_registered_tools`
(`hkask_mcp_spreadsheet.rs:35-40`). Count reconciliation executed in
`lisp_eval`: 2 tools + 30 engine public items + 13 contract re-exports + 9
consumers = 54 coverage rows; membership of every tool and consumer in the
coverage set verified → `true`.

### Server tools (2) → phase of scope

| Tool | Handler (`kask/mcp-servers/hkask-mcp-spreadsheet/src/server.rs`) | Phase |
|---|---|---|
| `spreadsheet_apply` | L94-119 | 2, 3, 4 |
| `spreadsheet_operation_get` | L124-152 | 2, 3, 4 |

### Engine public API (30 own items) → phase of scope

| Item | Location | Phase |
|---|---|---|
| `WorkbookService::start` / `start_with_root` | service.rs:132, 138 | 1, 2 |
| `WorkbookService::publish` / `open` / `apply` / `operation_get` | service.rs:156, 178, 200, 217 | 1, 2, 4 |
| `WorkbookDocument::artifact` / `viewport` / `stage` / `undo` / `redo` / `sheets` | service.rs:257, 261, 279, 296, 309, 322 | 1, 2 |
| `ArtifactStore::at` / `new_artifact_id` | artifact_store.rs:74, 87 | 1, 2 |
| `ArtifactStore::write_revision` / `read_revision` | artifact_store.rs:115, 156 | 1, 2, 4 |
| `ArtifactStore::write_metadata` / `read_metadata` | artifact_store.rs:167, 181 | 2 (F8) |
| `ArtifactStore::record_operation` / `find_operation` | artifact_store.rs:201, 216 | 1, 2, 4 |
| `production_root` / `digest_of` | artifact_store.rs:59, 67 | 1, 2 |
| `PublishOptions` / `SpreadsheetPublication` / `ViewportContent` | service.rs:32, 39, 49 | 1, 2 (F1, F15) |
| `ArtifactMeta` / `OperationRecord` | artifact_store.rs:33, 46 | 1, 2 |
| `BULK_APPLY_CHUNK_CELLS` / `SPREADSHEET_MCP_SERVER_ID` / `SPREADSHEET_APPLY_TOOL` | hkask_spreadsheet.rs:61, 64, 68 | 1, 2 |

The `engine` module (build_workbook, apply_edits, extract_viewport,
value_to_table_value — engine.rs:80, 183, 284, 43) is `pub mod` with
all-`pub(crate)` contents: implementation surface, not public API (an
observation — from scratch it would be a private `mod engine`).

### Contract re-exports (13, defined in `hkask-types/src/spreadsheet.rs`) → phase

`AnalyticalTable` (L112), `TableColumn` (L101), `TableValue` (L77), `ColumnKind`
(L89), `CellCoordinate` (L226), `SpreadsheetViewport` (L262),
`SpreadsheetArtifactRef` (L337), `SpreadsheetAccess` (L397), `CellEdit` (L443),
`EditTransaction` (L492), `ArtifactOrigin` (L411), `InlineTableBlock` (L550),
`SpreadsheetBlock` (L636) — Phases 1, 2 (F1, F2, F9).

### Widget (D18) → phase

`block.rs` (two-stage parse: tolerant `SpreadsheetBlockBody` L43-61, strict
`strict_block` L77-95), `logic.rs` (pure interaction core: navigation,
block-aligned windows, TSV, editor mapping), `view.rs` (the GPUI widget:
`SHARED_SERVICE` L52-64, `SaveStatus` L69-119, `dispatch_save` L479-527,
`apply_save_response` L532-596), viz-core registration
(`hkask_viz_core.rs:159-172`) — Phases 1, 2, 3 (F5, F6, F7, F16).

### Consumers (9) → phase

| Consumer | Seam | Phase |
|---|---|---|
| `hkask-mcp-portfolio` — `portfolio_what_if` WorkbookWhatIf branch | per-instance engine actor; `what_if_workbook_table` server.rs:89-176; publish at :773-783; hint appended via `report_response` extra_hints (:226-251) | 2, 3 |
| `hkask-mcp-companies` — `scenario_impact_valuation` WorkbookWhatIf branch (landed 2026-10-06, PR-10) | mirrored portfolio: `scenario_impact_workbook_table` valuation.rs:77-130; publish at :861-871 | 2, 3 (F3, F4, F13) |
| `hkask-spreadsheet-widget` | in-process actor + governed ToolInvoker dispatch | 2, 3 (F5, F6, F7) |
| `hkask-viz-core` | `VizWidget` registry (L159-172) | 3 |
| `kask_bridge` | `BUILT_IN_MCP_SERVERS` entry (mcp_servers.rs:523-533; config_env `HKASK_ARTIFACTS_DIR` — the plan §4 recorded deviation) | 3 |
| agent system prompt | display-hint bullet (system_prompt.hbs:60-62) | 3 (F12) |
| `context_server_registry.rs` | structural display-hint path (:1288-1305 — hints render as tool content, no model cooperation) | 3 |
| `company-research-flash` SKILL.md:117-118 | instructs `presentation: "WorkbookWhatIf"` for the full path grid | 3 (F14) |
| `portfolio_viewer.rs` | `ingest_thread` display-hint consumption (:102-106) | 3 |

Docs in scope: the plan itself, `reference/mcp-servers/spreadsheet.md`,
`diagrams/ui-widgets.md` (L700-723), `architecture/skills-and-composition.md`
(§5.4), `loop-register.md` (row L22), `core/MDS.md` (crate table),
`core/PRINCIPLES.md` (server table — spreadsheet has no vocabulary bridge
module), `standardized-artifact-storage.md`.

## Phase 1 — Primitives derivation

Derived from plan §5.1/§6/§7 **before** scoring the current code, per the
from-scratch discipline. "Exercised" = a consumer surface uses it today.

| # | Primitive | Properties / methods | Plan § | Exercised by |
|---|---|---|---|---|
| P1 | **AnalyticalTable** — typed columns/rows | Rectangular by construction; admission limits reject (never clamp); non-finite rejected at validation; no engine/UI types | §2, §5.1 | Both producers (portfolio :89-176; companies :77-130) |
| P2 | **Workbook revision** — immutable, digest-verified | Atomic publication (temp+fsync+rename); never rewritten; digest = byte identity (Phase 0 byte-determinism); opaque ids; contained resolution | §7 | All paths; property_tests.rs:148-204 |
| P3 | **Typed cell edit** — SetCell/SetFormula/ClearCell | Document-independent coordinate validation; formula shape at contract + engine parse-check at apply; `Empty` = measured empty (clears) | §5.1, §7 | Widget (commit_to_edit, tsv_to_edits); agent (`spreadsheet_apply`) |
| P4 | **Formula** — engine-supported function set | `check_formula` gate pre-apply; unknown function → visible `#` error value; parse-invalid rejected pre-apply | §5.1 (hidden complexity) | Exercised (service tests) — but the supported-function **inventory is undocumented** (F11) |
| P5 | **Publication** — origin + options + access mode | Explicit presentation choice; no hidden thresholds; `TooLargeForInline` never truncation; origin = server+tool+args | §6 | **Only the WorkbookWhatIf arm**; InlineTable dead (F1); DataOnly = typed rejection (F15) |
| P6 | **Artifact identity** — opaque, path-escape-proof | Single-segment ids rejected at contract; canonicalize backstop at store; digest 64-hex; human-readable dated folder names | §6, §7 | All paths; path-escape pins |
| P7 | **Idempotency key** — caller-authored, hashed at rest | Record {key, base, result}; replay returns recorded result; key-reuse-different-base rejected; reconciliation explicit-unknown | §7 | Server tool + property tests; **widget path broken on interrupt** (F5) |
| P8 | **SpreadsheetBlock** — bounded transport | viz discriminator, schema version, opaque identity, bounded viewport, origin, server-authored mutation endpoint; must-not-carry list enforced | §6 | All paths |
| P9 | **Display hint** — fenced block emission | Server-authoritative fenced ```spreadsheet block; structural rendering path (no model cooperation); model-instruction bullet | §6 + D18 | 3 emitters + structural path; **bullet text stale** (F12) |
| P10 | **Staging document** — local, undoable, no write-through | Staged edits undoable, never persisted; idempotent open keeps staged state; viewport reads staged state | §5.1, §8 | Widget; **no eviction** (F7) |

Cross-cutting (not a primitive): the **authoritative-state boundary** (§2) —
enforced architecturally (the server links no domain crate; pinned by
`writes_are_contained_beneath_the_artifact_root` and the portfolio proving
slice) and upheld everywhere.

**Phase 1 verdict:** the current API maps onto the primitive model almost
exactly — the plan was followed. The from-scratch deviations concentrate in
dead arms (P5's InlineTable, the mutation's `expected_access`) and in
integration gaps (P7's widget path, P9's bullet), not in the decomposition.

## Phase 2 — Fidelity findings

Every claim cited. IS statements are verified against the tree at the cited
lines; OUGHT statements are anchored to a plan section or a `.rules` trap and
are adjudicated further in the proposal set.

| # | Finding (IS → OUGHT) | Evidence |
|---|---|---|
| F1 | **InlineTable is a dead contract arm end-to-end.** IS: contract-complete (`InlineTableBlock` hkask-types spreadsheet.rs:550-628), engine-complete (service.rs:464-467), zero producers (grep: only engine-internal tests pass `SpreadsheetAccess::InlineTable`), and the widget **cannot render it** — `strict_block` requires artifact/viewport/mutation, so an inline block surfaces as `MissingArtifact` (block.rs:77-95). OUGHT: per §6 the mode is ratified and Phase 7 reserves it for research outputs (plan L650), but per §13 "no speculative compatibility shell" — a from-scratch build either completes it (renderer + producer) or does not ship it | hk-types:550-628; service.rs:464-467; block.rs:77-95; plan §6, §10 L650, §13 |
| F2 | **`EditTransaction.expected_access` is transported and validated but never read.** IS: defined at hkask-types spreadsheet.rs:493-498, validated at :517-541 (shape only); grep over the repo finds 3 matches, all definition-site; `handle_apply` never reads it (service.rs:537-616). OUGHT: §7 says each mutation carries "Expected access mode" — the field should either be enforced (a typed rejection when it contradicts the artifact's mode) or deleted as dead wire weight | hk-types:493-498; service.rs:537-616; plan §7 L258-264 |
| F3 | **Display-hint serialization is core-owned per the plan but hand-rolled in both producers.** IS: §5.1's hidden-complexity list names "SpreadsheetBlock and display-hint serialization" as complexity the core hides (plan L195); in fact the fence format is constructed at three sites — `block_display_hint` (spreadsheet server.rs:66-70), `spreadsheet_hint` byte-identical in portfolio (server.rs:180-192) and companies (valuation.rs:58-70) — and hand-parsed in the widget (view.rs:561-568) while a canonical extractor exists (`hkask_types::tool_response::display_hints_from_output_text`, used by the structural path). OUGHT: one owner for the fence format | plan §5.1 L195; server.rs:66-70; portfolio server.rs:180-192; companies valuation.rs:58-70; view.rs:561-568 |
| F4 | **`map_spreadsheet_error` exists in three copies with divergent catch-alls.** IS: the server's per-variant map (server.rs:32-62, `_ → internal`, commented) vs the producers' coarse copies (portfolio server.rs:56-65; companies valuation.rs:43-53, `_ → invalid_argument`). A future `SpreadsheetError` variant would classify differently per server. OUGHT: per the `.rules` trap, error classification must be per-variant — one canonical classifier | server.rs:32-62; portfolio server.rs:56-65; valuation.rs:43-53 |
| F5 | **The widget's interrupted-save reconciliation is not actionable.** IS: `dispatch_save` mints a fresh UUID idempotency key per save (view.rs:491-496); on `Interrupted` the label is static — "reconcile via spreadsheet_operation_get" (view.rs:89-93) — and the key is never surfaced, so no caller can complete the §7 reconciliation for widget-initiated saves (the tool needs artifact_id + key, server.rs:82-85). OUGHT: the interrupted state carries the reconciliation identity | view.rs:491-496, 89-93; server.rs:82-85; plan §7 L268 |
| F6 | **Conflict detection string-sniffs the error message.** IS: `SaveStatus::from_invoke_error` matches `message.contains("failed_precondition") \|\| message.contains("Conflict")` (view.rs:103-115) because the ToolInvoker seam flattens the typed kind to `Failed(String)` (hkask_tool_invoker.rs:69-71). The agent path deleted exactly this sniffing (DIVERGENCE.md error-path collapse); `kanban_panel` independently re-parses the error envelope from Ok strings (kanban_panel.rs:795-805) — two widgets compensating for the seam. OUGHT: the typed kind crosses the seam | view.rs:103-115; hkask_tool_invoker.rs:69-71; kanban_panel.rs:795-805 |
| F7 | **The engine actor's document cache never evicts.** IS: `ActorState.documents` (service.rs:336-339) has `insert` (:533) and no remove/retain anywhere; the widget opens every new revision after every save (view.rs:599-623) against a process-lifetime actor (`SHARED_SERVICE`, view.rs:52-64) — one resident `Workbook` per opened revision, forever. OUGHT: bounded residency | service.rs:336-339, 533; view.rs:52-64, 599-623 |
| F8 | **The publish metadata window is unpinned and unruled.** IS: `handle_publish` writes the revision then the metadata (service.rs:480-490), and `write_metadata` is non-atomic `std::fs::write` (artifact_store.rs:167-179) — a crash between leaves a revision whose block construction fails (`read_metadata` → `UnknownArtifact` at service.rs:594). The loop register rules the revision/record window deliberate (row L22) but not this one. OUGHT: either reorder (metadata first) or pin the window as deliberate | service.rs:480-490, 594; artifact_store.rs:167-179; loop-register.md L22 |
| F9 | **`SpreadsheetBlock.access` is a tautology.** IS: hardcoded `WorkbookWhatIf` at construction (hkask-types spreadsheet.rs:661-671), rejected if different (:696-701); same pattern for `InlineTableBlock.access` (:593-600, :618-623). The block *type* is the access mode. OUGHT: from scratch the field would not exist (or would be the wire discriminator, deliberately) | hk-types:661-671, 696-701, 593-623 |
| F10 | **The plan §2's ontology registration never landed.** IS: §2 directed derived-concept registration for the six public-contract terms before the names became public contracts (plan L40); the Phase 1 record says a ruling was requested (L409-412); `onto_anchor "spreadsheet"` today still resolves at the coarse `5w1h_core` rung with "request a ruling from the operator". OUGHT: the ruling lands | plan §2 L40, L409-412; onto_anchor receipt |
| F11 | **The supported-formula inventory is undocumented (evidence gap).** IS: the engine gates formulas via `check_formula` (engine.rs:249-255) and surfaces unknown functions as visible error values (:43-51); §11 pins only the negative behavior ("Unsupported formulas surface as spreadsheet errors"); no doc states which functions `logisheets-rs =1.15.1` supports. OUGHT: a probe-pinned inventory in the reference doc — recorded as an evidence gap, not fabricated here | engine.rs:43-51, 249-255; plan §11 L664 |
| F12 | **The display-hint instruction bullet is stale w.r.t. the third emitter.** IS: the bullet names only `spreadsheet_apply`/`portfolio_what_if` (system_prompt.hbs:60-62; doc mirror skills-and-composition.md:236-239) while `scenario_impact_valuation` also emits hints (valuation.rs:872-873, landed 2026-10-06). The structural path is name-agnostic (context_server_registry.rs:1288-1305), so rendering is unaffected — the staleness is in the agent-side instruction text. OUGHT: the bullet states the field-based rule, not an emitter enumeration | system_prompt.hbs:60-62; skills-and-composition.md:236-239; valuation.rs:872-873 |
| F13 | **The plan's Phase 6 record is absent for the companies expansion that shipped.** IS: §10 Phase 6 (L618-641) still reads as unchartered candidates while the companies `WorkbookWhatIf` landed 2026-10-06 (valuation.rs:849-874, pinned at acquisition_tests.rs:1154+). OUGHT: the phase record exists like every other phase's | plan §10 L618-641; valuation.rs:849-874 |
| F14 | **The skill-consumer expectation is falsified — the surface IS skill-facing.** IS: `company-research-flash/SKILL.md:117-118` instructs calling `scenario_impact_valuation` with `presentation: "WorkbookWhatIf"` ("the published workbook carries every path ... its ```spreadsheet display hint renders inline"). The instruction was audited against the tool contract and is **accurate** (matches valuation.rs:634-636, 849-874: full grid, editable revision, default unchanged when omitted). Recorded as a finding, not a skipped check | company-research-flash/SKILL.md:117-118; valuation.rs:634-636, 849-874 |
| F15 | **`DataOnly` in publish is a typed rejection — the enum does double duty.** IS: `SpreadsheetAccess` is both the caller's presentation choice and the engine's dispatch; `publish` rejects DataOnly with `AccessMismatch` (service.rs:461-463) because DataOnly means "don't publish". OUGHT (observation): from scratch the publish parameter would be the two publishing modes; the three-mode enum belongs to the caller-side choice. Low severity — the rejection is visible and typed | service.rs:461-463; hk-types:397-405 |
| F16 | **The widget re-implements the fence parse.** IS: `apply_save_response` hand-strips the ```spreadsheet fence (view.rs:561-568) while the canonical extractor exists (used by the structural path and the panels). Folds into F3's remedy (one owner for construct + parse) | view.rs:561-568; hkask_types::tool_response |

Observations recorded without findings: the response's top-level `artifact`
duplicates the block's artifact by design and is asserted consistent
(server.rs:107-111; tool_behavior.rs:149-155) — good for agent chaining; four
engine actors (server, portfolio, companies, editor process) share one
artifact root safely by immutability — the plan's architecture, not a defect;
the widget's fetch window (64×16, logic.rs:22-24) is smaller than the block's
initial viewport (≤200×32) by §8 virtualization design.

## Phase 3 — Integration review (essentialist verdicts)

Exist = deleting loses behavior; Surface = the interface earns its width;
Contract = every abstraction adds behavior beyond a direct call.

| Integration | Exist | Surface | Contract | Verdict |
|---|---|---|---|---|
| **Portfolio what-if branch** | Yes — the only what-if staging workbook for portfolios | One enum + one table builder + one publish call; the hint formatter and error mapper are the duplicated surface (F3, F4) | `WhatIfPresentation` mirrors the two relevant `SpreadsheetAccess` arms — acceptable local typing | **already exists (keep)**; the shared helpers extract (SP-01) |
| **Companies path-grid branch** | Yes — the workbook is where the FULL 2^N grid lives (JSON caps at 50) | The mirror is the finding: `spreadsheet_hint` byte-identical, `map_spreadsheet_error` coarse-copied, a second local presentation enum, a second per-server actor | Same as portfolio | **already exists (keep the capability); the mirrored helpers move to the engine** (SP-01) |
| **D18 widget (in-process staging vs governed MCP)** | Yes — the only editable surface | The dual path is exactly the plan §4 diagram (local staging → core; commit → invoker → MCP) | The staging document and the save payload mirror are genuine behavior | **already exists (keep)**; the reconciliation loop closes (SP-02, SP-03) and residency bounds (SP-04) |
| **Display-hint convention** | Yes — the model bullet puts the block in the assistant's reply (the structural path renders it as tool content; both are load-bearing) | One bullet | The bullet's emitter enumeration is stale (F12) | **already exists; text repair** (SP-07) |
| **company-research-flash (skill)** | Yes — the instruction is load-bearing for the flash pipeline's COMMUNICATION stage | One instruction | Audited accurate (F14) | **already exists (keep, no action)** |
| **InlineTable arm** | No — no producer, no renderer; deleting today loses no behavior beyond tests | Contract + engine arm + tests | The plan reserves it (Phase 7) — carrying it unrendered is the "speculative compatibility shell" §13 refuses | **operator decision: complete or retire** (SP-05) |
| **portfolio_viewer hint ingestion** | Yes — the panel consumes the combined hint (parsers scan for their own fence) | Thin | Adds the specialized upper viewer | **already exists (keep)** |
| **Multi-actor over one root** | Yes — per-instance actors are the testability design (Phase 5 record deleted the OnceLock global) | — | — | **observation only, no proposal** |

## Phase 4 — Formal and structural checks

### Lean spec pin (ran)

`kask/lean/spreadsheet_revision_invariants.lean` — core Lean 4.34.0, no
Mathlib, checked via `lean_check` (the project's pinned Lake toolchain):

| Theorem | Invariant | Status |
|---|---|---|
| `failed_apply_leaves_store_unchanged` (+ corollary `failed_apply_keeps_base_readable`) | A failed apply (conflict / key reuse / unknown base) leaves the store — and the base revision — unchanged and readable | checked, `[propext]` |
| `digest_mismatch_is_conflict_never_overwrite` | A digest mismatch is a conflict, never a silent overwrite: store unchanged, nothing published under the new id | checked, `[propext]` |
| `idempotent_replay_returns_recorded_result` | A repeated identity returns the recorded result revision and mints nothing | checked, `[propext]` |
| `key_reuse_against_different_base_is_rejected` | A key reused against a different base is rejected, store unchanged | checked, `[propext]` |
| `applied_extends_history_and_preserves_base` | A successful apply extends history by exactly one cons; every prior revision remains; the base stays readable with its exact digest | checked, `[propext]` |

`lean_check` exit 0; axioms `[propext]` only (the standard logical axiom — no
`sorryAx`, no computation axioms). **Negative control**
(`spreadsheet_revision_negative_control.lean`): exit 1, failed exactly as
required — "`decide` proved that the proposition ... is false" over a concrete
mismatching instance, so the checker is a real oracle and INV-2 is not
vacuous. **Stated assumptions** (the model abstracts these; the Rust side
pins them): atomic transition (the crash windows are deliberately outside —
pinned by property_tests.rs:216-271 and ruled deliberate in loop-register
L22); digest identity (Phase 0 byte-determinism); storage reflects the store;
fresh revision ids (uuid v4; the store's colliding-id refusal is the
backstop). A proof here establishes the invariant under these assumptions —
not fitness to the plan, and not the Rust implementation (pinned by
service.rs tests, property_tests.rs, and the server's tool_behavior.rs — the
implementation-side oracle).

### Finite structural checks (ran, `lisp_eval`)

1. Coverage reconciliation: 2 tools + 30 engine items + 13 contract re-exports
   + 9 consumers = 54 rows; arithmetic → `true`.
2. Coverage membership: all 11 tool/consumer names present in the coverage
   set → `true`.
3. Proposal completeness: 12 proposals × 7 required fields = 84; every
   proposal carries ≥1 evidence citation and ≥1 enumerated breaking change →
   `true`.
4. Kata convergence gate: `(and (= misalignment_count 0) (= (length
   weak_finer) 0) testable admissible feasible)` → `true`.

## Checks run versus proposed

**Ran:** live tool-surface enumeration (`list_mcp_tools`); full reads of the
plan (766 lines), all four engine modules, the server crate, the widget
crate, both consumer branches, the wire contract, the ToolInvoker seam, the
viz-core wiring, D18, and the in-scope docs; the consumer greps
(`spreadsheet_apply`/`spreadsheet_operation_get`, `WorkbookWhatIf`,
`InlineTable`, `display_hint`, `map_spreadsheet_error`, `expected_access`,
document-cache mutation, skills tree, Cargo dependents, `InvokeError`
consumers); `onto_anchor` for `spreadsheet` and `artifact`; the Lean spec pin
+ negative control (receipts above); the four `lisp_eval` checks.

**Remain proposed** (implementation- or operator-gated, nothing here
implemented): every proposal slice's cargo test/clippy gates; the
formula-inventory probe (SP-10); the eviction pin (SP-04); the
reconciliation-surfacing pin (SP-02); the canonical-mapper equivalence pin
(SP-01); the operator rulings (SP-05 complete-or-retire, SP-06
enforce-or-delete, SP-09 ontology registration).

**Skill battery honesty:** loaded and followed — kata-improvement,
hypothesis-framer, essentialist, refactor-architecture, deep-module,
lean-prover, task-breakdown, code-review. Applied from description (their
methods are embedded in the loaded skills) — pragmatic-semantics (IS/OUGHT
and constraint-force labels), pragmatic-cybernetics (the loop-register L22
audit is the existing cybernetic treatment; its five-property reading was
applied), grill-me (the self-challenge embedded in code-review's adjudicate
phase), coding-guidelines (surgical scope on every code-specifying
proposal), mcp-tool-review (its inventory-reconciliation method shaped
Phase 0), skill-maintenance (the company-research-flash audit ran — F14).

**Execution postscript (2026-10-06/07):** every "remains proposed" check
above has since run or landed — the improvement plan's execution record
names each (all twelve proposals SP-01..SP-12, commits `1a8c500c12`,
`c9a5c3b855`, `f66b136fe2`+`2bcf794dc8`, `87b673285`; the operator ruled
SP-05 complete, SP-06 enforce, SP-09 the Dublin Core/SUMO/PKO anchors;
the Lean spec pins now re-check in CI via `check-lean-spec-pins.sh`). This
document is the review-time record; the live state is the tree.
