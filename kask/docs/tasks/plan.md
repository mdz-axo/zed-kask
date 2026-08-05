---
title: "Documentation Base Alignment Plan"
audience: [agents, maintainers]
last_updated: 2026-08-04
version: "0.1.0"
status: "Active"
domain: "documentation"
mds_categories: [curation]
---

# Documentation Base Alignment Plan

## Overview

Bring the entire `kask/docs/` documentation base (~110 markdown files across 10
directories) into alignment with the current codebase state, the DIVERGENCE.md
divergence surface (D1–D20), and the documentation standards defined in
`DOCUMENTATION_STANDARDS.md` and `MDS.md`.

## Architecture Decisions

1. **Vertical slices by directory** — each directory cluster is an independent
   write scope, enabling parallel execution.
2. **Cross-cutting fixes first** — D1–D14 → D1–D20 reference updates and MDS.md
   factual corrections are foundation tasks that unblock downstream work.
3. **Governing docs are read-only** — `MDS.md` and `DOCUMENTATION_STANDARDS.md`
   are edited only for factual codebase errors, not stylistic changes.
4. **Git history is the archive** — deleted documents are recoverable via
   `git log --diff-filter=D`; no `archive/` directory is maintained.

## Phased Task List

### Phase 1: Foundation (cross-cutting fixes)

| Task | Description | Acceptance Criteria | Verification |
|------|-------------|---------------------|-------------|
| T1 | Fix MDS.md factual errors | `hkask-test-harness` not marked deleted; `hkask-email` and `hkask-lisp` added to crate mapping; crate count corrected | `grep hkask-test-harness MDS.md` shows it as surviving |
| T2 | Update D1–D14 → D1–D20 references across corpus | No file under `kask/docs/` references "D1–D14" without acknowledging D15–D20 | `grep -r "D1.D14\|D1–D14\|D1-D14" kask/docs/` returns zero hits |

### Phase 2: Architecture docs (architecture/)

| Task | Description | Acceptance Criteria | Verification |
|------|-------------|---------------------|-------------|
| T3 | Audit and align architecture/ docs | All 6-field headers valid; no deleted-crate refs as active; diagrams have DIAGRAM_ALIGNMENT | `grep` for stale refs returns zero |

### Phase 3: Diagrams (diagrams/)

| Task | Description | Acceptance Criteria | Verification |
|------|-------------|---------------------|-------------|
| T4 | Verify and fix all diagrams/ | Every `verified_against` path exists in codebase; DIAGRAM_ALIGNMENT blocks present and current | Check each `verified_against` path with `test -f` |

### Phase 4: Diataxis docs (diataxis/)

| Task | Description | Acceptance Criteria | Verification |
|------|-------------|---------------------|-------------|
| T5 | Audit and align diataxis/ docs | 6-field headers valid; D1–D20 refs; no stale code refs; INDEX.md current | Spot-check 3 files per crate set |

### Phase 5: Explanation docs (explanation/)

| Task | Description | Acceptance Criteria | Verification |
|------|-------------|---------------------|-------------|
| T6 | Audit and align explanation/ docs | 6-field headers valid; D1–D20 refs; no deleted-crate refs | `grep` for stale refs returns zero |

### Phase 6: Plans (plans/)

| Task | Description | Acceptance Criteria | Verification |
|------|-------------|---------------------|-------------|
| T7 | Audit plans/ — delete completed/stale | Completed plans git rm'd; unique insights consolidated into Diataxis/explanation docs | No `Status: Deprecated/Superseded` in active tree |

### Phase 7: QA (qa/)

| Task | Description | Acceptance Criteria | Verification |
|------|-------------|---------------------|-------------|
| T8 | Audit qa/ — delete resolved audits | Resolved audit reports git rm'd; unresolved findings consolidated into reference docs | No `Status: Deprecated/Superseded` in active tree |

### Phase 8: Reference (reference/)

| Task | Description | Acceptance Criteria | Verification |
|------|-------------|---------------------|-------------|
| T9 | Audit and align reference/ docs | 6-field headers valid; no deleted-crate refs; wallet spans section current | `grep` for stale refs returns zero |

### Phase 9: Research + Status + Audits

| Task | Description | Acceptance Criteria | Verification |
|------|-------------|---------------------|-------------|
| T10 | Audit research/, status/, audits/ | Stale research deleted; audit findings current or consolidated | No `Status: Deprecated/Superseded` in active tree |

### Phase 10: Index files

| Task | Description | Acceptance Criteria | Verification |
|------|-------------|---------------------|-------------|
| T11 | Update README.md, DIAGRAMS_INDEX.md, diataxis/INDEX.md | All indexes reflect current doc set; no stale entries; valid metadata headers | Index entries match directory listings |

## Checkpoints

- **Checkpoint 1** (after Phase 1): `grep -r "D1.D14\|D1–D14" kask/docs/` returns zero hits; MDS.md crate list matches `kask/crates/` directory.
- **Checkpoint 2** (after Phase 5): All diataxis + architecture + diagrams pass metadata header validation.
- **Checkpoint 3** (after Phase 10): Full acceptance criteria check (10 criteria).

## Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| Breaking internal cross-references when deleting docs | Medium | Check `grep -r <filename>` before deletion; update linking docs |
| Missing stale references that look correct | Medium | Use `grep` for each deleted crate name across all docs |
| Over-deleting plans with unresolved content | High | Read each plan's Status section before deleting; consolidate unique findings |

## Open Questions

1. Which plans in `plans/` are fully implemented vs. still in progress? Need to read each.
2. Which QA audit reports have all findings resolved? Need to read each.
3. Are the `research/media-research/` docs still relevant to the current media system?

## Refinement History

- **Iteration 1**: Initial decomposition from the task specification. No refinement needed yet — the plan is structured by directory cluster with cross-cutting fixes as foundation.