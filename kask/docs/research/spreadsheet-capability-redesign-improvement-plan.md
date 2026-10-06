---
title: "Spreadsheet Capability — From-Scratch Design Review (Improvement Plan)"
audience: [operators, developers, architects]
last_updated: 2026-10-06
version: "0.1.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, lifecycle, trust]
kind: research
related:
  - spreadsheet-capability-redesign-review.md
  - spreadsheet-capability-redesign-proposals.md
---

# Improvement Plan — Phase 6

Sequenced per task-breakdown: verifiable vertical slices, explicit
dependencies, per-slice acceptance criteria, and a checkpoint after every
slice. Nothing is implemented; nothing is committed — this plan is a
proposal awaiting operator charter (the same posture the LogiSheets plan
itself held before its 2026-09-18 charter). The repo gates for every slice
are the plan §11 set: `cargo test -p hkask-spreadsheet`, `-p
hkask-mcp-spreadsheet`, `-p hkask-spreadsheet-widget`, `-p
hkask-mcp-portfolio`, `-p hkask-mcp-companies` as touched, `./script/clippy`,
`cargo check -p zed`, and the full symbol/doc sweep on every removal.

## Slice 1 — The shared seam (SP-01)

**Why first:** highest value, lowest risk — five duplicated functions die,
the plan §5.1 hidden-complexity fidelity violation closes, and every later
slice builds on the canonical surfaces.

- Build: `SpreadsheetPublication::display_hint()` in `hkask-spreadsheet`;
  `SpreadsheetError::mcp_kind()` in `hkask-types`; the three servers'
  call sites collapse onto them.
- Delete: 2× `spreadsheet_hint` (portfolio server.rs:180-192; companies
  valuation.rs:58-70), 1× `block_display_hint` (spreadsheet server.rs:66-70),
  3× `map_spreadsheet_error` (server.rs:32-62; portfolio server.rs:56-65;
  valuation.rs:43-53), the widget's fence strip (view.rs:561-568).
- Acceptance: a mapper-equivalence pin — a table test over every current
  `SpreadsheetError` variant asserting one classification across all three
  servers (the divergence F4 is pinned dead); the existing tool-behavior
  suites pass unchanged (the wire shape is identical); `./script/clippy`
  clean on all five touched crates; machete clean (no orphaned deps).
- Depends: none.

**Checkpoint:** all five crates' suites green; the fence format has exactly
one construction site and one canonical parser.

## Slice 2 — The reconciliation loop closes (SP-03, then SP-02)

**Why second:** the widget's §7 loop is the capability's one broken contract
(F5); SP-03 lands first so SP-02's payload can carry the structural kind.

- Build (SP-03): `InvokeError::Failed` carries message + `McpErrorKind`
  (from the typed envelope the production invoker already sees,
  main.rs:4067-4092); the spreadsheet widget matches structurally; the
  kanban panel's envelope re-parse (kanban_panel.rs:795-805) simplifies
  onto the kind.
- Build (SP-02): `SaveStatus::Interrupted` carries `(artifact_id,
  idempotency_key)`; the label renders them.
- Delete: the string-sniffing branch (view.rs:103-115); the static
  `Interrupted` label (view.rs:89-93).
- Acceptance: a widget test asserting an interrupted save surfaces the
  reconciliation identity (artifact_id + key) in the label; a structural
  conflict test (a `failed_precondition` kind maps to the conflict status
  with no string matching); every enumerated `InvokeError::Failed` consumer
  (review F6's list — 7 crates) updated and green; the agent template's
  902-test pin unaffected.
- Depends: Slice 1 (the widget's response parsing uses the canonical
  extraction).

**Checkpoint:** an interrupted widget save can be reconciled end-to-end by
hand with the surfaced identity — the §7 loop is closed for the widget path.

## Slice 3 — Engine residency and publish ordering (SP-04, SP-11)

- Build (SP-04): bounded LRU residency for `ActorState.documents` (never
  evicting a staged-dirty document) + `WorkbookDocument::close()`; the
  widget closes superseded revisions on save.
- Build (SP-11): metadata-before-revision publish ordering; atomic
  `write_metadata`; the crash-window property test.
- Delete: the unbounded-residency contract; the non-atomic
  `write_metadata` body.
- Acceptance: an eviction pin (open N revisions → resident ≤ cap; a
  staged-dirty document survives eviction pressure); the new crash-window
  property test mirrors property_tests.rs:216-271's shape (a crash between
  the writes leaves the artifact openable and block-constructible);
  `reopen_keeps_staged_state` (service.rs:1198-1219) still passes — the
  idempotent-open semantics are untouched.
