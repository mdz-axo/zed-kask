# Todo — Upstream-Zed Removal Principles

## Phase 1 — Category slices
- [x] C1 Install/runtime collision: decision test on `check-desktop-no-collision.sh` + `check-zed-isolation.sh`; failure mode = Zed hijack; anchor `.rules:787-821`, D7 L28, D16 L37
- [x] C2 Platform scope: test = `#[cfg(target_os)]`-gated non-Linux / non-Linux bundler absent from build matrix; negative test excludes cross-platform libs; anchor D7 L28
- [x] C3 Redundant surface superseded by kask: two-part test (wired replacement + concrete defect); anchor D1 L22, D3 L24, D10 L31, `.rules:602-631`
- [x] C4 Dead surface rendered unreachable: G1+G2 test; anchor `.rules:586-602`,`:734-752`,`:752-775` (no upstream anchor — proposed)
- [x] Checkpoint P1: all 4 decision tests falsifiable + anchored

## Phase 2 — Audit slices
- [x] A1 D-seam-compatibility audit: no category authorizes editing upstream outside a D-seam
- [x] A2 Cross-category overlap: decision-test-disjoint + co-occurrence resolution documented
- [x] Checkpoint P2: D-seam-compat passes; overlap matrix complete

## Phase 3 — Ranking + self-assessment
- [x] R1 MCDA: 5 criteria, composite scores, ±20% sensitivity, robust/fragile
- [x] R2 Metacognition: Brier-scored coverage prediction over 12-case ground truth
- [x] Checkpoint P3: robust/fragile + Brier recorded

## Phase 4 — Finalize
- [x] F1 Write principles document; acceptance-criteria checklist all hold