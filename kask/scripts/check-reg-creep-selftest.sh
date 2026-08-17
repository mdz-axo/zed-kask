#!/usr/bin/env bash
# Self-test for the reg-creep gate (check-reg-creep.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-reg-creep.sh per CI gate sweep
# follow-up issue #6. The gate reported green while its output could have been
# the silent-disconnection pattern ("no reg.* targets found" → exit 0) flagged
# in the sweep. This self-test injects a synthetic unregistered reg.* target
# into a temp source tree and asserts the gate actually fails — so a future
# regression in check-reg-creep.sh (e.g. a broken grep, an inverted condition)
# cannot silently re-vacate the gate.
#
# Design: a temp dir is populated with a single .rs file containing an
# unregistered `target: "reg.selftest.fake"` string. The gate is invoked with
# SCAN_DIRS pointed at the temp dir and REGISTRY pointed at a temp copy of the
# real event.rs (so the real registry is never touched). The synthetic target
# is NOT in the registry, so the gate must exit 1.
#
# A second case asserts the empty-scan case still exits 0 (the "no targets
# found" path) — but only when the registry is real. This pins the
# silent-disconnection behavior: an empty scan is exit 0, but a non-empty scan
# with an unregistered target is exit 1.
#
# Exit codes:
#   0 — the synthetic violation was detected (gate is alive)
#   1 — the synthetic violation was NOT detected (gate is vacuous)
#
# Usage: bash scripts/check-reg-creep-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-reg-creep.sh"
REAL_REGISTRY="$KASK_ROOT/crates/hkask-types/src/event.rs"

if [ ! -f "$GATE" ]; then
  echo "FAIL: gate not found at $GATE"
  exit 1
fi
if [ ! -f "$REAL_REGISTRY" ]; then
  echo "FAIL: real registry not found at $REAL_REGISTRY"
  exit 1
fi

TMPDIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMPDIR"
}
trap cleanup EXIT

# Copy the real registry so the self-test's SCAN_DIRS redirect doesn't break
# the registry path. The synthetic target is NOT in this registry.
cp "$REAL_REGISTRY" "$TMPDIR/event.rs"

# Case 1: a source file with an unregistered reg.* target → gate must fail.
mkdir -p "$TMPDIR/src"
cat > "$TMPDIR/src/selftest.rs" <<'EOF'
// selftest: this file deliberately contains an unregistered reg.* target.
// The string `reg.selftest.fake` is not in CANONICAL_NAMESPACES.
fn emit() {
    tracing::warn!(target: "reg.selftest.fake", "selftest synthetic violation");
}
EOF

set +e
CASE1_OUT=$(SCAN_DIRS="$TMPDIR/src/" REGISTRY="$TMPDIR/event.rs" bash "$GATE" 2>&1)
CASE1_RC=$?
set -e

failures=0

if [ "$CASE1_RC" -ne 1 ]; then
  echo "FAIL (case 1 — unregistered target): expected exit 1, got $CASE1_RC"
  echo "  output:"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE1_OUT" | grep -q "UNREGISTERED"; then
  echo "FAIL (case 1 — unregistered target): exit 1 but 'UNREGISTERED' not in output"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — unregistered target): gate detected the synthetic violation"
fi

# Case 2: an empty scan dir → gate must exit 0 with "no reg.* targets found".
# This pins the silent-disconnection behavior so a future change can't invert
# it (e.g. exit 1 on empty, which would hide a real disconnection behind a
# different failure mode).
mkdir -p "$TMPDIR/empty"
set +e
CASE2_OUT=$(SCAN_DIRS="$TMPDIR/empty/" REGISTRY="$TMPDIR/event.rs" bash "$GATE" 2>&1)
CASE2_RC=$?
set -e

if [ "$CASE2_RC" -ne 0 ]; then
  echo "FAIL (case 2 — empty scan): expected exit 0, got $CASE2_RC"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE2_OUT" | grep -q "no reg.* targets found"; then
  echo "FAIL (case 2 — empty scan): exit 0 but 'no reg.* targets found' not in output"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — empty scan): gate exited 0 with the expected message"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: reg-creep gate is alive (synthetic violation detected, empty-scan path pinned)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not pass"
  echo "The gate is vacuous — see kask/scripts/check-reg-creep.sh."
  exit 1
fi
