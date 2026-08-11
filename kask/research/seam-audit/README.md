# Kask↔Zed Seam Audit — Engagement 2026-08-11

A single-pass, multi-track audit-and-refactor engagement over the zed-kask
Kask↔Zed seam (DIVERGENCE.md D1–D24), encoded as the reproducible
`kask-seam-audit` FlowDef registry crate.

## Deliverables

| # | Deliverable | Path |
|---|-------------|------|
| 1 | Plan | `tasks/plan.md` |
| 1 | Todo | `tasks/todo.md` |
| 2 | Security review (kali-audit) | `security-review.md` |
| 3 | Refactor-architecture review | `refactor-architecture-review.md` |
| 4 | UI / interaction audit | `ui-interaction-audit.md` |
| 5 | MCDA ranked remediation + sensitivity | `mcda-remediation.md` |
| 6 | FlowDef process manifest | `../../registry/manifests/kask-seam-audit.yaml` |
| 6 | FlowDef templates | `../../registry/templates/kask-seam-audit/` |
| 6 | SKILL.md companion | `../../../../.agents/skills/kask-seam-audit/SKILL.md` |
| 7 | Metacognition per-cycle log | `metacognition-log.md` |

## Headline results

- **Security**: Conditional. 7/8 defense layers covered. Layer 7 (FIDES taint)
  is structurally present but operationally inert (KS-01 `__taint__` markers
  never written + KS-02 all tools hardcoded `Pure`). Primary OCAP+gas membrane
  fail-closed. KS-03: a stale `.rules`/docs reference to the removed
  `propagate_taint_for_binding` (phantom prior).
- **Architecture**: ~2,400 lines of dead surface, concentrated in
  `hkask-templates` (registry trait layer). Top deletions: `hmem::archive`
  (557 lines, test-only), `falsification` module (492 lines, research),
  `SkillLoader` (440 lines, zero callers). Folded `hkask-mcp-corpus` services
  are clean.
- **UI**: blocking finding UI-13 (all 5 viz widgets use raw `div().on_click`
  instead of `Button`/`IconButton`, 18 sites); UI-01 (graph header clips the
  "I disagree" affordance); no `PopoverMenu` anywhere. `KaskExtensionsPage` is
  a positive control (correct `Toggle` + deploy-focus).
- **MCDA**: top cluster KS-01 (0.405) / RA-04 (0.400) / UI-13 (0.398) — a
  near-tie. Top-3 is weight-sensitive (flips under ±20% single-axis emphasis);
  rank-1 stable only under security-emphasis. Single pass → the "two
  consecutive slices stable" criterion is not satisfiable; a re-audit is
  recommended.
- **Remediation**: not applied in-session. Top remediation (KS-01) requires
  taint-bridge surgery + a regression test; dead-code deletions require
  workspace compile verification exceeding a safe single-session bound.
  Remediations are presented as a test-pinning plan in `mcda-remediation.md`.
  No hard-stop triggered.
- **Metacognition**: Brier scores 0.040 (security) / 0.090 (architecture) /
  0.123 (UI) — all well-calibrated (< 0.25 threshold).

## Reproducing

The engagement is reproducible by invoking the `kask-seam-audit` skill (via
the `skill` tool) — the ManifestExecutor runs the FlowDef cascade. The
`lisp.eval` gates (prior exclusivity, finding citation + severity exclusivity,
remediation test-pinning + divergence-surface membership) enforce the
structural invariants between phases. See `tasks/plan.md` for the PDCA trace.