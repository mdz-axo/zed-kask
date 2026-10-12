#!/usr/bin/env bash
# Self-test for the string-errors gate (check-string-errors.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-string-errors.sh per CI gate
# sweep follow-up issue #6. The gate matches `Result<_, String>` anti-patterns
# structurally (ast-grep rule, 2026-10-11); a broken rule or inverted
# condition would silently vacate it. This self-test injects synthetic
# fixtures and asserts the gate's verdict on each.
#
# Four cases (cases 3-4 pin the structural pass's two wins over the former
# line-based grep — both were its documented blind spots):
#   1. Violation: a .rs file with `-> Result<(), String>` → gate must exit 1.
#   2. Clean: a .rs file with `-> Result<(), MyError>` → gate must exit 0.
#   3. Multi-line violation: `Result<\n (),\n String\n>` split across
#      lines → gate must exit 1 (the line-based pass could not see it).
#   4. Nested-String Ok type: `Result<(String, String), MyError>` → gate
#      must exit 0 (the line-based pass false-positived here).
#
# Exit codes:
#   0 — all cases passed (gate is alive)
#   1 — at least one case failed (gate is vacuous)
#
# Usage: bash scripts/check-string-errors-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-string-errors.sh"

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
# Case 1: violation — a .rs file with `-> Result<(), String>`.
# The gate must exit 1 with "FAIL".
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/crates/hkask-selftest/src"
cat > "$TMPDIR/crates/hkask-selftest/src/lib.rs" <<'EOF'
// selftest: this file deliberately contains the Result<_, String> anti-pattern.
pub fn bad_function() -> Result<(), String> {
    Err("selftest synthetic violation".to_string())
}
EOF

set +e
CASE1_OUT=$(SCAN_DIRS="$TMPDIR/crates/hkask-*" bash "$GATE" 2>&1)
CASE1_RC=$?
set -e

if [ "$CASE1_RC" -ne 1 ]; then
  echo "FAIL (case 1 — violation): expected exit 1, got $CASE1_RC"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE1_OUT" | grep -q "FAIL"; then
  echo "FAIL (case 1 — violation): exit 1 but 'FAIL' not in output"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — violation): gate detected the Result<_, String> pattern"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: clean — a .rs file with `-> Result<(), MyError>` (no String).
# The gate must exit 0 with "OK".
# ──────────────────────────────────────────────────────────────────────────
cat > "$TMPDIR/crates/hkask-selftest/src/lib.rs" <<'EOF'
// selftest: this file uses a proper error enum, not String.
pub enum MyError { Bad }
pub fn good_function() -> Result<(), MyError> {
    Err(MyError::Bad)
}
EOF

set +e
CASE2_OUT=$(SCAN_DIRS="$TMPDIR/crates/hkask-*" bash "$GATE" 2>&1)
CASE2_RC=$?
set -e

if [ "$CASE2_RC" -ne 0 ]; then
  echo "FAIL (case 2 — clean): expected exit 0, got $CASE2_RC"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE2_OUT" | grep -q "OK"; then
  echo "FAIL (case 2 — clean): exit 0 but 'OK' not in output"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — clean): gate passed on proper error enum"
fi

# ──────────────────────────────────────────────────────────────────────
# Case 3: multi-line violation — `Result<\n (),\n String\n>` split across
# lines. The structural pass must catch it (the line-based pass could not).
# ──────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/case3/crates/hkask-selftest/src"
cat > "$TMPDIR/case3/crates/hkask-selftest/src/lib.rs" <<'EOF'
pub fn bad_multiline() -> Result<
    (),
    String
> {
    Err("selftest synthetic multi-line violation".to_string())
}
EOF

set +e
CASE3_OUT=$(SCAN_DIRS="$TMPDIR/case3/crates/hkask-*" bash "$GATE" 2>&1)
CASE3_RC=$?
set -e

if [ "$CASE3_RC" -ne 1 ]; then
  echo "FAIL (case 3 — multi-line violation): expected exit 1, got $CASE3_RC"
  printf '%s\n' "$CASE3_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE3_OUT" | grep -q "FAIL"; then
  echo "FAIL (case 3 — multi-line violation): exit 1 but 'FAIL' not in output"
  printf '%s\n' "$CASE3_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 3 — multi-line violation): structural pass sees the split signature"
fi

# ──────────────────────────────────────────────────────────────────────
# Case 4: nested-String Ok type — `Result<(String, String), MyError>`.
# The structural pass must NOT flag it (the line-based pass false-positived
# here — its documented blind spot 2).
# ──────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/case4/crates/hkask-selftest/src"
cat > "$TMPDIR/case4/crates/hkask-selftest/src/lib.rs" <<'EOF'
pub struct MyError;
pub fn nested_ok_string() -> Result<(String, String), MyError> {
    Err(MyError)
}
EOF

set +e
CASE4_OUT=$(SCAN_DIRS="$TMPDIR/case4/crates/hkask-*" bash "$GATE" 2>&1)
CASE4_RC=$?
set -e

if [ "$CASE4_RC" -ne 0 ]; then
  echo "FAIL (case 4 — nested-String Ok type): expected exit 0, got $CASE4_RC"
  printf '%s\n' "$CASE4_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 4 — nested-String Ok type): no false positive on the Ok-side String"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: string-errors gate is alive (violation detected, clean path pinned)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not pass"
  echo "The gate is vacuous — see kask/scripts/check-string-errors.sh."
  exit 1
fi
