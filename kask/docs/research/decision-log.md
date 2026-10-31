---
title: "Decision Log — Load-Bearing Cross-Cutting Decisions"
audience: [architects, agents, operators]
last_updated: 2026-10-31
version: "0.1.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [lifecycle, trust, composition]
---

# Decision Log — Load-Bearing Cross-Cutting Decisions

A rolling, append-only index of decisions that are **hard to reverse,
surprising without context, and the result of a real trade-off** (the
refactor-architecture skill's ADR criterion). One entry per decision;
entries are never edited after landing — a superseded decision gets a new
entry pointing back.

**What belongs here:** cross-cutting decisions with no single governing
artifact. Decisions that live naturally inside a seam, an eval protocol, or
a design doc stay there and get only a one-line index entry here pointing at
the full record (this log is an index, not a second home — the record is
the authority, the entry is the findability).

**Entry shape:** date — title — the decision in one line — the full record
(commit hash and/or artifact path).

## Entries

- **2026-10-31 — The divergence register is current-state-only.** DIVERGENCE.md carries current state and functional purpose; dated history lives in git. Full record: commit `b6c81f2d8b` (the D27/D66 row deletions, the follow-up-ruling fold, the sync-history removal); the preamble states the content standard.
- **2026-10-31 — The panels are registered seams.** `crates/kanban_panel` and `crates/media_panel` (with `hkask-tool-invoker` and `marketplace_ui_common`) are divergences with their own rows (D97/D98); `remote_server_download` registered under D7. Full record: commit `b6c81f2d8b`; rows in DIVERGENCE.md.
- **2026-10-31 — The PM/TPM four-moves interaction contract retired from the system prompt.** It never produced functional-term communication — the agent kept reporting in technical register while holding technical responsibility, the exact outcome the framing existed to prevent. Survivors: the role-free Task Execution ask-trigger and the D58 goal-tool conditional (`## Goal lifecycle`). Full record: commit `a0116e6f0c`; the D40 entry in DIVERGENCE.md's retired list.
- **2026-10-31 — The trust clause lands as ambient policy at a recorded boundary.** Binding 1 renders as `## Trust and expectations`; it is a standing condition of every future measurement arm, not a treatment variable — isolating its effect requires a new pre-registration (clause-off arm). Full record (the ADR): `kask/eval/register-adherence-v1/PROTOCOL.md`; commit `9fc2639ca1`.
- **2026-10-31 — The rules trust objective states the anchored account.** The dynamics bullet cites what `hkask-trust` prices (+4·w / −9·w, the 2.25 asymmetry, the 600-point scale, the Beta(1,1) prior, the multiplicative cliff) instead of the superseded pre-anchoring magnitudes. Full record: commit `c979044ef2`; the anchored constants in `kask/crates/hkask-trust/src/hkask_trust.rs`.
- **2026-10-31 — Decision records live with the artifact they govern; this log indexes them.** Operator ruling: a single rolling log (this file) over an ADR directory. Full record: this entry.
