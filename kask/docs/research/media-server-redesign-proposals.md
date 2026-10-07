---
title: "Media MCP Server — Redesign Review Proposals"
audience: [operators, developers, architects]
date: 2026-10-06
status: "Active"
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
- **Status:** **FILED — operator review.** Skills are the operator's
  algedonic-review surface; the line lands on the operator's ruling, not
  unilaterally. The proposed addition: after `educt_apply_corrections`
  returns `unaligned`, call `educt_realign_transcript` to store the
  realignment layer, then re-verify with `educt_locate`.

## Coverage check

Every finding in exactly one record (reconciled via `lisp_eval` → green):
MF-1/MF-2/MF-4 → closed-condition records; MF-3/MF-5 → operator-decision
records; MF-6 → PR-M1 (executed); MF-7/MF-8 → watched/no-action records
with triggers. PR-M1 and PR-S1 are the only proposals; no finding maps to
more than one record.