---
title: "Media MCP Server — Redesign Review Proposals"
audience: [operators, developers, architects]
last_updated: 2026-10-07
version: "1.0.0"
date: 2026-10-06
status: "Active"
domain: "Composition"
mds_categories: [composition, domain]
kind: research
related:
  - media-server-redesign-review.md
  - media-server-redesign-findings.md
  - media-server-redesign-improvement-plan.md
---

# Proposal Set — Phase 5

Required fields per proposal: id, component, goal tags, evidence (file:line,
from the findings), BREAKING CHANGES (the path each replaces and what it
deletes — a proposal that only adds is incomplete), effort (XS/S/M/L),
depends. The findings-to-proposals map reconciles via `lisp_eval`: every
finding MF-1…MF-8 appears in exactly one proposal or no-action record
(receipt in the findings doc).

## PR-M1 — Canonical cardinality caps in `media_limits`

- **Component:** `hkask-types/src/media_limits.rs` + the three copying sites
  (`hkask-mcp-media/src/tools/audio.rs` ×2, `src/tools/reduct.rs`).
- **Goal tags:** fidelity (the cardinality-limits contract — "limits import
  from `hkask_types::media_limits`, never copied"), surface-efficiency (one
  source per limit).
- **Evidence:** MF-6 — the 3600.0 capture cap hardcoded at `audio.rs:368`
  and `:412`; the 128 MiB upload cap as a function-local
  `MAX_UPLOAD_BYTES` const at `reduct.rs:1686`; `audio_concat`'s correct
  import pattern (`MAX_CONCAT_ITEMS`, `audio.rs:567`).
- **BREAKING CHANGES:** replaces the two `3600.0` literals and the
  function-local `MAX_UPLOAD_BYTES` const — all three deleted in the same
  change; the error messages derive from the canonical constants (`format!`)
  instead of restating the caps (a message restating a cap is a second copy
  of the cap). No wire change: the values are identical (3600.0 s, 128 MiB).
- **Effort:** XS.
- **Depends:** none.
- **Status:** **EXECUTED 2026-10-06** (uncommitted, rides with the
  operator's batch). Receipts: `MAX_CAPTURE_DURATION_SECS` (f32 — the
  request fields it caps are f32; the first draft as f64 failed compile on
  the comparison and the constant was fixed at its type, not cast at the
  site) and `MAX_REDUCT_UPLOAD_BYTES` (u64) in `media_limits.rs`; all three
  sites import; full-repo sweep found zero other copies of either cap;
  clippy 0 errors; tests green.

## PR-S1 — The transcript-reel skill's realignment step

- **Component:** `.agents/skills/transcript-reel/SKILL.md` (the correction
  step, `SKILL.md:61-68`).
- **Goal tags:** integration (the skill is the educt surface's primary
  consumer), fidelity (the skill should name the re-alignment path the
  transcript discipline now has).
- **Evidence:** the Phase 3 integration table — the skill's correction step
  names `educt_correction_pass` → `educt_apply_corrections` and stops; the
  realignment path after cardinality-changing corrections
  (`educt_realign_transcript`, MF-2, closed `c9a5c3b855`) is absent from the
  process text.
- **BREAKING CHANGES:** none — a skill process-text change; what it replaces
  is the skill's current silence on the unaligned outcome (the implicit
  "unaligned corrections block timed consumers" dead end), replaced by the
  named path. No code path changes.
- **Effort:** XS.
- **Depends:** MF-2 (closed).
- **Status:** **EXECUTED 2026-10-06** (uncommitted, rides with the operator's
  batch). Reclassification basis, recorded: the original filing treated this
  as a skill-design change needing algedonic review; on re-read it is a
  stale-doc correction — the skill's Phase 2 step 3 dead-ended at the
  `unaligned` state, which is active misinformation since MF-2 landed
  (`c9a5c3b855`), the same class as the session's README corrections. The
  operator's proceed directive after the filing accepted it. Vetoable at
  algedonic review. Receipt: the step now names `educt_realign_transcript`
  after an `unaligned` `educt_apply_corrections`, in the skill's terse step
  style; `check-skill-crossrefs.sh` green.

## PR-M2 — Route the collage through canonical storage

- **Component:** `hkask-mcp-media/src/tools/processing.rs`
(`image_create_collage`, `:480-516`) + `src/assets.rs` (a PNG arm in the
local-media publish path, if absent).
- **Goal tags:** fidelity (the Phase 1 Asset primitive — every derived form
gets the storage contract), integration (gallery identity → search,
lineage, panel).
- **Evidence:** MF-13 — the collage writes to OS temp and returns a bare
path; no gallery row, no stable identity, no lineage, no rollback-armed
publication; the only asset-producing tool that bypasses
`publish_local_media`/`persist_slim_and_enrich`.
- **BREAKING CHANGES:** replaces the temp-dir write + bare-path result
(`:480-489`, `:504`) — the temp output and unpersisted path are deleted in
the same change; the result gains gallery identity (id, display hint) and
loses nothing callers were promised (the `output` path survives as the
published asset's path). Result shape changes: adds `image_id`/`gallery_id`
fields.
- **Effort:** S.
- **Depends:** none.
- **Status:** **FILED 2026-10-06 — operator acceptance pending.** Not
executed in-tranche: the result-shape change is a functional change, not a
residue-class correction. Acceptance criteria when executed: the collage
output is gallery-indexed with lineage (op `image_create_collage`),
rollback-armed; a test pins the gallery row + lineage; the temp-dir write
is gone.

## Coverage check

Every finding in exactly one record (reconciled via `lisp_eval` → green):
MF-1/MF-2/MF-4 → closed-condition records; MF-3/MF-5 → operator-decision
records; MF-6 → PR-M1 (executed); MF-7/MF-8 → watched/no-action records
with triggers. PR-M1 and PR-S1 are the only proposals; no finding maps to
more than one record.