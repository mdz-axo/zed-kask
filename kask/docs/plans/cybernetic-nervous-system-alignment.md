---
title: "Cybernetic Nervous System — Alignment Plan"
audience: [architects, developers, operators, agents]
last_updated: 2026-09-30
version: "0.1.0"
status: "Proposed — pending the operator's coaching-kata checkpoint ruling (§7)"
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
| O1 | The tracing→log bridge is undocumented and incidental (rpc's feature flag) | INV6 | Corrected finding; doc step S2 priced |
| O2 | Surprise-gated reporting governs 1 of 12 pathways; the rest log raw activity | INV3 | The core gap; S3 is the smallest instance |
| O3 | Expectation carriage absent outside L2 + stored priors | INV2 | Direction only; no code step admitted this pass |
| O4 | B→C handoff receipt deferrals (L1/L7 memory receipt, L9 ack gate) | INV5 | **Ruled design** (operator D3/D4: deliberate loose coupling) — parked, not obstacles |
| O5 | L23 poisoned-lock silent fallback | INV4 | Standing operator deferral; S4 priced |
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
  events are "process-local diagnostics" only. No production lines.
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
- **S4 — L23 degraded-status contract (operator decision, behavior
  change):** surface the poisoned-lock leg (INV4 repair) — priced in
  L23's standing deferral with its falsifier; not admitted without the
  ruling.
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
