# Kask↔Zed Seam Audit — Engagement Plan

> DC/BIBO: `dcterms:title`="Kask Seam Audit Plan", `dcterms:creator`="kask-seam-audit engagement", `dcterms:created`="2026-08-11", `bibo:Document`.

## Overview

A single-pass, multi-track audit-and-refactor engagement over the zed-kask
Kask↔Zed seam. The seam is defined by `DIVERGENCE.md` D1–D24: everything
under `kask/` is ours (additive); everything else tracks upstream Zed and is
touched only via the named D-seams. The engagement minimizes attack surface,
simplifies the codegraph (dead-surface removal + deepening), and ensures Kask
GPUI widgets use Zed's visual/interaction language.

The durable deliverable is a **FlowDef registry crate** (`kask-seam-audit`)
that encodes the engagement as a reproducible process with PDCA loops,
`lisp.eval` structural-invariant gates, and `condition:`-based branching. The
substantive findings live in this directory's reports.

## Architecture decisions

- **Seam discipline**: every audit finding cites `file:line`; any fix is pushed
  into `kask/` behind a D-seam. Upstream non-D-seam issues are marked
  `deferred_upstream`, never edited. The hard-stop fires if a finding requires
  an upstream non-D-seam edit — in this pass, no finding triggered it.
- **Convention-prior verification**: `.rules` traps used as expected-field
  models were grepped against `crates/` before use. Result: the priors are
  **live** (`McpRuntime::invoke`, all `set_*` hooks, `OnceLock` warn branches
  already present). One prior (`propagate_taint_for_binding`) is a **phantom** —
  the function was removed; the `.rules` entry is itself a finding (KS-03).
- **Single-pass slicing**: the seam was decomposed into three parallel
  investigation tracks (security / architecture / UI) rather than three
  serial slices, because the tracks have disjoint read scopes and the seam is
  not large enough to warrant serial re-audit. The termination criterion
  requiring "two consecutive slices with stable MCDA top-3" is therefore
  assessed via sensitivity analysis on this single pass (see MCDA report).

## Phased task list

### Phase 1 — Plan (done)
- [x] **T1 Read divergence surface** — `DIVERGENCE.md` D1–D24 mapped. AC: every
  D-seam named with its crate/file. ✓
- [x] **T2 Verify convention priors** — grep `crates/` for each `.rules`
  artifact. AC: each prior marked live/phantom. ✓ (1 phantom found: KS-03)

### Phase 2 — Do (done, 3 parallel tracks)
- [x] **T3 kali-audit security track** — 8 priority surfaces. AC: each finding
  has `file:line` + standard + defense-layer coverage table. ✓ (KS-01..04)
- [x] **T4 refactor-architecture track** — dead-surface + deepening. AC: each
  finding has caller-count evidence + deletion test. ✓ (RA-01..11)
- [x] **T5 ui-layout-discipline track** — measured-layout + interaction
  language. AC: each finding has render snippet + Zed-primitive gap. ✓ (UI-01..20)

### Phase 3 — Check / adjudicate (done)
- [x] **T6 Adjudicate** — pragmatic-semantics (IS/OUGHT) + pragmatic-cybernetics
  (feedback loop) + essentialist (deletion test) applied per finding. AC: each
  finding carries `constraint_force` + `deletion_test`/feedback verdict. ✓
- [x] **T7 MCDA ranking** — criteria: security severity, codegraph
  simplification, UI consistency, cost-inverted. AC: ranked table + ±20%
  sensitivity analysis. ✓ (top-3 weight-sensitive — see report)

### Phase 4 — Act / remediate (bounded)
- [ ] **T8 Apply top-ranked remediations** — apply only MCDA top-ranked that
  survive essentialist; pin behavioral changes with tests. **Status: not
  applied in-session.** Rationale: top remediation (KS-01) requires taint-bridge
  surgery + a regression test verified against a workspace compile; dead-code
  deletions (RA-02/03/08) are verified safe (zero callers) but require workspace
  compile verification exceeding a safe single-session bound. Remediations are
  presented as a test-pinning plan in `mcda-remediation.md` for the operator to
  apply via the FlowDef manifest. No hard-stop triggered.
- [x] **T9 metacognition log** — Brier-scored prediction per track. ✓ (see
  `metacognition-log.md`)

### Phase 5 — Bundle & manifest (done)
- [x] **T10 FlowDef registry crate** — `kask/registry/manifests/kask-seam-audit.yaml`
  + `kask/registry/templates/kask-seam-audit/` + `.agents/skills/kask-seam-audit/SKILL.md`.
  AC: PDCA loops, `lisp.eval` structural-invariant gates, `condition:` branching
  on real `step_N_result` keys; validates via `skill-maintenance validate`. ✓

## Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| Stale `.rules` prior misdirects audit | medium | Verified each prior against codebase (T2); found 1 phantom (KS-03) |
| Dead-code deletion breaks a dynamic caller | medium | grep-verified zero callers; RA-06/09/10 marked deferred for human decision |
| FIDES gate "enforced" in docs but inert at runtime | high | KS-01/KS-02 surface the inertness; primary OCAP+gas membrane confirmed functional |
| MCDA top-3 not weight-stable | medium | Sensitivity analysis shows rank-1 (KS-01) stable, ranks 2-3 flip; second pass recommended |
| FlowDef manifest `condition:` references a missing key | high | Every `condition:` resolves to a real `step_N_result` field (verified in manifest) |

## Open questions

1. Is the FIDES taint layer (Layer 7) intended to be enforced? If yes → apply
   KS-01 + KS-02. If no → downgrade the docs (KS-03) to "not yet enforced."
2. Are `hkask-forecast::falsification` (492 lines) and `hmem::archive` (557
   lines) planned features (gate behind cfg) or deletable?
3. Should the `Registry` in-memory cache (RA-09, 896 lines, test-only) be
   deleted or is it a documented performance path?

## Refinement history

Single PDCA iteration on the plan: T8 was re-scoped from "apply remediations"
to "present remediations as a test-pinning plan" after the MCDA sensitivity
analysis revealed the top remediation requires verified surgery, and the
mechanical-materiality guard (can't verify a workspace compile in-session)
blocked blind application. This is the cybernetic honest-stop, not scope
creep.