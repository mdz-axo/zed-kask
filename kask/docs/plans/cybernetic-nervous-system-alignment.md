---
title: "Cybernetic Nervous System — Alignment Plan"
audience: [architects, developers, operators, agents]
last_updated: 2026-10-05
version: "0.3.9"
status: "Active — reference model admitted (operator ruling 2026-09-30); S1–S6 recorded; §8 cleanup CU-1–CU-5/CU-7 complete, CU-6 gated on displacement; L25 classified; parking lot fully dispositioned — O3 closed as designed (2026-10-01)"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle]
---

# Cybernetic Nervous System — Alignment Plan

Kata steps 3–4 for the three-layer review (operator spec 2026-09-30).
Step 2's current condition lives in `loop-register.md` Pass 3 (naming
survey, mechanism inventory R1–R12, premise verdict, layer coverage,
per-row classifications, alignment gap table). The reference model is
`research/cybernetic-nervous-system-reference-model.md` (admitted by
operator ruling 2026-09-30). This plan states the target condition, ranks the pragmatic
steps under the subtractive constraint, records the executed PDCA
experiment, and closes with the coaching-kata checkpoint.

## 1. Target condition (measurable)

1. **Every register row** carries a layer classification (primary +
   named secondaries), a sense → report → actuate map, and an
   expectation-signal (INV2) assessment. — **Met by Pass 3** (23/23 rows;
   `loop-register.md` per-row blocks).
2. **One canonical report pathway per layer**, with every other pathway
   either deleted or explicitly scoped as diagnostics in the register's
   inventory. — Current: twelve pathways (R1–R12) serve three layers;
   none is scoped. Measured by the inventory table carrying a
   canonical/diagnostics role per pathway.
3. **The expectation/surprise principle governs the Layer-A report**:
   actuations carry expectations; the report carries the delta; steady
   state coalesces with a counted liveness heartbeat. — Current: INV2
   partial (L2 exemplar + stored priors in L8/L9/L17/L22), INV3 gap (one
   coalesced pathway of twelve).

## 2. Executed experiment (kata step 4 — a closed PDCA cycle)

**Obstacle:** the Phase 1 premise verdict recorded an INFERRED
consequence — that in-host `tracing` events are discarded in standard
builds (no global subscriber; only the cfg-gated Tracy layer).

- **Plan (prediction, recorded before the probe):** the live
  `Zed-Kask.log` contains zed `log` records but no kask tracing output
  (no `hkask.mcp.child` lines, no `reg.tool … REG` lines). Confidence
  0.7. Falsifier: finding such lines refutes the finding.
- **Do:** grepped the live log of the running host
  (`~/.local/share/zed-kask/logs/Zed-Kask.log`, 2,703 lines, 2026-09-30).
- **Check — REFUTED:** 1,744 `REG` / 1,513 `reg.tool` lines present,
  wrapped as `INFO [hkask_mcp::runtime] <child's formatted line>`. The
  prediction failed; the finding must be withdrawn.
- **Act (theory revised, recorded):** the mechanism was traced and the
  register corrected (Pass 3 premise verdict item 1): tracing's `log`
  feature — enabled graph-wide by zed's `crates/rpc/Cargo.toml:35` —
  makes in-host tracing macros emit `log` records when no subscriber is
  active (tracing-0.1.43 `__tracing_log`, `src/lib.rs:1048-1069`), and
  zlog writes them to the log file (`zlog.rs:73-78` prints the module
  path). The corrected finding: the bridge is an **undocumented
  incidental coupling** (no kask doc records the dependency on rpc's
  feature flag; a Tracy subscriber deactivates the fallback).

Learning banked: the live host is the cheapest oracle for report-pathway
questions — grep the running system's log before believing a wiring
inference. An INFERRED consequence is not an IS finding until the running
system confirms it.

## 3. Obstacles parking lot (from the alignment gap table)

