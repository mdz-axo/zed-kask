#!/usr/bin/env bash
# Self-test for the reg-canonical gate (check-reg-canonical.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-reg-canonical.sh per CI gate
# sweep follow-up issue #6. The gate uses HIERARCHICAL (ancestor) matching:
# a sub-namespace passes if any prefix is registered. A broken ancestor-trimming
# loop or inverted condition would silently vacate it.
#
# Two cases:
#   1. Violation: a .rs file with `target: "reg.selftest.fake"` where neither
#      `reg.selftest.fake` nor any ancestor is in the registry → gate must
#      exit 1 with "non-canonical".
#   2. Clean: a .rs file with `target: "reg.skill.lifecycle"` where an ancestor
#      (`reg.skill`) is in the registry → gate must exit 0 (hierarchical match).
#      This pins the ancestor-matching behavior — a future change that breaks
#      ancestor trimming would turn this into a false failure.
#
# Exit codes:
#   0 — all cases passed (gate is alive)
#   1 — at least one case failed (gate is vacuous)
#
# Usage: bash scripts/check-reg-canonical-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-reg-canonical.sh"
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

failures=0

# ──────────────────────────────────────────────────────────────────────────
# Case 1: violation — a .rs file with an unregistered reg.* target.
# The gate must exit 1 with "non-canonical".
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/src"
cat > "$TMPDIR/src/selftest.rs" <<'EOF'
// selftest: this file deliberately contains an unregistered reg.* target.
fn emit() {
    tracing::warn!(target: "reg.selftest.fake", "selftest synthetic violation");
}
EOF

set +e
CASE1_OUT=$(SCAN_DIRS="$TMPDIR/src/" REGISTRY="$TMPDIR/event.rs" TEMPLATE_DIR="$TMPDIR/no-templates/" bash "$GATE" 2>&1)
CASE1_RC=$?
set -e

if [ "$CASE1_RC" -ne 1 ]; then
  echo "FAIL (case 1 — violation): expected exit 1, got $CASE1_RC"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE1_OUT" | grep -q "non-canonical"; then
  echo "FAIL (case 1 — violation): exit 1 but 'non-canonical' not in output"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — violation): gate detected the non-canonical target"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: clean — a .rs file with a registered ancestor (hierarchical match).
# `reg.skill` is in the real registry, so `reg.skill.lifecycle` must pass.
# The gate must exit 0 with "OK".
# ──────────────────────────────────────────────────────────────────────────
cat > "$TMPDIR/src/selftest.rs" <<'EOF'
// selftest: reg.skill.lifecycle has ancestor reg.skill in the registry.
fn emit() {
    tracing::warn!(target: "reg.skill.lifecycle", "selftest clean");
}
EOF

set +e
CASE2_OUT=$(SCAN_DIRS="$TMPDIR/src/" REGISTRY="$TMPDIR/event.rs" TEMPLATE_DIR="$TMPDIR/no-templates/" bash "$GATE" 2>&1)
CASE2_RC=$?
set -e

if [ "$CASE2_RC" -ne 0 ]; then
  echo "FAIL (case 2 — clean/ancestor): expected exit 0, got $CASE2_RC"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE2_OUT" | grep -q "OK"; then
  echo "FAIL (case 2 — clean/ancestor): exit 0 but 'OK' not in output"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — clean/ancestor): gate passed on hierarchical ancestor match"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: reg-canonical gate is alive (violation detected, ancestor-match pinned)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not pass"
  echo "The gate is vacuous — see kask/scripts/check-reg-canonical.sh."
  exit 1
fi
