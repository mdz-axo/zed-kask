---
title: "Cybernetic Nervous System — Alignment Plan"
audience: [architects, developers, operators, agents]
last_updated: 2026-09-30
version: "0.3.0"
status: "Active — operator ruling 2026-09-30 ('proceed as proposed — confirmed'): reference model admitted; S2 and S3 approved and landed"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle]
---

# Cybernetic Nervous System — Alignment Plan

Kata steps 3–4 for the three-layer review (operator spec 2026-09-30).
Step 2's current condition lives in `loop-register.md` Pass 3 (naming
survey, mechanism inventory R1–R12, premise verdict, layer coverage,
per-row classifications, alignment gap table). The reference model is
`research/cybernetic-nervous-system-reference-model.md` (draft; admission
pending). This plan states the target condition, ranks the pragmatic
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
| O2 | Surprise-gated reporting governs 1 of 12 pathways; the rest log raw activity | INV3 | The core gap; **S3 landed** (the raw duplicate deleted, net −14); the count changes only with S6 |
| O3 | Expectation carriage absent outside L2 + stored priors | INV2 | Direction only; no code step admitted this pass |
| O4 | B→C handoff receipt deferrals (L1/L7 memory receipt, L9 ack gate) | INV5 | **Ruled design** (operator D3/D4: deliberate loose coupling) — parked, not obstacles |
| O5 | L23 poisoned-lock silent fallback | INV4 | **S4 landed** (2026-09-30: `live_stats_degraded` + the rationale surfacing, red-first) |
| O6 | Scoping 12 pathways into 3 canonical ones | INV1 | Multi-slice program; not this session's unit |

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
12 MCP servers' tool schemas) may change. Where a deletion changes a
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
