# Security Audit Log — `check-convergence-weights.sh` Gate Deletion

**Date:** 2026-08-17
**Cycle:** Post-cycle-1 follow-up (CI gate sweep)
**Scope:** CI gate inventory accuracy
**Status:** Resolved

## Summary

The `check-convergence-weights.sh` CI gate was deleted during the hkask-templates
refactor (convergence-check templates → deterministic Kata primitives) but no
audit-log entry recorded the deletion. The baseline audit log
(`2026-07-22-baseline.md:107`) still lists `check-convergence-weights.sh ✓` as a
verified CI gate, which would confuse a future auditor looking for the gate.

This entry records the deletion, the rationale, and where the invariant migrated.

## What was deleted

- **Gate script:** `kask/scripts/check-convergence-weights.sh`
- **CI workflow step:** the `Check convergence weights` step in
  `.github/workflows/kask-ci.yml` (removed when the gate was deleted)

## Why it was deleted

The gate globbed `registry/templates/*/convergence-check.j2` and asserted that
the `weight: 0.NN` entries in each template summed to 1.0 (±0.02 tolerance).
The `convergence-check.j2` template name no longer exists: those templates were
replaced by deterministic Kata `compute` primitives in
`kask/crates/hkask-templates/src/compute.rs:540-560` (the `── Kata convergence
primitives ──` block). The gate's output had degraded to
`0 checked, 0 skipped — no weights found`, a success message that actually meant
"sensor disconnected" — the exact silent-disconnection failure mode flagged in
the CI gate sweep.

## Where the invariant migrated

The weight-sum invariant survived the migration. It is now enforced by
`kask/crates/hkask-templates/tests/evaluate_weight_sums.rs`, which scans the
new location (`*-evaluate.j2` templates) for the same `weight: 0.NN` syntax and
asserts the same ±0.02 tolerance. The test's module doc explicitly documents
that it replaces the deleted shell gate.

The test runs as part of the normal `cargo nextest` job in CI
(`.github/workflows/kask-ci.yml` `test` job), so the invariant is still
enforced — just at a different layer (Rust test, not shell gate).

## Baseline audit log correction

The baseline entry at `2026-07-22-baseline.md:107` (`check-convergence-weights.sh ✓`)
is superseded by this entry. A future auditor reading the baseline should treat
that line as "deleted, see 2026-08-17-convergence-weights-deletion.md" rather
than "currently enforced."

## Related

- CI gate sweep follow-up: `kask/docs/plans/ci-sweep-follow-up-issues.md` issue #4
- Silent-disconnection audit: same doc, issue #6
- Replacement test: `kask/crates/hkask-templates/tests/evaluate_weight_sums.rs`
- Kata primitives: `kask/crates/hkask-templates/src/compute.rs:540-560`
