#!/usr/bin/env bash
# Self-test for the unsafe-forbid gate (check-unsafe-forbid.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-unsafe-forbid.sh per CI gate
# sweep follow-up issue #6. The gate's "0 = 0" failure mode (zero crate roots
# discovered → zero violations → trivially passes) was flagged in the sweep.
# This self-test pins both failure modes:
#   1. Missing attribute: a crate root whose line 1 does NOT have
#      `forbid(unsafe_code)` or `deny(unsafe_code)`.
#   2. Empty: a SCAN_DIRS with zero matching crate dirs must exit 0 (no
#      violations) without crashing — pinning the silent-disconnection path.
#
# Design: temp crate dirs are populated with minimal Cargo.toml + lib root
# files, and the gate is invoked with SCAN_DIRS pointed at the temp globs.
# Each case asserts the gate exits with the expected code and keyword.
#
# Exit codes:
#   0 — all cases behaved as expected (gate is alive)
#   1 — at least one case did not behave as expected (gate is vacuous or broken)
#
# Usage: bash scripts/check-unsafe-forbid-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-unsafe-forbid.sh"

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
# Case 1: missing attribute — a crate root whose line 1 has NO
# `forbid(unsafe_code)` / `deny(unsafe_code)`. The gate must exit 1 with
# "missing an unsafe-gating attribute".
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/crates/selftest-bad/src"
cat > "$TMPDIR/crates/selftest-bad/Cargo.toml" <<'EOF'
[package]
name = "selftest-bad"
version = "0.0.0"

[lib]
path = "src/hkask_bad.rs"
EOF
# Line 1 deliberately lacks the unsafe-gating attribute. The comment text
# must NOT contain the literal tokens `forbid(unsafe_code)` or
# `deny(unsafe_code)` or the gate's grep would match it.
cat > "$TMPDIR/crates/selftest-bad/src/hkask_bad.rs" <<'EOF'
// selftest: this root is missing the unsafe-gating attribute
pub fn nothing() {}
EOF

set +e
CASE1_OUT=$(SCAN_DIRS="$TMPDIR/crates/*/" bash "$GATE" 2>&1)
CASE1_RC=$?
set -e

if [ "$CASE1_RC" -ne 1 ]; then
  echo "FAIL (case 1 — missing attribute): expected exit 1, got $CASE1_RC"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE1_OUT" | grep -q "missing an unsafe-gating attribute"; then
  echo "FAIL (case 1 — missing attribute): exit 1 but 'missing an unsafe-gating attribute' not in output"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — missing attribute): gate detected the missing attribute"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: empty SCAN_DIRS — zero matching crate dirs must exit 0 (no
# violations) without crashing. This pins the silent-disconnection path:
# 0 roots = 0 violations is the correct verdict here, NOT a vacuous pass.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/empty"

set +e
CASE2_OUT=$(SCAN_DIRS="$TMPDIR/empty/*/" bash "$GATE" 2>&1)
CASE2_RC=$?
set -e

if [ "$CASE2_RC" -ne 0 ]; then
  echo "FAIL (case 2 — empty): expected exit 0, got $CASE2_RC"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE2_OUT" | grep -q "OK"; then
  echo "FAIL (case 2 — empty): exit 0 but 'OK' not in output"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — empty): gate exited 0 on empty scan dirs"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: unsafe-forbid gate is alive (missing-attribute + empty both pinned)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
  echo "The gate is vacuous or broken — see kask/scripts/check-unsafe-forbid.sh."
  exit 1
fi
