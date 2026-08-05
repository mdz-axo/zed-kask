# Documentation Base Alignment — TODO Checklist

## Phase 1: Foundation — ✅ COMPLETE

- [x] T1: Fix MDS.md factual errors (hkask-test-harness, hkask-email, hkask-lisp)
  - [x] `hkask-test-harness` not marked as deleted in crate mapping
  - [x] `hkask-email` and `hkask-lisp` added to crate mapping
  - [x] Crate count corrected (19 hkask-* + kask_bridge = 20)
  - [x] Fixed KaskCore contradiction in §7.4
  - [x] Fixed wallet primitives annotation
- [x] T2: Update D1–D14 → D1–D20 references across corpus
  - [x] README.md
  - [x] architecture/zed-host-architecture-plan.md
  - [x] architecture/hkask-types-core-domain-split.md
  - [x] diataxis/INDEX.md
  - [x] diataxis/hkask-templates/explanation.md
  - [x] diataxis/hkask-types/explanation.md
  - [x] diataxis/hkask-types/reference.md
  - [x] diataxis/kask_bridge/explanation.md
  - [x] diataxis/kask_bridge/reference.md
  - [x] explanation/README.md
  - [x] explanation/cognition-and-replica.md
  - [x] explanation/companies-mcp.md
  - [x] explanation/skills-and-composition.md
  - [x] explanation/training-and-adapters.md
  - [x] reference/README.md
  - [x] reference/mcp-servers/condenser.md

## Phase 2: Architecture docs — ✅ COMPLETE

- [x] T3: Audit and align architecture/ docs
  - [x] All 6-field metadata headers valid
  - [x] kask_panel refs fixed (D10 deleted)
  - [x] D1–D20 divergence map updated
  - [x] hkask-test-harness corrected to surviving
  - [x] hkask-email and hkask-lisp added to inventory
  - [x] Stale status values fixed ("Proposed" → "Draft", "Current" → "Active")
  - [x] Citations added (Sourced-Ideas Mandate)

## Phase 3: Diagrams — ✅ COMPLETE

- [x] T4: Verify and fix all diagrams/
  - [x] Every verified_against path exists in codebase
  - [x] DIAGRAM_ALIGNMENT blocks present and current
  - [x] Missing blocks added (4 diagrams: regulation-spans, swarm, media-landscape, design-schema)

## Phase 4: Diataxis docs — ✅ COMPLETE

- [x] T5: Audit and align diataxis/ docs (10 crate sets, 39 files + INDEX)
  - [x] 6-field headers valid
  - [x] D1–D20 references
  - [x] kask_panel → swarm_panel refs fixed
  - [x] STALE diagram statuses fixed
  - [x] Missing metadata header added (hkask-capability/reference.md)
  - [x] INDEX.md updated (10 sets, 39 artifacts)

## Phase 5: Explanation docs — ✅ COMPLETE

- [x] T6: Audit and align explanation/ docs
  - [x] 6-field headers valid
  - [x] D1–D20 references
  - [x] kask_panel refs removed (15 locations)
  - [x] AgentPod stale refs removed
  - [x] Broken links fixed (sovereignty-and-observability, UserPods anchor, magna-carta)
  - [x] DIAGRAM_ALIGNMENT blocks added
  - [x] Citations added (Sourced-Ideas Mandate)
  - [x] CANONICAL_NAMESPACES count corrected (248/243 → 262)

## Phase 6: Plans — ✅ COMPLETE

- [x] T7: Audit plans/ — delete completed/stale
  - [x] Deleted: local-swarm-knowledge-tools.md (fully implemented, covered by Diataxis)
  - [x] Kept 8 plans with metadata fixes
  - [x] Fixed invalid status "Proposed" → "Draft" (semantic-memory-wiki.md)

## Phase 7: QA — ✅ COMPLETE

- [x] T8: Audit qa/ — delete resolved audits
  - [x] Deleted 8 superseded files (phase reports pass1/pass2, pre-release pass1/pass2)
  - [x] Kept 4 active files with metadata fixes
  - [x] Added metadata header to pre-release-final-summary-pass3.md

## Phase 8: Reference — ✅ COMPLETE

- [x] T9: Audit and align reference/ docs
  - [x] 6-field headers valid
  - [x] D1–D20 references
  - [x] kask_panel refs fixed
  - [x] Tool count drift corrected
  - [x] Wallet spans section verified (retained for tracing stability)
  - [x] Citations added (Sourced-Ideas Mandate)

## Phase 9: Research + Status + Audits — ✅ COMPLETE

- [x] T10: Audit research/, status/, audits/
  - [x] Research docs kept (historical snapshots, still relevant)
  - [x] Audit findings verified as unresolved (4 structural items)
  - [x] DIAGRAM_ALIGNMENT blocks added to 4 diagrams

## Phase 10: Index files — ✅ COMPLETE

- [x] T11: Update README.md, DIAGRAMS_INDEX.md, diataxis/INDEX.md
  - [x] README.md: all docs listed, D1–D20, crate count 20, Audits section added
  - [x] DIAGRAMS_INDEX.md: invalid status "living document" → "Active", mds_categories fixed
  - [x] diataxis/INDEX.md: 10 sets, 39 artifacts, kask_panel removed
  - [x] Dangling link fixed (swarm_system/reference.md → local-swarm-knowledge-tools.md)
  - [x] Broken magna-carta link fixed in skills-and-composition.md

## Post-Task: Release notes draft

- [x] Added 6-field metadata header to qa/release-notes-v0.32.0-draft.md