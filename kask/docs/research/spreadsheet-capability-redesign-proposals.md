---
title: "Spreadsheet Capability — From-Scratch Design Review (Proposal Set)"
audience: [operators, developers, architects]
last_updated: 2026-10-06
version: "0.1.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, lifecycle, trust]
kind: research
related:
  - spreadsheet-capability-redesign-review.md
  - spreadsheet-capability-redesign-improvement-plan.md
---

# Proposal Set — Phase 5

Fields: **id** · **component** · **goal tags** (fidelity / engine-efficiency /
integration) · **evidence** (file:line, from the review's findings) ·
**breaking changes** (enumerated: the path each replaces and what it deletes —
a proposal that only adds is incomplete) · **effort** (XS/S/M/L) ·
**depends**. Every proposal that specifies code was run through code-review
IS/OUGHT adjudication (each finding in the review states what IS, cited, and
what OUGHT to be, anchored to a plan section or `.rules` trap) and
coding-guidelines (surgical scope; no speculative generality). Nothing here
is implemented; nothing here is committed.

---

## SP-01 — One owner for the display-hint fence and the error classifier

- **Component:** `hkask-spreadsheet` (the hint formatter, as
  `SpreadsheetPublication::display_hint()`), `hkask-types` (the canonical
  error classifier, as `SpreadsheetError::mcp_kind() -> McpErrorKind`), and
  the three servers' call sites.
- **Tags:** fidelity (plan §5.1 hidden-complexity list names display-hint
  serialization as core-owned), integration.
- **Evidence:** F3, F4, F16 — plan L195; spreadsheet server.rs:32-62, 66-70;
  portfolio server.rs:56-65, 180-192; companies valuation.rs:43-53, 58-70;
  widget view.rs:561-568.
- **What it builds:** two canonical functions. The hint formatter lives on
  the publication (pure serialization — `SPREADSHEET_VIZ` + serde, no new
  dependencies); the error classifier lives beside `SpreadsheetError` in
  `hkask-types` (which already defines `McpErrorKind`), so all three servers
  collapse to `McpToolError::new(e.mcp_kind(), e.to_string())` with the
  server's per-variant table as the canonical classification.
- **Breaking changes:** deletes the two byte-identical `spreadsheet_hint`
  copies (portfolio server.rs:180-192; companies valuation.rs:58-70) and the
  server's private `block_display_hint` (server.rs:66-70) — three
  construction sites replaced by one; deletes all three local
  `map_spreadsheet_error` copies (server.rs:32-62; portfolio server.rs:56-65;
  valuation.rs:43-53) — the producers' `_ → invalid_argument` catch-all dies
  with them, removing the cross-server classification divergence; the
  widget's hand-rolled fence strip (view.rs:561-568) is replaced by the
  canonical extraction.
- **Effort:** S. **Depends:** none.

## SP-02 — Surface the interrupted save's reconciliation identity

- **Component:** `hkask-spreadsheet-widget` — `SaveStatus` + `dispatch_save`.
- **Tags:** integration, fidelity (§7: reconciliation keyed by the
  idempotency identity).
- **Evidence:** F5 — view.rs:491-496 (fresh UUID per save), 89-93 (static
  `Interrupted` label), server.rs:82-85 (reconciliation needs artifact_id +
  key).
- **What it builds:** `SaveStatus::Interrupted` carries `(artifact_id,
  idempotency_key)` and the label renders them, so the §7 reconciliation
  instruction becomes actionable — the operator can hand the identity to the
  agent (or a future reconcile affordance can dispatch
  `spreadsheet_operation_get` through the same governed invoker).
- **Breaking changes:** the static label string (view.rs:89-93) is deleted in
  favor of the identity-carrying label; `SaveStatus::Interrupted` becomes a
  payload variant (a private enum — no wire break); the
  `interrupted_maps_to_the_reconciliation_status` pin (view.rs:1060-1066) is
  rewritten to assert the payload.
- **Effort:** S. **Depends:** none (SP-03 complements it).

## SP-03 — Carry the typed error kind across the ToolInvoker seam

- **Component:** `hkask-tool-invoker` (`InvokeError::Failed`), the production
  invoker (`crates/zed/src/main.rs:4067-4092`), and the widget consumers.
- **Tags:** integration, engine-efficiency (removes re-parsing work).
- **Evidence:** F6 — hkask_tool_invoker.rs:69-71 (`Failed(String)`); view.rs:
  103-115 (string-sniffing); kanban_panel.rs:795-805 (independent envelope
  re-parsing — a second widget compensating for the same seam); DIVERGENCE.md
  error-path collapse (the agent path deleted sniffing; the typed kind is
  already in the envelope's `raw_output`).
- **What it builds:** `InvokeError::Failed` carries the structured kind
  (message + `McpErrorKind`), taken from the typed envelope the production
  invoker already sees; the spreadsheet widget's conflict detection matches
  structurally; the kanban panel's envelope re-parse simplifies onto the same
  kind.
- **Breaking changes:** `InvokeError::Failed(String)` is replaced by a
  kind-carrying variant — every constructor and matcher updates. Enumerated
  blast radius (grep `InvokeError::Failed|from_invoke_error`, 19 matches):
  hkask-tool-invoker itself; the production impl (main.rs:4067-4092); the
  spreadsheet widget (view.rs:103-115, tests :1081-1089); the kanban panel
  (:795-805, retry tests :1981-1985); the media widget (media_widget.rs:1765);
  the portfolio widget (view.rs:1496); the media viewer (media_viewer.rs:948);
  the swarm panel (tests :2128-2153); kask_bridge's algedonic test invoker
  (:54-81). The string-sniffing branch (view.rs:107) is deleted.
- **Effort:** M. **Depends:** none.

## SP-04 — Bound the engine actor's document residency

- **Component:** `hkask-spreadsheet` — `ActorState.documents`, a new
  `WorkbookDocument::close()`.
- **Tags:** engine-efficiency.
- **Evidence:** F7 — service.rs:336-339 (the map), :533 (insert only; no
  remove/retain anywhere), view.rs:52-64 (process-lifetime actor), 599-623
  (a fresh open per saved revision).
- **What it builds:** bounded residency: a cap on open documents with
  least-recently-used eviction that never evicts a document with staged
  edits (a conservative dirty flag set on `Stage`), plus an explicit
  `close()` so the widget can release a superseded revision deterministically.
- **Breaking changes:** replaces the implicit "documents live forever"
  contract with bounded residency; adds `close()` to `WorkbookDocument` (the
  plan §5.1 conceptual surface gains one method — justified by the §8
  widget's reload-per-save pattern, which is the growth driver). No wire
  change.
- **Effort:** M. **Depends:** none.

## SP-05 — Complete or retire the InlineTable presentation arm

- **Component:** the contract (`hkask-types`), the engine's publish arm, the
  widget renderer, and (on completion) the first producer.
- **Tags:** fidelity (§6's ratified mode table), integration.
- **Evidence:** F1 — hk-types:550-628; service.rs:464-467; block.rs:77-95;
  plan §6, §10 L650 (Phase 7 reserves InlineTable for research outputs), §13
  ("No speculative compatibility shell").
- **What it builds (operator decision, two licensed outcomes):**
  - **Complete (recommended):** a bounded inline-table renderer in the widget
    (the §6 "Native bounded sortable/scrollable table" — the two-stage parse
    discriminates on `access` after the viz claim) plus the first producer
    (research evidence tables, Phase 7's own candidate list) — the contract
    and engine arm already exist and are tested.
  - **Retire:** delete `InlineTableBlock`, `SpreadsheetPublication::Inline`,
    `SpreadsheetAccess::InlineTable`, the engine's Inline arm, and their
    tests; amend plan §6's mode table and §10 Phase 7's reservation.
- **Breaking changes:** complete — the widget's strict parse gains a second
  block shape (the `MissingArtifact` error path for inline bodies is
  deleted — it becomes a renderable shape). Retire — the deletions above,
  plus the `TooLargeForInline` error variant loses its only producer (the
  variant goes with it), plus the plan amendments.
- **Effort:** M (complete) / S (retire). **Depends:** operator ruling
  (functional: does the user want bounded inline tables for research
  outputs, or a smaller contract?).

## SP-06 — Enforce or delete `EditTransaction.expected_access`

- **Component:** the contract (`hkask-types`) and `handle_apply`.
- **Tags:** fidelity (§7), essentialism.
- **Evidence:** F2 — hk-types:493-498, :517-541; service.rs:537-616; plan §7
  L258-264; grep: 3 matches, all definition-site.
- **What it builds (operator decision):** **Enforce (recommended — §7's
  letter):** `handle_apply` rejects a transaction whose `expected_access` is
  not `WorkbookWhatIf` (a `AccessMismatch` — only workbook artifacts exist,
  so any other mode on a mutation is caller confusion worth a typed
  rejection). **Delete:** the field leaves the wire; `EditTransaction::new`
  loses a parameter; all constructors update (widget view.rs:491-496; the
  engine, server, and portfolio test suites; property_tests.rs:116-124).
- **Breaking changes:** enforce — applies with a non-workbook
  `expected_access` now reject (no current caller sends anything else —
  verified: the widget and every test construct with `WorkbookWhatIf`).
  Delete — the wire field and its validation are deleted.
- **Effort:** XS (either). **Depends:** operator ruling.

## SP-07 — Make the display-hint instruction emitter-agnostic

- **Component:** `crates/agent/src/templates/system_prompt.hbs` +
  `kask/docs/architecture/skills-and-composition.md` §5.4.
- **Tags:** fidelity, integration.
- **Evidence:** F12 — system_prompt.hbs:60-62;
  skills-and-composition.md:236-239; valuation.rs:872-873 (the third
  emitter); F14 (the skill whose "renders inline" promise depends on the
  bullet).
- **What it builds:** the bullet states the field-based rule — "When any
  tool result contains a `display_hint` field with a fenced ```spreadsheet
  block, copy that block verbatim into your reply" — with the doc mirror
  updated in the same pass.
- **Breaking changes:** the stale emitter enumeration ("an editable workbook
  what-if from `spreadsheet_apply` or `portfolio_what_if`") is deleted in
  favor of the field-based rule; the agent template's 902-test pin runs
  against the changed bullet.
- **Effort:** XS. **Depends:** none.

## SP-08 — Record the companies expansion in the plan's Phase 6

- **Component:** `kask/docs/plans/logisheets-spreadsheet-capability-plan.md`
  §10 Phase 6.
- **Tags:** fidelity.
- **Evidence:** F13 — plan L618-641 (no record; Phase 5's record still says
  Phases 6–7 "remain unchartered post-proving-slice candidates" at L602-603);
  valuation.rs:849-874 (landed 2026-10-06); acquisition_tests.rs:1154+
  (PR-10 pin).
- **What it builds:** the Phase 6 record entry for the companies slice
  (scenario_impact_valuation's WorkbookWhatIf), mirroring the other phase
  records' shape: what shipped, the gates observed, the deviation notes.
- **Breaking changes:** replaces the plan's stale "unchartered" claim for the
  companies slice (the claim is deleted, the record replaces it).
- **Effort:** XS. **Depends:** none.

## SP-09 — Land the derived-ontology ruling for the six contract terms

- **Component:** `hkask-bridge-ontology` derived registry (operator ruling)
  + plan §2.
- **Tags:** fidelity.
- **Evidence:** F10 — plan §2 L40 (registration directed before public
  contract names), L409-412 (ruling requested 2026-09-18);
  `onto_anchor "spreadsheet"` today → coarse `5w1h_core` with "request a
  ruling from the operator".
- **What it builds:** the operator ruling registering derived concepts for
  `AnalyticalTable`, `SpreadsheetBlock`, `SpreadsheetArtifact`,
  `SpreadsheetViewport`, `EditTransaction`, `SpreadsheetError` (§2's own
  list), after which the terms resolve at the derived tier.
- **Breaking changes:** none (the anchors improve from coarse; no code
  path). The coarse-anchor state it replaces is recorded in the review
  (F10).
- **Effort:** XS. **Depends:** operator ruling.

## SP-10 — Publish the supported-formula inventory

- **Component:** `kask/docs/reference/mcp-servers/spreadsheet.md` + a probe
  script under `kask/scripts/` (bash, per the project rule).
- **Tags:** fidelity (evidence-gap closure).
- **Evidence:** F11 — engine.rs:43-51 (error-value convention), 249-255
  (`check_formula` gate); plan §11 L664 (only the negative behavior is
  pinned); no doc lists the supported functions of `logisheets-rs =1.15.1`.
- **What it builds:** an admission-gate-style probe (a scratch harness
  running `check_formula` over a candidate function inventory) whose
  verified results land in the reference doc — the formula primitive (P4)
  gains a documented positive inventory instead of an undocumented
  engine-inherited set.
- **Breaking changes:** replaces the absence (the inventory's
  non-existence is the thing deleted); no code change.
- **Effort:** S. **Depends:** none.

## SP-11 — Close or pin the publish metadata window

- **Component:** `hkask-spreadsheet` — `handle_publish` ordering +
  `write_metadata` atomicity.
- **Tags:** engine-efficiency, fidelity (§7's atomic-publication spirit).
- **Evidence:** F8 — service.rs:480-490 (revision then metadata),
  artifact_store.rs:167-179 (non-atomic `std::fs::write`), service.rs:594
  (`read_metadata` → `UnknownArtifact` on the window's far side);
  loop-register L22 (the sibling revision/record window is ruled deliberate
  and tested — this one is neither).
- **What it builds (recommended: close it):** write `artifact.json` before
  the first revision (the metadata is computed before either write —
  service.rs:483-489) and make `write_metadata` atomic like
  `write_revision`; add the crash-window property test mirroring
  property_tests.rs:216-271's shape. Alternative: pin the window as
  deliberate in the loop register, like L22.
- **Breaking changes:** the publish ordering changes (metadata-before-
  revision replaces revision-before-metadata); the non-atomic
  `write_metadata` is deleted in favor of the atomic primitive.
- **Effort:** XS. **Depends:** none.

## SP-12 — Delete the tautological `access` field from the two block types

- **Component:** `hkask-types/src/spreadsheet.rs` — `SpreadsheetBlock`,
  `InlineTableBlock`.
- **Tags:** fidelity (essentialism).
- **Evidence:** F9 — hk-types:661-671 (hardcoded at construction), 696-701
  (rejected if different), 593-600, 618-623 (the same pattern on the inline
  block).
- **What it builds:** nothing — it removes: the block *type* is the access
  mode, so the field is deleted from both blocks and their validators. If
  the wire wants a self-describing discriminator, the viz tag already is one.
- **Breaking changes:** deletes `SpreadsheetBlock.access` and
  `InlineTableBlock.access` and their validation branches; the wire shape
  changes (no backward-compatibility requirement); the round-trip and
  validation tests update; the widget's tolerant body never read the field
  (block.rs:43-61 — serde-defaulted absence), so the widget is unaffected.
- **Effort:** XS. **Depends:** SP-05 (the InlineTableBlock's fate is decided
  there; deleting its field is moot if the type is retired).