- Depends: none (parallel-safe with Slice 2 after Slice 1).

**Checkpoint:** the engine's memory is bounded by design, and every publish
crash window is either closed or pinned-and-ruled.

## Slice 4 — Operator-gated contract decisions (SP-05, SP-06, SP-12)

Each is a licensed breaking change awaiting a functional ruling:

- SP-05 InlineTable complete-or-retire (complete: renderer + research
  producer, M; retire: S). Acceptance (complete): a widget test rendering
  an inline block as a table (the `MissingArtifact` path for inline bodies
  is gone); the first research producer's tool-behavior pin. Acceptance
  (retire): the deletions enumerated in SP-05 with the plan §6/§7
  amendments in the same change.
- SP-06 `expected_access` enforce-or-delete (XS). Acceptance (enforce): a
  rejection pin for a non-workbook expected_access; every existing caller
  unchanged (all construct with `WorkbookWhatIf` — verified). Acceptance
  (delete): the field's wire absence with all constructors updated.
- SP-12 the tautological `access` field (XS; moot for the inline block if
  SP-05 retires it). Acceptance: round-trip and validation tests updated;
  the widget unaffected (its tolerant body never read the field).

**Checkpoint:** the contract carries no dead arms — every field and variant
has either a consumer or a plan-recorded reservation with a charter.

## Slice 5 — Documentation, ontology, and the evidence gap (SP-07, SP-08, SP-09, SP-10)

- SP-07: the display-hint bullet becomes emitter-agnostic
  (system_prompt.hbs:60-62; skills-and-composition.md §5.4) — the agent
  template's 902-test pin runs.
- SP-08: the plan's Phase 6 record entry for the companies slice.
- SP-09: the operator's derived-ontology ruling for the six contract terms
  (§2's list); the anchors resolve at the derived tier afterward.
- SP-10: the supported-formula inventory — an admission-gate-style probe
  (bash script under `kask/scripts/`, scratch harness outside the
  workspace) whose verified results land in
  `reference/mcp-servers/spreadsheet.md`; the evidence gap F11 closes with
  receipts, never a fabricated list.
- Acceptance: docs gates green (frontmatter, links, the count ledger
  updated — see the note below); `onto_anchor` receipts for the six terms at
  the derived tier; the reference doc's function inventory carries its
  probe receipts.
- Depends: SP-12's outcome (the reference doc's block-field table changes
  if the field dies); otherwise none.

**Checkpoint:** every documentation claim about the capability is true of
the shipped tree; no evidence gap remains open except by explicit ruling.

## Net summary

**Built:** one canonical display-hint fence and error classifier (Slice 1);
an actionable interrupted-save reconciliation identity and a typed kind
across the ToolInvoker seam (Slice 2); bounded engine residency and a
closed publish window (Slice 3); per the rulings — an inline-table renderer
or a smaller contract (Slice 4); an emitter-agnostic agent instruction, the
plan's missing phase record, derived-tier ontology anchors, and a documented
formula inventory (Slice 5).

**Deleted:** five duplicated functions (`spreadsheet_hint` ×2,
`block_display_hint`, `map_spreadsheet_error` ×3 → one canonical each);
the widget's string-sniffing conflict branch and its hand-rolled fence
strip; the static interrupted label; the unbounded document-residency
contract; the non-atomic metadata write; and per the rulings — the
InlineTable arm (or its dead weight), the `expected_access` field (or its
meaninglessness), the tautological `access` fields.

**Migrated:** nothing — no data migration, no persisted-format change.
Wire shapes change only where the license permits (SP-05/06/12), and every
existing artifact on disk remains readable under the immutable-revision
contract (the Lean pin's INV-5).

**Docs-tree note:** this set adds three research documents to a tree
already over the fewer-than-75 cap (79 measured 2026-10-06, cap ruling
pending per `kask/docs/README.md`). The count ledger must record them, and
the pending condensation ruling should weigh this set against the
scenario-server set it mirrors.
