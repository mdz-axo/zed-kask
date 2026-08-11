# Kask↔Zed Seam Audit — Todo

## Phase 1 — Plan
- [x] T1 Read `DIVERGENCE.md` D1–D24 and map each D-seam to its crate/file
- [x] T2 Verify each `.rules` convention prior against `crates/` (live vs phantom)

## Phase 2 — Do (3 parallel tracks)
- [x] T3 kali-audit: 8 priority surfaces, file:line per finding, defense-layer coverage table
- [x] T4 refactor-architecture: dead-surface + deepening, caller-count evidence, deletion test
- [x] T5 ui-layout-discipline: measured-layout + interaction language, Zed-primitive gap

## Phase 3 — Check / adjudicate
- [x] T6 Adjudicate: pragmatic-semantics + pragmatic-cybernetics + essentialist per finding
- [x] T7 MCDA: ranked table + ±20% sensitivity analysis

## Phase 4 — Act / remediate
- [ ] T8 Apply MCDA top-ranked remediations surviving essentialist (DEFERRED — see plan; no hard-stop)
- [x] T9 metacognition: Brier-scored prediction per track

## Phase 5 — Bundle & manifest
- [x] T10 FlowDef registry crate: manifest.yaml + .j2 templates + SKILL.md + validation

## Acceptance gate (self-check)
- [x] Every security finding has `file:line` OR is marked `deferred` with reason
- [x] No change touches a file outside `kask/` or a non-D-seam (no edits applied; vacuously satisfied)
- [x] No behavioral remediation applied without a test (none applied)
- [ ] MCDA top-3 stable across last two slices — N/A (single pass; sensitivity analysis shows rank-1 stable, ranks 2-3 weight-sensitive)
- [x] FlowDef manifest validates; every `condition:` references a real `step_N_result` key
- [x] Every `lisp.eval` gate asserts a structural invariant (count / completeness / exclusivity)
- [x] metacognition Brier scores recorded per track
- [x] No `unwrap_or(0)` on a regulation-loop sense input introduced (none found; RA-08 flagged a dead one)