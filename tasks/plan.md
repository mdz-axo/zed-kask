# Plan — Upstream-Zed Removal Principles for the zed-kask Seam

> **DC+BIBO metadata:** title=Upstream-Zed Removal Principles; creator=zed-kask agent; date=2026-08-11; type=bibo:Document.
> **PKO anchor:** pko:Procedure targeting pko:ProcedureTarget "a ranked, decision-test-bearing taxonomy of upstream-removal principles compatible with the D-seam meta-constraint."

## Overview

Produce a consolidated, testable principle set governing **what to remove from
upstream Zed** (everything outside `kask/` and outside the named D1–D24 D-seams)
when maintaining the zed-kask fork, and **why**. The D-seam discipline is a
*boundary on the mechanism* (never edit upstream outside a named D-seam; push
the fix into a `kask/` crate behind a D-seam and pin with a test), not a removal
*reason*. These principles govern the *justification* for removal; the D-seam
governs the *execution*. No principle may authorize forking upstream outside a
D-seam.

## Architecture decisions

- **4 ranked categories** (the task seeds 3; evidence demands splitting
  "elegance" into two falsifiable forms — *redundant surface superseded by
  kask* and *dead surface rendered unreachable* — because they have distinct
  decision tests and distinct failure modes).
- Pure "elegance/simplification" (removing code because it is "ugly" or "could
  be cleaner") is **rejected** — it would authorize forking upstream. Only the
  *provably unreachable* form (essentialist G1+G2) survives as Category 4.
- Categories are **decision-test-disjoint**, not instance-disjoint: a single
  removal may satisfy multiple tests; the agent applies all and classifies by
  the test that captures the load-bearing risk. Co-occurrence resolution rules
  are documented in the overlap slice.
- Every category carries a binary/script-checkable decision test, a failure
  mode, anchoring evidence (file:line or "no anchor — proposed"), and a scope
  boundary naming what it does NOT authorize.

## Phased task list with checkpoints

### Phase 1 — Category slices (parallelizable; no inter-slice dependencies)

| Slice | Title | AC (decision-test checkpoint) | Verification | Deps | Scope |
| --- | --- | --- | --- | --- | --- |
| C1 | Install/runtime collision principle | Decision test fires on `check-desktop-no-collision.sh` + `check-zed-isolation.sh`; failure mode = hijack of real Zed; anchor = `.rules:787-821`, DIVERGENCE.md D7 L28, D16 L37 | run both scripts; grep `.desktop` templates for forbidden strings | None | S |
| C2 | Platform-scope principle | Decision test = `#[cfg(target_os=...)]`-gated non-Linux OR non-Linux bundler absent from build matrix; negative test excludes cross-platform libs Linux uses; anchor = DIVERGENCE.md D7 L28 | grep `target_os` + `bundle-mac`/`bundle-windows`/`snap-build`; confirm fail-closed | None | S |
| C3 | Redundant surface superseded by kask | Two-part test: (a) kask replacement is wired+load-bearing (enforcement point grepped) AND (b) retaining upstream causes a concrete defect; anchor = D1 L22 (catalog budget), D1 L22 (desc-length), D3 L24 (daemon transport), D10 L31, `.rules:602-631` | grep replacement call site; grep upstream surface for the defect it causes | None | M |
| C4 | Dead surface rendered unreachable (sharpened elegance) | Essentialist G1 (delete → complexity reappears at call sites?) + G2 (any test/path asserts reachability?); both must be NO + no `.rules`/DIVERGENCE invariant depends on it; anchor = `.rules:586-602`, `:734-752`, `:752-775` (all kask-side; upstream form = "no existing anchor — proposed") | grep production callers; grep test assertions of reachability | None | M |

### Phase 2 — Audit slices (depend on Phase 1)

| Slice | Title | AC | Verification | Deps | Scope |
| --- | --- | --- | --- | --- | --- |
| A1 | D-seam-compatibility audit | For each of C1–C4, confirm the scope boundary does NOT authorize editing upstream outside a D-seam; any upstream edit must be expressible as a D-seam entry + test pin | read each scope-boundary clause; cross-check against DIVERGENCE.md D1–D24 + `.rules:248-276` | C1,C2,C3,C4 | S |
| A2 | Cross-category overlap check | Document pairwise overlaps + co-occurrence resolution (classify by load-bearing-risk test); confirm categories are decision-test-disjoint | build the overlap matrix; confirm no two categories share a decision test | C1,C2,C3,C4 | S |

### Phase 3 — Ranking + self-assessment (depends on Phase 1+2)

| Slice | Title | AC | Verification | Deps | Scope |
| --- | --- | --- | --- | --- | --- |
| R1 | MCDA ranking + ±20% sensitivity | 5 criteria (merge-friction, install-safety, behavior-preservation, decision-rule testability, blast-radius); composite scores; robust vs fragile classification | recompute composites under ±20% weight perturbation; record rank reversals | C1-C4,A1,A2 | M |
| R2 | Metacognition coverage prediction | Brier-scored prediction that the taxonomy covers the next 5 real removal decisions; iterate taxonomy if poor; stop after 2 failed predictions | apply taxonomy to ground-truth prior-removal set; compute coverage + Brier | C1-C4,A1,A2,R1 | M |

### Phase 4 — Finalize

| Slice | Title | AC | Verification | Deps | Scope |
| --- | --- | --- | --- | --- | --- |
| F1 | Write principles document | ≥3 ranked categories each with decision test + failure mode + anchor; no category authorizes upstream edit outside D-seam; elegance sharpened or rejected; mcda sensitivity report; metacognition coverage prediction; all citations real file:line | acceptance-criteria checklist | C1-C4,A1,A2,R1,R2 | S |

**Checkpoint after Phase 1:** all 4 category decision tests are falsifiable and
anchored. **Checkpoint after Phase 2:** D-seam-compatibility audit passes (no
category authorizes upstream fork) and overlap matrix is documented.
**Checkpoint after Phase 3:** mcda robust/fragile classification + metacognition
Brier score recorded. **Final checkpoint:** acceptance criteria all hold.

## Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Category 4 (dead surface) has no upstream-side anchor — all instances are kask-side | An untested principle applied to upstream could over-remove | Mark "no existing anchor — proposed"; prescribe disable-behind-D-seam + test-pin, NOT file deletion, for upstream |
| "Redundant surface" (C3) and "collision" (C1) co-occur (D16 update actions) | Mis-classification hides the safety-critical reason | Co-occurrence resolution: classify by load-bearing-risk test (collision outranks redundancy) |
| Pure "elegance" leaks back in as a removal reason | Forks upstream silently | Reject pure elegance explicitly; only G1+G2-proven unreachability qualifies |
| Metacognition coverage prediction is un-verifiable (no future removals yet) | Brier score is self-referential | Score against the *retrospective* ground-truth prior-removal set (10 upstream-side cases) as a proxy; note the limitation |

## Open questions

1. Should "behavior-correctness removal" (D4 `hkask-guard` — RoleOverride false
   positives) be a 5th upstream category, or does it fold into C3 (the kask
   replacement — direct inference ports — supersedes the guard)? **Resolved in
   A2:** for *upstream* surface, behavior-correctness is a *modification* reason
   (D11/D13/D14/D15 pattern), not a *removal* reason; the rule "file an upstream
   issue, don't fork-fix" covers real upstream bugs. Folds into C3 only when
   kask provides a wired replacement that makes the upstream behavior wrong.
2. Is the retrospective ground-truth set (10 cases) large enough for a calibrated
   Brier score? **Noted limitation** — reported as a small-n prediction.

## Refinement history

- **Iteration 1 (decompose):** initial 3-category plan (collision, platform,
  elegance). Evaluate flagged elegance as unfalsifiable (score 0.6 on AC
  specificity) and C3/C4 as conflated (one slice trying to cover two distinct
  tests). Refinement directive: split elegance into C3 (redundant, defect-based)
  and C4 (dead, reachability-based); reject pure elegance.
- **Iteration 2 (re-decompose):** 4-category plan with the split. Evaluate:
  sizing 0.05, vertical-slice 0.05, AC-specificity 0.05, dependency 0.05,
  checkpoint 0.05, red-flag 0.05 → weighted_total 0.05 ≤ 0.15. Quality gate
  passes (no criterion > 0.30). Plan is stable (Cauchy: Δ ≤ 0.02).