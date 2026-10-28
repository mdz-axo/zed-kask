#!/usr/bin/env bash
# Self-test for the forecast-conformance gate (check-forecast-conformance.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-forecast-conformance.sh per
# CI gate sweep follow-up issue #6. The gate's "0 = 0" failure mode (empty
# primitive list and empty contract list both being 0, trivially equal) was
# flagged in the sweep. The gate already has an empty-parse guard (lines 57-60),
# but this self-test pins both failure modes the gate is supposed to catch:
#   1. Orphan: a primitive in the lib not named in the contract.
#   2. Dangle: a contract reference not in the lib.
#
# Design: temp LIB and CONTRACT files are populated with minimal content, and
# the gate is invoked with LIB/CONTRACT env vars pointed at the temp files.
# Each case injects one violation class and asserts the gate exits 1 with the
# expected keyword. A third case asserts the empty-parse guard fires when the
# lib has no #[must_use] pub fns.
#
# Exit codes:
#   0 — all synthetic violations were detected (gate is alive)
#   1 — at least one synthetic violation was NOT detected (gate is vacuous)
#
# Usage: bash scripts/check-forecast-conformance-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-forecast-conformance.sh"

if [ ! -f "$GATE" ]; then
  echo "FAIL: gate not found at $GATE"
  exit 1
fi

TMPDIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMPDIR"
}
trap cleanup EXIT

failures=0

# ──────────────────────────────────────────────────────────────────────────
# Case 1: orphan — a #[must_use] pub fn in the lib NOT named in the contract.
# The gate must exit 1 with "not in conformance contract".
# ──────────────────────────────────────────────────────────────────────────
cat > "$TMPDIR/lib.rs" <<'EOF'
#[must_use]
pub fn selftest_orphan_fn() -> f64 { 0.0 }
EOF
cat > "$TMPDIR/contract.md" <<'EOF'
# Selftest contract

## Deterministic Primitives

| Stage | Function | Notes |
|-------|----------|-------|
| (empty — no entry for selftest_orphan_fn) | `` | |
EOF

set +e
CASE1_OUT=$(LIB="$TMPDIR/lib.rs" CONTRACT="$TMPDIR/contract.md" bash "$GATE" 2>&1)
CASE1_RC=$?
set -e

if [ "$CASE1_RC" -ne 1 ]; then
  echo "FAIL (case 1 — orphan): expected exit 1, got $CASE1_RC"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE1_OUT" | grep -q "not in conformance contract"; then
  echo "FAIL (case 1 — orphan): exit 1 but 'not in conformance contract' not in output"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — orphan): gate detected the orphan primitive"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: dangle — a contract reference NOT in the lib.
# The gate must exit 1 with "not in hkask-forecast".
# ──────────────────────────────────────────────────────────────────────────
cat > "$TMPDIR/lib.rs" <<'EOF'
#[must_use]
pub fn real_fn() -> f64 { 0.0 }
EOF
cat > "$TMPDIR/contract.md" <<'EOF'
# Selftest contract

## Deterministic Primitives

| Stage | Function | Notes |
|-------|----------|-------|
| dangle | `selftest_dangle_fn` | does not exist in lib |
| real | `real_fn` | exists |
EOF

set +e
CASE2_OUT=$(LIB="$TMPDIR/lib.rs" CONTRACT="$TMPDIR/contract.md" bash "$GATE" 2>&1)
CASE2_RC=$?
set -e

if [ "$CASE2_RC" -ne 1 ]; then
  echo "FAIL (case 2 — dangle): expected exit 1, got $CASE2_RC"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE2_OUT" | grep -q "not in hkask-forecast"; then
  echo "FAIL (case 2 — dangle): exit 1 but 'not in hkask-forecast' not in output"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — dangle): gate detected the dangling contract reference"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 3: empty-parse guard — lib with no #[must_use] pub fns.
# The gate must exit 1 with "no #[must_use] pub fns parsed" (the
# silent-disconnection guard added in the sweep).
# ──────────────────────────────────────────────────────────────────────────
cat > "$TMPDIR/lib.rs" <<'EOF'
// No #[must_use] pub fns here — parsing should yield 0, triggering the guard.
pub fn not_must_use() -> f64 { 0.0 }
EOF
cat > "$TMPDIR/contract.md" <<'EOF'
# Selftest contract

## Deterministic Primitives

| Stage | Function | Notes |
|-------|----------|-------|
EOF

set +e
CASE3_OUT=$(LIB="$TMPDIR/lib.rs" CONTRACT="$TMPDIR/contract.md" bash "$GATE" 2>&1)
CASE3_RC=$?
set -e

if [ "$CASE3_RC" -ne 1 ]; then
  echo "FAIL (case 3 — empty parse): expected exit 1, got $CASE3_RC"
  printf '%s\n' "$CASE3_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE3_OUT" | grep -qF "pub fns parsed"; then
  echo "FAIL (case 3 — empty parse): exit 1 but 'pub fns parsed' not in output"
  printf '%s\n' "$CASE3_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 3 — empty parse): guard fired on zero primitives"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: forecast-conformance gate is alive (orphan + dangle + empty-parse all detected)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not detect their synthetic violation"
  echo "The gate is vacuous — see kask/scripts/check-forecast-conformance.sh."
  exit 1
fi