| # | Obstacle | Invariant | Status |
| --- | --- | --- | --- |
| O1 | The tracing→log bridge is undocumented and incidental (rpc's feature flag) | INV6 | Corrected finding; **S2 landed** (recorded as designed in regulation-spans.md §1) |
| O2 | Surprise-gated reporting governs 1 of 12 pathways; the rest log raw activity | INV3 | The core gap; **S3 landed** (the raw duplicate deleted, net −14); **S6 recorded** (register v0.24.2: the R1–R12 role table — no displacement candidate; the count stands until a pathway is displaced, CU-6's gate) |
| O3 | Expectation carriage absent outside L2 + stored priors | INV2 | **CLOSED as designed** (operator ruling 2026-10-01): scoped to actuation against a stored reference — the scored loops and the board cards' deficit/threshold pair hold it; the event-record pathways (R1–R4, R9, R12) stay expectation-free by design (ceremony with no scoring consumer fails the deletion test). Records-only: the INV2 gap-table verdict and the reference model's INV2 scope note carry the ruling |
| O4 | B→C handoff receipt deferrals (L1/L7 memory receipt, L9 ack gate) | INV5 | **Ruled design** (operator D3/D4: deliberate loose coupling) — parked, not obstacles |
| O5 | L23 poisoned-lock silent fallback | INV4 | **S4 landed** (2026-09-30: `live_stats_degraded` + the rationale surfacing, red-first) |
| O6 | Scoping 12 pathways into 3 canonical ones | INV1 | **S6 recorded** (register v0.24.2: three canonical, eight diagnostics, one substrate — no-candidate); the deletion arm is CU-6, gated on an actual displacement |

**Focus obstacle: O2.** It is the target condition's core (clause 3),
has the smallest verifiable instance (S3), and the in-tree precedent
(R10's coalescer) already defines the shape to generalize.

## 4. Ranked pragmatic steps (subtractive constraint)

Each step names what it replaces and deletes. Code-touching steps pass
the deletion test, preserve behavior, and validate (`./script/clippy`,
`cargo check -p zed`) before any green claim.

- **S1 — DONE this pass (no code):** the live-log adjudication (§2).
  Replaces: the INFERRED drop. Deletes: nothing (evidence-gathering).
- **S2 — record the bridge as a designed pathway (operator decision,
  ~10 doc lines):** a note in `reference/regulation-spans.md` §1 (and,
  if the operator prefers the seam ledger, a DIVERGENCE.md entry) stating
  that in-host tracing visibility depends on the `log` feature enabled
  by `crates/rpc/Cargo.toml:35`, that zlog prints the module path, and
  that a Tracy build moves these lines to Tracy. Replaces: the
  undocumented coupling. Deletes: the false implication that tracing
  "process-local diagnostics" only. No production lines.
  **DONE (2026-09-30, operator-approved):** landed in
  `reference/regulation-spans.md` §1 (v0.42.0; +21/−2 doc lines) — the
  undocumented-coupling finding closed by record.
- **S3 — delete the duplicative raw-activity emission (code, ~−16
  production lines):** the `tracing::debug!` quality-metrics block at
  `hkask-regulation/src/cybernetics_loop.rs:905-920` fires every tick and
  duplicates the fields the coalesced `LoopMetricsTelemetry` span
  observation carries (`:940-960`, emitted only on change/heartbeat).
  Replaces: the per-tick raw copy with the existing surprise-gated form
  (the INV3 direction). Deletes: ~16 production lines of raw-activity
  logging. Deletion test: the complexity (a second, uncoalesced emission
  of the same metrics) vanishes; no caller consumes the debug form
  (falsifier: a consumer of that debug format in code, docs, or the log
  analysis). Behavior preserved: the durable span is unchanged; the debug
  line's only destinations were the fallback log (duplicating the span)
  and Tracy. Validate: `./script/clippy -p hkask-regulation`,
  `bash kask/scripts/cargo-test-nonzero.sh -p hkask-regulation --lib`,
  `cargo check -p zed`, and a live-log before/after check that
  `reg.outcome.loop_quality` spans still appear.
  **DONE (2026-09-30, operator-approved):** the block (pre-deletion
  `cybernetics_loop.rs:913-925`) is deleted — **net −14 production
  lines** (0 insertions). Falsifier pre-check: zero consumers of the
  debug format (the only remaining occurrence is the `SpanKind` doc
  comment, which documents the span) and 0 occurrences in the live log —
  zlog filters debug, so the deleted emission was inert output; the
  Tracy-build destination is the only behavior delta, as priced.
  Receipts: rustfmt --check clean; hkask-regulation --lib **99/99** via
  cargo-test-nonzero (the suite grew from 96 with concurrent additions);
  scoped `./script/clippy` clean (machete + buf included);
  `cargo check -p zed` passed.
- **S4 — L23 degraded-status contract (operator decision, behavior
  change):** surface the poisoned-lock leg (INV4 repair) — priced in
  L23's standing deferral with its falsifier; not admitted without the
  ruling.
  **DONE (2026-09-30, the operator's proceed instruction; red-first, two
  tdd cycles):** `score_providers` no longer drops zero-penalty
  rationales (the poisoned arm's message now reaches every
  recommendation), and `ProviderRecommendation.live_stats_degraded`
  (serialized only when `true`, so healthy responses are unchanged)
  distinguishes a broken channel from thin samples. Pins:
  `poisoned_performance_channel_surfaces_the_degradation_in_every_recommendation`
  (observed RED: the rationale carried only "not configured (no API
  key)" — the exact falsifier) and
  `live_stats_degraded_distinguishes_poisoned_channel_from_thin_samples`.
  Receipts: 145/145 crate tests, rustfmt clean, scoped clippy clean.
  Source landed in the concurrent stream's `49b5f1518a`; the test-file
  half (two stub initializers) landed as `dfc2f292d0` after `49b5f1518a`
  left the `tool_behavior` target uncompilable at HEAD.
- **S5 — ruled design, no step:** the O4 receipt deferrals stand per
  operator rulings D3/D4 (2026-09-29).
- **S6 — pathway scoping (direction, multi-slice):** classify R1–R12
  into the three canonical pathways (A: RegulationRecord archive + R10
  telemetry; B: in-thread results + toasts; C: curator memory), deleting
  or explicitly scoping the rest — one pathway per slice, deletion test
  per pathway. Not this session's unit; named so the target condition's
  clause 2 has a path.
  **DONE (2026-09-30, register v0.24.2):** the R1–R12 role table
  recorded — three canonical pathways (A: the R5 archive with R10's
  producer and R6's escalation arm; B: the envelope + R7 toasts on
  R11's substrate; C: R8 memory with R12 work-state), eight
  diagnostics pathways with explicit roles, one substrate; the
  no-candidate finding stands (the one true duplication was S3,
  already deleted). The deletion arm is CU-6, gated on an actual
  pathway displacement.

**Net production lines this pass: zero changed** — the pass is a review
(doc-only: the register, the reference-model draft, this plan, and the
docs README rows). The no-candidate finding: no code-touching step was
admitted; the one deletion candidate found (S3) is priced above and
gated on the checkpoint ruling, not landed.

## 5. The next experiment (selected, gated on the ruling)

**S3** — the kata PDCA unit for the focus obstacle O2.

- **Prediction:** deleting the per-tick debug block leaves the coalesced
  `reg.outcome.loop_quality` spans unchanged in the live log (they are
  emitted by the separate span path), the crate's tests pass unchanged,
  and net production lines decrease (~−16). Confidence 0.7.
- **Measurement:** the four validation commands above + a before/after
  live-log diff (the span lines persist; the per-tick raw lines, if any
  were passing zlog's filter, disappear).
- **Success criterion:** all validations green, net negative lines, span
  path observably unchanged.
- **When to check:** one session — the slice is a single-file edit with
  an existing test surface (hkask-regulation --lib, 96 tests at the
  last recorded run).

**Observed (2026-09-30, landed same session):** all three prediction
components held — the coalesced span path is untouched (the telemetry
pins passed within 99/99); net production lines −14 (predicted ~−16);
the live-log falsifier pre-check found the debug format already absent
from the log (zlog filters debug), so the observable log is unchanged
and the deletion removes inert output. The coalescer region's register
citations were re-measured to `cybernetics_loop.rs:895-968` in the same
change.

## 6. What this plan is NOT

- Not an authorization to refactor — every code-touching step is gated on
  the operator's ruling at the checkpoint; the plan proposes, the
  operator disposes.
- Not a claim that the reference model is admitted — that ruling is
  separate (the paper's §7).
- Not a re-adjudication of ruled design — O4 stays parked per the
  recorded rulings.

## 7. Coaching-kata checkpoint (operator ruling requested)

The five questions, with this pass's answers, presented for the ruling:

1. **What are you trying to learn?** Whether the register's 23 loops can
   be governed as one three-layer nervous system — one canonical
   pathway per layer, expectation-carried actuation, surprise-gated
   reporting — and which single subtractive step moves the tree closest
   next.
2. **What can you see now?** The tree's own vocabulary already names the
   substrate (`Sensor`, `set_point`, `Deviation`, `advisory`, `algedonic`,
   `RegulationRecord`); the expectation signal exists in exactly one
   loop (L2); one of twelve report pathways is surprise-gated (R10); the
   tracing→log bridge works but is undocumented and incidental; every
   row is now classified with per-row maps and INV1–INV6 verdicts.
3. **What is blocking you?** Three operator decisions: (a) admit the
   reference-model draft (paper §7); (b) S2 — record the bridge as a
   designed pathway; (c) S3 — approve the ~−16-line deletion experiment
   (or reject it and name the next obstacle). S4 (L23) remains priced
   behind its standing deferral.
4. **What is your next step?** If S3 is approved: land the single-file
   deletion with its four validations and the live-log before/after
   check, then re-read the gap table's INV3 row. If rejected: the
   parking lot's next obstacle is O1/S2 (the doc note), which needs no
   code.
5. **When can we see results?** S3 is one session (single file, existing
   test surface); its effect is visible in the same session's clippy,
   tests, and the next launch's log. S2 is one doc edit, visible
   immediately. The reference-model admission is a one-word ruling.

Requested ruling: approve/modify/reject S3; approve/modify S2; admit or
keep the reference-model draft as unanchored; and score the goal's
ground truth when you confirm the deliverables.

**Ruling (2026-09-30): "proceed as proposed — confirmed".** The
reference model is ADMITTED as-is; S2 and S3 approved and landed the
same session (receipts in §4–§5); S4 stays priced behind its standing
deferral; the goal's ground truth was confirmed by the operator — the
0.75 intake prediction Brier-scored at 0.0625.

*(Superseded later the same day: the operator's proceed instruction
executed S4 — see §4 — and directed the §8 cleanup program below.)*

## 8. Legacy and orphan cleanup — strangler-fig removal (operator direction 2026-09-30)

**No backward-compatibility requirement** (operator, 2026-09-30,
reaffirming the repair plan's standing rule): deletions are outright —
no compatibility shims, no `#[deprecated]` attributes, no
kept-for-compatibility states, no migration paths. Wire formats (the
13 MCP servers' tool schemas) may change. Where a deletion changes a
persisted shape, the slice names the consequence and the operator
rules on migrate-vs-recreate before it lands.

**Governing discipline** (existing rules): deletions clean up what they
orphan in the same change — the dep line, the h_mem's embeddings and
memory_links, the settings knobs, the doc sections; every removal
ends with a full-repo symbol sweep over code AND docs plus a full
build before any green claim; MCP servers are leaf crates (only truly
unused items — zero references anywhere including `tests/` — are dead;
test seams are alive; `pub`→`pub(crate)` tightening is churn); stale
comments describing deleted behavior are updated in the same change.

Tasks (each code-touching slice: deletion test, existing-suite
behavior preservation, `./script/clippy` + crate tests + `cargo check
-p zed` where zed-side, net-lines accounting):

- **CU-1 — Dead-surface sweep.** One-impl traits and convention
  helpers: grep `self.<field>`/call sites; constructor-only matches are
  unwired → delete trait + impl + its tests together. Deliverable: a
  findings ledger (file:line, IS), each candidate deleted or rejected
  with reason; net production lines decrease or the no-candidate
  finding stated with evidence.
- **CU-2 — Dependency sweep.** `cargo machete` (kask-scoped) plus the
  deps orphaned by CU-1: zero `use <dep>` hits in `src/` AND `tests/`
  → remove the dep line.
- **CU-3 — Advertised-invariant sweep.** Doc comments claiming gates,
  audits, or migrations must point to their enforcement line or say
  "not yet enforced"; "kept for compatibility" wording dies with its
  dead claim.
- **CU-4 — Stale-comment sweep.** Rides every slice: grep comments
  naming the deleted behavior; update or delete in the same change.
- **CU-5 — Orphaned-artifact sweep.** (a) run
  `delete_orphaned_embeddings` after memory-touching slices; (b) probe
  and test artifacts out of production trees; (c) the docs corpus at
  74 files vs the 60-file target — a doc-update realignment pass over
  the condensation candidates (the register's superseded historical
  ledger sections first), under the fewer-than-75 gate.
- **CU-6 — Strangler-fig completion.** Where the S6 scoping shows a
  canonical pathway carrying a diagnostics-only duplicate's load,
  delete the old form only after the canonical path verifiably carries
  it; one pathway per slice; no compat state survives. S3 was the
  first instance; the S6 table names the rest (currently: none — the
  no-candidate finding stands until a pathway is displaced).
- **CU-7 — Legacy-mode verification.** Named candidates — cite the
  production consumer or delete the mode: the standalone-launch
  tracing subscriber
  (`hkask-services-core/src/standalone_settings.rs:391`); any
  `HKASK_USE_*` opt-in without a recorded consumer; settings knobs
  whose capability was deleted.
- **CU-8 — Specification and doc updates ride with every slice.** The
  owning reference/per-server doc, DIVERGENCE.md (a zed-side deletion
  retires or amends its D-seam entry), the register's inventory, the
  docs README, and the diagram registry — in the same change.

Sequencing: CU-1 → CU-2 (deps orphaned by CU-1); CU-3/CU-4/CU-8 ride
every slice; CU-5 after memory- and doc-touching slices; CU-6 gated on
an actual displacement; CU-7 independent, smallest first.

### Execution record — first slice (2026-09-30: CU-1 + CU-2 + CU-7's named candidates)

- **CU-1, all four instruments run:** (a) the `#[allow(dead_code)]`
  baseline — one kask-scoped site
  (`hkask-types/src/tool_schema.rs:259`), a documented test fixture for
  the JsonSchema derive — REJECTED with reason (deleting it deletes the
  test); (b) one-impl traits — 32 traits defined in kask production
  `src/`, zero with ≤1 impl and ≤1 use — NO CANDIDATE; (c) pub and
  `pub(crate)` fns — 1,479 swept, one real candidate:
  `hkask-storage/src/core/connection.rs:349`
  `Database::in_memory_with_extensions` (zero references repo-wide,
  including tests and docs) — **DELETED** together with the
  `in_memory_impl` helper its deletion orphaned (inlined into
  `in_memory()`), net **−8 production lines** (+2/−10); one artifact
  rejected (`connectedne` — a comment fragment, not a function); (d)
  pub types — 1,426 swept case-corrected, zero with one occurrence — NO
  CANDIDATE.
- **CU-2:** `cargo machete` — zero kask-scoped findings (the only
  flags are upstream crates, out of scope per the machete-scoping
  rule); no deps orphaned by the CU-1 deletion. NO CANDIDATE, with
  evidence.
- **CU-7's named candidates:** (a) the standalone-launch subscriber
  (`standalone_settings.rs:391`) — REJECTED: a test fixture inside
  `malformed_settings_surface_fallback`, in a live module with cited
  consumers (the corpus server's settings load,
  `compose_tools.rs:23`, `hkask_mcp_corpus.rs:66`); (b)
  `HKASK_USE_*` opt-ins — one in the tree, `HKASK_USE_FAL_DOCRES`, and
  it is a removal guard pin (`mcp_servers.rs:1421-1422`), not an
  opt-in — REJECTED; (c) settings knobs — the pub settings fields
  swept, zero with one occurrence — NO CANDIDATE (a full
  nested-settings audit remains available as a CU-7 continuation).
- **CU-3/CU-4 at slice scope:** the touched region's comments verified
  current; no stale comment referenced the deleted constructors.
- **Receipts:** hkask-storage --lib 72/72; rustfmt clean; scoped
  `./script/clippy` clean (machete + buf included); `cargo check -p
  zed` passed; full-repo symbol sweep over code AND docs clean for
  both removed identifiers. Running §8 production ledger: **−8**.

### Execution record — second slice (2026-09-30: CU-3 + CU-4, the advertised-invariant sweep)

Verified against the current tree throughout — past audits are context,
not verdict (operator calibration, 2026-09-30).

- **The load-bearing advertised invariants (7 named-enforcement claims)
  all verified with cited enforcement lines:** `write_turn`
  (`kask_bridge/src/memory/ingest.rs:277`, claimed at
  `hkask-types/src/ports/memory_port.rs:56`); `validate_board_name`
  (`kata-kanban/src/kanban/service_impl/service.rs:124`, claimed at
  `kanban_wire.rs:28`); the >=8 passphrase minimum
  (`hkask-storage/src/core/connection.rs:250`, `rotation.rs:125`,
  claimed at `hkask-keystore/src/passphrase.rs:14`); the legacy
  `declared_method.threshold` rejection
  (`hkask-mcp-corpus/src/compose.rs:214-224`, a typed BadRequest naming
  the remediation, claimed at `salience.rs:438`); the never-untagged
  ladder (the total rung chain, `axis.rs:184-196`); the `busy_timeout`
  ordering (enforced by the `WAL_PRAGMA_BATCH` constant itself,
  `sqlite.rs:16-24`); drop-safety for the forgetting-spec migration (the
  `Immediate` transaction, `connection.rs:384-395`).
- **One actionable finding, landed:** the `tool_schema` backward-compat
  re-export — `hkask-mcp-server` re-exported
  `AnyJsonValue`/`find_boolean_schema_positions` "for backward
  compatibility" (`hkask_mcp_server.rs:29-35`) while all 8 servers
  already depend on `hkask-types`, whose root re-export is the shorter
  canonical path. **DELETED** (the shim + its comment); 16 import sites
  across 8 servers repointed to `hkask_types::`; the `tool_schema.rs`
  doc paragraph repaired (the compat sentence died; its broken
  grammar fixed). Net **−10 production lines** (+21/−31).
- **Rejected with reason:** the salience legacy `threshold` field (a
  live rejection gate — deleting it would make legacy declarations
  silently ignored instead of clearly rejected); the `actions.rs`
  "backward-compatible" encoding claim (a serde format property — the
  tag attribute is inline — not a code shim); the data-state "legacy
  row" handling (migration windows with tests).
- **`#[deprecated]`: zero in kask — the hard rule holds.**
- **Receipts:** cargo check on all 10 affected crates clean; **1340
  tests green** across 36 result lines; rustfmt clean; scoped
  `./script/clippy` clean (machete + buf included); `cargo check -p
  zed` passed; residue grep for the re-export path clean. Running §8
  production ledger: **−18** (−8 first slice, −10 this slice).

### Execution record — third slice (2026-09-30: CU-5, the docs condensation, register leg)

- **The register (the named first candidate) condensed: 1,882 → 1,509
  lines (−373).** The superseded pass-1/pass-2 process narrative (the
  pass-2 checkpoint's process subsections, the Phase 4 partial
  ledger, the closure's scored-predictions/AC6/gap-ledger/acceptance
  subsections, and the executed pass-1 INVEST worklist) is deleted
  with successors named (the rows, the per-server reference docs, the
  change log, Pass 3's INV gap table; git history the archive).
  **Retained:** the reference-model anchor ledger (the admitted model
  cites it), the lessons with the operator's recording-note
  correction, the D1–D12 dispositions, the F1–F6 records, and every
  current row.
- **Whole-file candidates adjudicated:** the LogiSheets plan — KEPT
  (Active with open phases 6–7; verified, not assumed); the repair
  plan — EXCLUDED (the concurrent stream is actively editing it); the
  aeneas trio (the Proposed plan + two evidence files) — **operator
  proposal:** a blocked upstream program (Aeneas issue #838) whose
  records are retained-for-decision; deleting all three (−3 files,
  74 → 71) is a one-word ruling — the plan's own summary paragraph
  carries the durable facts, git history the detail.
  **RULING (2026-09-30): delete.** Executed same session: the three
  files removed (`git rm`), the README lifecycle ledger carries the
  tombstone with successors, `DOCUMENTATION_STANDARDS` §258's example
  row updated, corpus 74 → **71**. The gates re-run green.

### Execution record — fourth slice (2026-09-30: CU-7 continuation, the zed-side settings sweep — CU-7 COMPLETE)

- **The instrument:** the 72 `pub` fields of
  `kask_bridge/src/settings.rs` (`KaskSettings` + its subsections)
  counted against a fresh repo-wide word corpus (rebuilt after the
  experimentation server landed — the tree moved mid-sweep, per the
  operator's calibration).
- **No-candidate, with evidence:** zero fields at ≤2 occurrences (both
  definition sites, no readers) and zero at 3 — every knob has ≥4
  occurrences (the settings-struct definition, the Content mirror, and
  readers across `mcp_env()`, the settings UI, and the consumers).
- **The mirror is drift-free:** the 72 `Content` fields in
  `crates/settings_content/src/settings_content.rs:1650+` correspond
  1:1 with the settings fields — zero orphaned deserialization knobs,
  zero unsettable settings.
- **CU-7 is complete:** all named candidate classes verified — the
  standalone subscriber (a test fixture in a live module), the
  `HKASK_USE_*` opt-ins (one, a removal guard), and the settings knobs
  (both surfaces: `HkaskSettings` swept in the first slice,
  `KaskSettings` here). No dead knob exists in either settings
  surface.
- **Observed, not acted (2026-09-30; since closed):** the concurrent
  stream's experimentation server landed with its registry row (6 tools,
  pinned) — its per-server detail doc did not exist yet. **Closed by
  register v0.24.7 (2026-09-30):** `reference/mcp-servers/experimentation.md`
  created, with the docs-README row and the credential-declaration
  repair in the same change.

### Code-facing naming proposal (the expectation-signal category; closes the reference model §6/§7-4 cross-reference)

The admitted model's expectation-signal category needs code-facing
names chosen against the Phase 1 surveyed vocabulary — no new
coinages. Proposal:

- **The expectation carrier**: reuse the surveyed `set_point` name
  (`hkask-regulation/src/loops/signals.rs` — `Signal { value, set_point }`)
  as the canonical field name for a stored expectation on any record
  that carries one; the delta remains `Deviation` where the regulation
  vocabulary already applies, and `expected`/`observed` as plain field
  names elsewhere (the L23 repair's `live_stats_degraded` pattern:
  plain, self-describing fields over new types).
- **The surprise gate**: no new name — a pathway is surprise-gated when
  its report carries the delta and coalesces steady state (the R10
  precedent, `cybernetics_loop.rs:895-968`).
- **No new types**: the category is a naming convention over the
  existing vocabulary (`set_point`, `Deviation`, `expected`,
  `observed`, `degraded`), not a new abstraction — the deletion test
  holds by construction (nothing is added).
- **CU-5(b) probe artifacts: clean** (no probe/scratch/tmp/bak files
  in production trees).
- **Gates (at this slice, 2026-09-30):** count 74 under the 75 cap
  (currency 2026-10-01: **72** — the aeneas deletion −3, the experimentation
  per-server doc +1; register v0.24.6/v0.24.7). The 60-file target
  remains the direction — the file-count work needs the whole-file
  rulings;
  links, citations, frontmatter, and no-deleted-surfaces re-checked
  (the README's register row updated to drop the stale INVEST
  emphasis).

### Execution record — fifth slice (2026-10-01: plan-record reconciliation — CU-8 applied to this plan)

Records-only; no code, no corpus change. The §8 slices landed their
work but left this plan's own records behind it: (a) §4's S6 entry and
§3's O2/O6 statuses said "not this session's unit" while the S6
record existed in the register (v0.24.2) — pointers added; (b) the
fourth slice's "per-server detail doc does not exist yet" note was
overtaken by the v0.24.7 repair — closed with the pointer; (c) the
gates line carried the stale 74 count — dated and corrected to 72;
(d) the intro still said the reference model was "draft; admission
pending" — contradicted by this plan's own §7 ruling record —
corrected to the admitted state; (e) the frontmatter status line
predated S4, §8, and L25 — refreshed. Open surface after this slice:
O3 (INV2 direction) awaits an operator decision; the 60-file docs
target awaits whole-file rulings; CU-6 stays gated; the L25 row's
`store.rs:707` citation is commit-anchored at `c57e1706db` and
refreshes when the concurrent stream's in-flight experimentation work
lands.

### Execution record — sixth slice (2026-10-01: CU-5(c) continuation — the whole-file pricing pass; a ruling menu, no deletions)

Method: line counts, frontmatter statuses (all 72 claim Active —
status does not discriminate), the README lifecycle ledger (seven
prior condensation rounds), and a corrected repo-wide
inbound-reference matrix over every basename (the first sweep's
exclusion filtered inbound references too — the `.rules`
under-match class, caught when the region-routing SKILL.md citation
surfaced; re-run counting files that reference each basename,
excluding only the file itself).

**No-candidate finding (pure orphans): ZERO.** Every file carries at
least one inbound reference and a role; the corpus is fully
cross-referenced after seven rounds. The 60-file target is not
reachable by orphan deletion.

**The ruling menu:**

| # | Candidate | Price | Successor | Assessment |
| --- | --- | --- | --- | --- |
| P1 | Fold `research/navigating-the-region-space-collaboration.md` (344 ln) into its sibling `research/syntax-semantic-probabilistic-deterministic-space.md` | −1 file, ~−305 corpus lines | the sibling (its only inbound reference, `:170`); the reified machinery lives in the `region-routing` SKILL.md and the skills README row; git history archives the session narrative (the 2026-09-23 fold pattern) | **recommended** — low risk; the skill cites the sibling, not this doc |
| P2 | Delete `research/media-server-lead-onboarding.md` (253 ln) | −1 file | `reference/mcp-servers/media.md` + the media diataxis set (the grounded inventory and architecture content is duplicated there); git history | **executed 2026-10-05** (docs deep-alignment run) on role-closure evidence: zero live role presence repo-wide, zero media-crate commits since 2026-09-25, all five §6 focus areas untouched; vetoable by single-commit revert |
| P3 | Fold the 8 diataxis `explanation.md` files into their set references (condenser 102, inference 112, mcp-server 135, regulation 187, storage 156, types 155, kask_bridge 143, swarm_system 148 ln) | −8 files | the set references' opening sections (the hkask-tool-port precedent, 2026-09-28) | **priced, recommended against** — the explanations are substantial, and the fold dissolves the Diataxis explanation layer the standards prescribe; an OUGHT taxonomy change, not condensation |

**Gap findings (additions the per-server discipline requires):**
`reference/mcp-servers/curator.md` and
`reference/mcp-servers/training.md` do not exist — 2 of the 13 live
servers are undocumented against the fleet's per-server discipline
(the gap `experimentation.md` closed on 2026-09-30). Ruling these adds +2
files.

**Target math (honest):** P1 → 71. P1+P2 → 70. With the gap docs →
72. P1+P2+P3+gaps → 64. **The 60-file target is not reachable
without P3** (the explanation-layer dissolution, recommended
against); the steady-state floor under current standards is ~70–72.
Options: re-rule the target to the measured floor, rule P3 to
approach 60, or keep 60 as a standing direction.

**README map repairs landed in this slice (CU-8):** the
loop-register row (23 → 25 loops), the alignment-plan row (the
current program state), the DIAGRAMS_INDEX row (105 → 107 records,
71 → 73 inline), the README frontmatter (last_updated 2026-09-28 →
2026-10-01 — a version-drift catch), and the three missing map rows
(`architecture/compaction-pipeline-spec.md` — the ratified 2026-09-29
spec; the two region-space research docs — one being the
region-routing skill's reference model, absent from the map until
now).

**Ruling (2026-10-01):** P1 APPROVED and executed — the fold landed
(sibling §4 gained the overlay subsection; the probe citations
repointed; README tombstone recorded; corpus 72 → 71). P2 REJECTED by
operator ruling ("leave the media file alone" — the role document
stays). P3 stands as priced, recommended against, unrulled. The gap
docs (curator, training) are deferred per the operator's direction to
focus on code work. **Closed 2026-10-01** (the operator's proceed ruling,
after the code focus ran to exhaustion): both per-server docs created
(`reference/mcp-servers/curator.md`, `reference/mcp-servers/training.md`);
corpus 72 → 74. The 60-file target remains a standing direction;
the measured floor moves with the discipline-required docs.

### Execution record — seventh slice (2026-10-01: CU-1 continuation — the legacy metric vocabulary; found by the closure test)

The v0.25.0 closure test's allowlist was the finding instrument: the
four legacy Loop-6 metrics (`EnergyRemaining`, `ErrorRate`,
`ConnectorLatency`, `CommunicationQueueDepth`) had zero production
emission sites, no rules, no set-points — dead vocabulary. Deleted
with their impl arms, the strategy evaluator's inert seeds, and the
rollout bridge's two unreachable string arms (one a naming lie —
"energy_remaining" extracted token usage). Net **−356 lines**
(+38/−394, six files). Decode safety verified: both `from_str_name`
production callers warn-and-skip unknown names. The concurrent
stream absorbed the working-tree deletion into its `301c5a29d5`
(alongside its selection-race transaction); the set verified intact
at HEAD and the battery re-run green there (104/104, 258/258, 13/13,
clippy, `cargo check -p zed`). Register v0.25.1 carries the full
record.
