# Documentation Base Alignment — TODO Checklist

## Phase 1: Foundation

- [ ] T1: Fix MDS.md factual errors (hkask-test-harness, hkask-email, hkask-lisp)
  - [ ] `hkask-test-harness` not marked as deleted in crate mapping
  - [ ] `hkask-email` and `hkask-lisp` added to crate mapping
  - [ ] Crate count corrected (19 hkask-* + kask_bridge = 20)
- [ ] T2: Update D1–D14 → D1–D20 references across corpus
  - [ ] README.md
  - [ ] architecture/zed-host-architecture-plan.md
  - [ ] architecture/hkask-types-core-domain-split.md
  - [ ] diataxis/INDEX.md
  - [ ] diataxis/hkask-templates/explanation.md
  - [ ] diataxis/hkask-types/explanation.md
  - [ ] diataxis/hkask-types/reference.md
  - [ ] diataxis/kask_bridge/explanation.md
  - [ ] explanation/README.md
  - [ ] explanation/cognition-and-replica.md
  - [ ] explanation/companies-mcp.md
  - [ ] explanation/skills-and-composition.md
  - [ ] explanation/training-and-adapters.md

## Phase 2: Architecture docs

- [ ] T3: Audit and align architecture/ docs
  - [ ] All 6-field metadata headers valid
  - [ ] No deleted-crate references as active
  - [ ] Diagrams have DIAGRAM_ALIGNMENT blocks
  - [ ] zed-host-architecture-plan.md updated for D1–D20

## Phase 3: Diagrams

- [ ] T4: Verify and fix all diagrams/
  - [ ] Every verified_against path exists in codebase
  - [ ] DIAGRAM_ALIGNMENT blocks present and current
  - [ ] DIAGRAMS_INDEX.md updated

## Phase 4: Diataxis docs

- [ ] T5: Audit and align diataxis/ docs (10 crate sets, ~40 files)
  - [ ] 6-field headers valid
  - [ ] D1–D20 references
  - [ ] No stale code references
  - [ ] INDEX.md current

## Phase 5: Explanation docs

- [ ] T6: Audit and align explanation/ docs
  - [ ] 6-field headers valid
  - [ ] D1–D20 references
  - [ ] No deleted-crate refs

## Phase 6: Plans

- [ ] T7: Audit plans/ — delete completed/stale
  - [ ] Read each plan, determine status
  - [ ] Delete completed plans (git rm)
  - [ ] Consolidate unique insights

## Phase 7: QA

- [ ] T8: Audit qa/ — delete resolved audits
  - [ ] Read each audit, determine if findings resolved
  - [ ] Delete resolved audits (git rm)
  - [ ] Consolidate unresolved findings

## Phase 8: Reference

- [ ] T9: Audit and align reference/ docs
  - [ ] 6-field headers valid
  - [ ] No deleted-crate refs
  - [ ] Wallet spans section current

## Phase 9: Research + Status + Audits

- [ ] T10: Audit research/, status/, audits/
  - [ ] Stale research deleted
  - [ ] Audit findings current

## Phase 10: Index files

- [ ] T11: Update README.md, DIAGRAMS_INDEX.md, diataxis/INDEX.md
  - [ ] All indexes reflect current doc set
  - [ ] No stale entries
  - [ ] Valid metadata headers